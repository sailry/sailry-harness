package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"path"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const (
	mediaPromptBytes  = 32000
	mediaAnswerBytes  = 64 << 10
	mediaInspectIdle  = 120 * time.Second
	defaultSpeechName = "alloy"
)

var mediaToolSchema = json.RawMessage(`{"type":"object","properties":{"prompt":{"type":"string"}},"required":["prompt"]}`)

// mediaRoute is the model one media purpose runs on for one turn.
type mediaRoute struct {
	provider protocol.ConversationProvider
	model    string
	channel  model.MediaChannel
	// assigned is false when the purpose follows the conversation model.
	assigned bool
}

// routeMedia applies the three routing steps to the assignments frozen when
// the turn was admitted: an assigned model is always used; otherwise the
// conversation model is used when it has the capability; otherwise the tool is
// not offered. An assigned model that could not run at admission offers
// nothing, because no other model silently replaces it.
func routeMedia(inv *store.AgentInvocation, kind protocol.MediaKind) (mediaRoute, bool) {
	provider, name, assigned := inv.Provider, inv.Turn.Config.Model, false
	if assignment, bound := inv.Media[kind]; bound {
		provider, name, assigned = assignment.Provider, assignment.Model, true
	}
	if provider == nil {
		return mediaRoute{}, false
	}
	for _, entry := range provider.Models {
		if entry.ID == name {
			channel, supported := model.MediaChannelFor(kind, *provider, entry)
			return mediaRoute{provider: *provider, model: name, channel: channel, assigned: assigned}, supported
		}
	}
	return mediaRoute{}, false
}

func mediaOperationKind(operation protocol.ToolOperation) (protocol.MediaKind, bool) {
	switch operation {
	case protocol.ToolOperationInspectMedia:
		return protocol.MediaKindVision, true
	case protocol.ToolOperationGenerateImage:
		return protocol.MediaKindImage, true
	case protocol.ToolOperationGenerateVideo:
		return protocol.MediaKindVideo, true
	}
	return "", false
}

// mediaTools binds the media operations a turn's packages declare. A purpose
// without a route has no native tool, so its declared tool is not offered.
func (node *Node) mediaTools(ctx context.Context, inv *store.AgentInvocation) ([]engine.Tool, error) {
	var infos []protocol.PluginInfo
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		for _, reference := range inv.Turn.Plugins {
			info, err := store.ResolvePluginInfo(ctx, db, reference)
			if err != nil {
				return err
			}
			infos = append(infos, info)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	var schema map[string]any
	if err := json.Unmarshal(mediaToolSchema, &schema); err != nil {
		return nil, err
	}
	var tools []engine.Tool
	for _, info := range infos {
		if info.Extension == nil || info.Extension.Host == nil {
			continue
		}
		for _, declaration := range info.Extension.Tools {
			if declaration.Handler == nil || declaration.Handler.Operation == nil {
				continue
			}
			operation := *declaration.Handler.Operation
			kind, media := mediaOperationKind(operation)
			if !media {
				continue
			}
			route, offered := routeMedia(inv, kind)
			if !offered {
				continue
			}
			name := plugins.ToolAlias(info.Summary.Name, declaration.Name)
			tools = append(tools, node.mediaTool(inv, plugins.PackageReference(info), name, operation, kind, route, schema))
		}
	}
	return tools, nil
}

func mediaFault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}

func validMediaPrompt(prompt string) bool {
	return strings.TrimSpace(prompt) != "" && len(prompt) <= mediaPromptBytes && utf8.ValidString(prompt)
}

func (node *Node) mediaTool(inv *store.AgentInvocation, owner protocol.PluginReference, name string, operation protocol.ToolOperation, kind protocol.MediaKind, route mediaRoute, schema map[string]any) engine.Tool {
	readOnly := kind == protocol.MediaKindVision
	var mu sync.Mutex
	calls := map[string]*protocol.Request{}
	tool := engine.Tool{Definition: fantasy.FunctionTool{Name: name, InputSchema: schema}, SchemaJSON: mediaToolSchema, ReadOnly: readOnly, Approval: !readOnly,
		Binding: &store.ApprovalBinding{Owner: owner, Tool: name, Operation: &operation}}
	tool.Prepare = func(_ context.Context, call canonical.ToolRequest) error {
		action := protocol.MediaAction{}
		switch kind {
		case protocol.MediaKindVision:
			var input protocol.MediaActionInspectPayload
			if json.Unmarshal(call.Arguments, &input) != nil || !validMediaPrompt(input.Prompt) {
				return mediaFault(protocol.ErrorCodeInvalidRequest, "invalid media arguments")
			}
			if value, ok := input.Source.Data.(*string); ok {
				if err := core.CheckFilePath(*value); err != nil {
					return err
				}
			}
			action = protocol.MediaAction{Kind: "inspect", Data: &input}
		default:
			var input protocol.MediaActionImagePayload
			if json.Unmarshal(call.Arguments, &input) != nil || !validMediaPrompt(input.Prompt) {
				return mediaFault(protocol.ErrorCodeInvalidRequest, "invalid media arguments")
			}
			extension := ".png"
			if kind == protocol.MediaKindVideo {
				extension = ".mp4"
			}
			// The output stays inside the worktree: the path is checked here
			// and the confined file service writes it.
			if err := core.CheckFilePath(input.Path); err != nil {
				return err
			}
			if path.Ext(input.Path) != extension {
				return mediaFault(protocol.ErrorCodeInvalidRequest, "media output has an unsupported extension")
			}
			confined, err := core.OpenFiles(inv.Worktree.Path, core.RestrictedFiles)
			if err != nil {
				return err
			}
			defer confined.Close()
			// An existing file is refused before the provider is called.
			if existing, err := confined.Open(input.Path); err == nil {
				existing.Close()
				return mediaFault(protocol.ErrorCodeConflict, "a file already exists at the media output path")
			}
			if kind == protocol.MediaKindVideo {
				action = protocol.MediaAction{Kind: "video", Data: &protocol.MediaActionVideoPayload{Prompt: input.Prompt, Path: input.Path}}
			} else {
				action = protocol.MediaAction{Kind: "image", Data: &input}
			}
		}
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "use_media", Data: &protocol.CommandUseMediaPayload{Turn: inv.Run.Turn, Session: inv.Session.ID, Worktree: inv.Run.Worktree, Action: action}})
		mu.Lock()
		calls[call.ID] = &request
		mu.Unlock()
		return nil
	}
	if !readOnly {
		tool.ConfigureApproval = func(ctx context.Context, db *sql.Conn, call canonical.ToolRequest, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
			mu.Lock()
			current := calls[call.ID]
			mu.Unlock()
			if current == nil {
				return value, errors.New("native tool was not prepared")
			}
			return store.ConfigureWorkspaceApproval(ctx, db, inv.Run.Turn, value, operation, current.Command, nil)
		}
		tool.Approved = func(call canonical.ToolRequest, _ protocol.ApprovalID, request protocol.RequestID) {
			mu.Lock()
			defer mu.Unlock()
			if current := calls[call.ID]; current != nil {
				current.ID = request
			}
		}
	}
	tool.Execute = func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
		mu.Lock()
		current := calls[call.ID]
		delete(calls, call.ID)
		mu.Unlock()
		if current == nil {
			return workspaceToolFault(errors.New("native tool was not prepared"))
		}
		action := current.Command.Data.(*protocol.CommandUseMediaPayload).Action
		binding, err := node.mediaBinding(ctx, inv, route)
		if err != nil {
			return workspaceToolFault(mediaModelFault(err))
		}
		var result engine.ToolResult
		switch input := action.Data.(type) {
		case *protocol.MediaActionInspectPayload:
			result, err = node.inspectMedia(ctx, inv, binding, *input)
		case *protocol.MediaActionImagePayload:
			result, err = node.generateMedia(ctx, inv, binding, route, kind, input.Prompt, input.Path)
		case *protocol.MediaActionVideoPayload:
			result, err = node.generateMedia(ctx, inv, binding, route, kind, input.Prompt, input.Path)
		}
		if err != nil {
			return workspaceToolFault(mediaModelFault(err))
		}
		return result
	}
	return tool
}

// mediaModelFault maps a driver failure to its protocol fault and keeps a
// local stop distinct from a provider failure.
func mediaModelFault(err error) error {
	var failure *model.Failure
	if errors.As(err, &failure) {
		if failure.Kind == model.Cancelled {
			return mediaFault(protocol.ErrorCodeCancelled, failure.Message)
		}
		return model.Fault(err)
	}
	return err
}

// mediaBinding authorizes the routed model. The conversation model reuses the
// turn's own authorization and renewal; an assigned model resolves its
// provider's current credential, renewing an account sign-in when needed.
func (node *Node) mediaBinding(ctx context.Context, inv *store.AgentInvocation, route mediaRoute) (model.Binding, error) {
	if !route.assigned {
		captured := *inv
		if err := node.authorizeTurnProvider(ctx, &captured); err != nil {
			return model.Binding{}, err
		}
		if captured.AuthorizationFailure != nil {
			return model.Binding{}, captured.AuthorizationFailure
		}
		return model.Capture(*captured.Provider, captured.Turn.Config, captured.Secret, inv.Session.ID)
	}
	return node.assignedMediaBinding(ctx, route.provider, route.model, inv.Session.ID)
}

func (node *Node) assignedMediaBinding(ctx context.Context, provider protocol.ConversationProvider, name string, session protocol.SessionID) (model.Binding, error) {
	if provider.Authentication != protocol.AuthenticationApiKey {
		if provider.Credential == nil {
			return model.Binding{}, mediaFault(protocol.ErrorCodeNotConfigured, "provider sign-in is required")
		}
		secret, err := node.providerAuthorization(ctx, provider, *provider.Credential)
		if err != nil {
			return model.Binding{}, err
		}
		return model.Capture(provider, store.MediaConfig(provider.ID, name), secret, session)
	}
	var binding model.Binding
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		binding, err = store.ResolveMediaBinding(ctx, db, node.ID(), provider, name, session)
		return err
	})
	return binding, err
}

// mediaImage loads the inspected image from the confined worktree or from an
// attachment of this conversation.
func (node *Node) mediaImage(ctx context.Context, inv *store.AgentInvocation, source protocol.MediaSource) (string, []byte, error) {
	switch value := source.Data.(type) {
	case *string:
		confined, err := core.OpenFiles(inv.Worktree.Path, core.RestrictedFiles)
		if err != nil {
			return "", nil, err
		}
		defer confined.Close()
		image, err := files.ReadImage(ctx, confined, *value)
		if err != nil {
			return "", nil, err
		}
		data, err := base64.StdEncoding.DecodeString(image.InlineData[0].Data)
		return image.InlineData[0].MimeType, data, err
	case *protocol.AttachmentID:
		var attachment protocol.Attachment
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			attachment, err = store.ResolveTurnAttachment(ctx, db, inv.Run.Turn, *value)
			return err
		})
		if err != nil {
			return "", nil, err
		}
		if attachment.Spec.Worktree != inv.Run.Worktree {
			return "", nil, mediaFault(protocol.ErrorCodePermissionDenied, "attachment belongs to another worktree")
		}
		if attachment.Spec.Size > protocol.MAX_IMAGE_BYTES {
			return "", nil, mediaFault(protocol.ErrorCodeInvalidRequest, "image exceeds the inspection limit")
		}
		data, err := node.transfers.AttachmentBytes(ctx, attachment)
		if err != nil {
			return "", nil, err
		}
		facts, sent, encoded, err := files.PrepareImage(data)
		if err != nil {
			return "", nil, err
		}
		if sent != nil {
			facts = *sent
		}
		return facts.MimeType, encoded, nil
	}
	return "", nil, mediaFault(protocol.ErrorCodeInvalidRequest, "select one image source")
}

// inspectMedia asks the routed vision model about one image through its
// normal chat request and returns the model's text.
func (node *Node) inspectMedia(ctx context.Context, inv *store.AgentInvocation, binding model.Binding, input protocol.MediaActionInspectPayload) (engine.ToolResult, error) {
	mime, data, err := node.mediaImage(ctx, inv, input.Source)
	if err != nil {
		return engine.ToolResult{}, err
	}
	languageModel, err := node.newModel(ctx, binding)
	if err != nil {
		return engine.ToolResult{}, err
	}
	sum := blake3.Sum256(data)
	digest := hex.EncodeToString(sum[:])
	message := canonical.NewMessage(canonical.User, canonical.Text(0, input.Prompt),
		canonical.Block{Index: 1, Kind: "image", Data: &canonical.AssetContent{Asset: canonical.AssetRef{Digest: digest, Mime: mime, Size: uint64(len(data))}}})
	record := canonical.NewRecord(inv.Run.Turn, "message", message, time.Now())
	call, err := model.BuildCall(binding, model.Request{Records: []canonical.Record{record}, Assets: map[string][]byte{digest: data}})
	if err != nil {
		return engine.ToolResult{}, err
	}
	operation, cancel := context.WithCancel(ctx)
	defer cancel()
	timer := time.AfterFunc(mediaInspectIdle, cancel)
	defer timer.Stop()
	stream, err := languageModel.Stream(operation, call)
	if err != nil {
		return engine.ToolResult{}, model.Fault(err)
	}
	var text strings.Builder
	var usage canonical.Usage
	finished := false
	for part := range stream {
		if operation.Err() != nil {
			break
		}
		timer.Reset(mediaInspectIdle)
		switch part.Type {
		case fantasy.StreamPartTypeTextDelta:
			text.WriteString(part.Delta)
		case fantasy.StreamPartTypeError:
			return engine.ToolResult{}, model.Fault(part.Error)
		case fantasy.StreamPartTypeFinish:
			finished = true
		}
		if reported := model.PartUsage(part); reported.Input != nil || reported.Output != nil || reported.Total != nil {
			usage = reported
		}
		if text.Len() > mediaAnswerBytes {
			return engine.ToolResult{}, mediaFault(protocol.ErrorCodeUnavailable, "vision model response exceeds the size limit")
		}
	}
	if ctx.Err() != nil {
		return engine.ToolResult{}, mediaFault(protocol.ErrorCodeCancelled, "media request was cancelled")
	}
	if operation.Err() != nil {
		return engine.ToolResult{}, mediaFault(protocol.ErrorCodeUnavailable, "vision model request timed out")
	}
	if !finished || strings.TrimSpace(text.String()) == "" {
		return engine.ToolResult{}, mediaFault(protocol.ErrorCodeUnavailable, "vision model returned no text")
	}
	if err := node.recordMediaUsage(ctx, inv, binding, protocol.MediaKindVision, usage); err != nil {
		return engine.ToolResult{}, err
	}
	return engine.ToolResult{Value: map[string]any{"kind": "media", "data": map[string]any{
		"text": text.String(), "provider": binding.Provider.Name, "model": binding.Config.Model,
	}}}, nil
}

// recordMediaUsage counts the call under the provider and model that ran it.
func (node *Node) recordMediaUsage(ctx context.Context, inv *store.AgentInvocation, binding model.Binding, kind protocol.MediaKind, usage canonical.Usage) error {
	var zero uint64
	if usage.Input == nil {
		usage.Input = &zero
	}
	if usage.Output == nil {
		usage.Output = &zero
	}
	return nodeFault(node.agent.RecordMediaUsage(ctx, inv.Session.ID, inv.Run.Turn, binding.Provider, binding.Config.Model, kind, usage))
}

// generateMedia runs the provider call, writes the asset through the confined
// file service without replacing a file, and records the result and usage.
func (node *Node) generateMedia(ctx context.Context, inv *store.AgentInvocation, binding model.Binding, route mediaRoute, kind protocol.MediaKind, prompt, target string) (engine.ToolResult, error) {
	var output model.MediaOutput
	var err error
	limit := uint64(model.MaxMediaImageBytes)
	if kind == protocol.MediaKindVideo {
		limit = model.MaxMediaVideoBytes
		output, err = model.GenerateVideo(ctx, binding, prompt, node.mediaPoll)
		if err != nil && output.Job != "" {
			// The provider accepted the job; stopping or failing locally does
			// not cancel it.
			return engine.ToolResult{}, mediaFault(protocol.ErrorCodeOutcomeUnknown, "video job "+output.Job+" was accepted but its result was not saved: "+mediaModelFault(err).Error())
		}
	} else {
		output, err = model.GenerateImage(ctx, binding, route.channel, prompt)
	}
	if err != nil {
		return engine.ToolResult{}, err
	}
	// The provider call is billed whether or not the file can be saved.
	if err := node.recordMediaUsage(ctx, inv, binding, kind, output.Usage); err != nil {
		return engine.ToolResult{}, err
	}
	confined, err := core.OpenFiles(inv.Worktree.Path, core.RestrictedFiles)
	if err != nil {
		return engine.ToolResult{}, err
	}
	defer confined.Close()
	parents := strings.Split(path.Dir(target), "/")
	for index := range parents {
		if directory := strings.Join(parents[:index+1], "/"); directory != "." {
			if err := confined.CreateDirectory(directory); err != nil {
				return engine.ToolResult{}, err
			}
		}
	}
	file, err := confined.PrepareFileTarget(context.WithoutCancel(ctx), target, nil, limit)
	if err != nil {
		return engine.ToolResult{}, err
	}
	sum := blake3.Sum256(output.Data)
	revision := hex.EncodeToString(sum[:])
	if _, err := file.Write(bytes.NewReader(output.Data), uint64(len(output.Data)), revision); err != nil {
		return engine.ToolResult{}, err
	}
	node.watches.changed(inv.Worktree.Path, nil)
	data := map[string]any{"path": target, "media_type": output.MediaType, "size": len(output.Data), "provider": binding.Provider.Name, "model": binding.Config.Model}
	if output.Job != "" {
		data["job"] = output.Job
	}
	result := engine.ToolResult{Value: map[string]any{"kind": "media", "data": data}}
	if kind == protocol.MediaKindImage {
		// The stored image is shown with the tool result; it is never sent
		// back to the model.
		var digest string
		err := node.profile.Store.Node.Write(context.WithoutCancel(ctx), func(db *sql.Conn) error {
			var err error
			digest, err = store.PutBlob(context.WithoutCancel(ctx), db, output.Data)
			return err
		})
		if err != nil {
			return engine.ToolResult{}, mediaFault(protocol.ErrorCodeOutcomeUnknown, "image was saved at "+target+" but could not be recorded for display")
		}
		result.Attachments = []canonical.Attachment{{Asset: canonical.AssetRef{Digest: digest, Mime: output.MediaType, Size: uint64(len(output.Data))}}}
	}
	return result, nil
}

// synthesizeSpeech reads text aloud with the assigned speech model. There is
// no conversation model to follow, so an absent or unavailable assignment is
// reported instead of substituted.
func (node *Node) synthesizeSpeech(ctx context.Context, input protocol.CommandSynthesizeSpeechPayload) (protocol.Output, error) {
	if strings.TrimSpace(input.Text) == "" || utf8.RuneCountInString(input.Text) > model.MaxSpeechCharacters {
		return protocol.Output{}, mediaFault(protocol.ErrorCodeInvalidRequest, "speech text must contain 1–4096 characters")
	}
	voice := defaultSpeechName
	if input.Voice != nil {
		voice = *input.Voice
		if voice == "" || len(voice) > 64 || strings.ContainsFunc(voice, func(r rune) bool { return r < 0x20 || r == 0x7f }) {
			return protocol.Output{}, mediaFault(protocol.ErrorCodeInvalidRequest, "speech voice is invalid")
		}
	}
	var capture store.MediaCapture
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		capture, err = store.CaptureMedia(ctx, db)
		return err
	})
	if err != nil {
		return protocol.Output{}, err
	}
	assignment, bound := capture[protocol.MediaKindSpeech]
	if !bound {
		return protocol.Output{}, mediaFault(protocol.ErrorCodeNotConfigured, "speech model is not assigned")
	}
	if assignment.Provider == nil {
		return protocol.Output{}, mediaFault(protocol.ErrorCodeNotConfigured, "assigned speech model is unavailable")
	}
	binding, err := node.assignedMediaBinding(ctx, *assignment.Provider, assignment.Model, protocol.NewSessionID())
	if err != nil {
		return protocol.Output{}, mediaModelFault(err)
	}
	output, err := model.SynthesizeSpeech(ctx, binding, input.Text, voice)
	if err != nil {
		return protocol.Output{}, mediaModelFault(err)
	}
	return protocol.Output{Kind: "speech", Data: protocol.SpeechAudio{MediaType: output.MediaType, DataBase64: base64.StdEncoding.EncodeToString(output.Data), Provider: assignment.Provider.ID, Model: assignment.Model}}, nil
}

// dispatchSpeech runs read-aloud synthesis for a controller. Speech never
// changes Node state and is not offered to plugins or the agent.
func (node *Node) dispatchSpeech(ctx context.Context, request protocol.Request) (link.Admission, error) {
	if request.Plugin != nil {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "speech synthesis requires a controller"}
	}
	if err := node.begin(); err != nil {
		return link.Admission{}, err
	}
	completion := make(chan link.Response, 1)
	go func() {
		defer node.work.Done()
		defer close(completion)
		operation, cancel := context.WithCancel(ctx)
		defer cancel()
		defer context.AfterFunc(node.ctx, cancel)()
		output, err := node.synthesizeSpeech(operation, *request.Command.Data.(*protocol.CommandSynthesizeSpeechPayload))
		completion <- response(output, err)
	}()
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
}
