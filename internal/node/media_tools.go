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
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/files"
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

// mediaTools are the Node's own media tools. A purpose without a route has no
// tool for the turn. They are discovered through tool_search unless an
// assistant declaration names them.
func (node *Node) mediaTools(inv *store.AgentInvocation) ([]engine.Tool, error) {
	specs, err := operations.MediaSpecs()
	if err != nil {
		return nil, err
	}
	var tools []engine.Tool
	for _, spec := range specs {
		kind, _ := mediaOperationKind(spec.Operation)
		route, offered := routeMedia(inv, kind)
		if !offered {
			continue
		}
		tool, err := node.mediaTool(inv, spec, kind, route)
		if err != nil {
			return nil, err
		}
		tools = append(tools, tool)
	}
	return tools, nil
}

func mediaFault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}

// mediaArguments are what the model may send. Inspection names one image
// source; generation names at most a plain output file name, and a video may
// name the job an earlier call left unsaved.
type mediaArguments struct {
	Prompt     string  `json:"prompt"`
	Path       *string `json:"path"`
	Attachment *string `json:"attachment"`
	FileName   *string `json:"file_name"`
	Job        *string `json:"job"`
}

// mediaCall is one prepared call: the request its approval names and, for a
// video, the provider job it fetches instead of submitting the prompt.
type mediaCall struct {
	request protocol.Request
	job     string
}

// mediaAction checks the arguments and returns the action the approval
// names. Generated files are saved under assets/generated.
func mediaAction(kind protocol.MediaKind, arguments json.RawMessage) (protocol.MediaAction, error) {
	var input mediaArguments
	decoder := json.NewDecoder(bytes.NewReader(arguments))
	decoder.DisallowUnknownFields()
	if decoder.Decode(&input) != nil {
		return protocol.MediaAction{}, mediaFault(protocol.ErrorCodeInvalidRequest, "invalid media arguments")
	}
	// A job is a plain identifier: it becomes one segment of the provider URL.
	if input.Job != nil && (kind != protocol.MediaKindVideo || *input.Job == "" || len(*input.Job) > 128 || strings.Trim(*input.Job, ".") == "" || strings.ContainsFunc(*input.Job, func(r rune) bool {
		return !(r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' || r >= '0' && r <= '9' || r == '-' || r == '_' || r == '.')
	})) {
		return protocol.MediaAction{}, mediaFault(protocol.ErrorCodeInvalidRequest, "job must be the identifier an earlier video result named")
	}
	if strings.TrimSpace(input.Prompt) == "" || len(input.Prompt) > mediaPromptBytes || !utf8.ValidString(input.Prompt) {
		return protocol.MediaAction{}, mediaFault(protocol.ErrorCodeInvalidRequest, "media prompt must be nonempty and bounded")
	}
	if kind == protocol.MediaKindVision {
		if (input.Path == nil) == (input.Attachment == nil) || input.FileName != nil {
			return protocol.MediaAction{}, mediaFault(protocol.ErrorCodeInvalidRequest, "select one image source")
		}
		source := protocol.MediaSource{Kind: "path", Data: input.Path}
		if input.Attachment != nil {
			id, err := protocol.ParseAttachmentID(*input.Attachment)
			if err != nil {
				return protocol.MediaAction{}, mediaFault(protocol.ErrorCodeInvalidRequest, "invalid media arguments")
			}
			source = protocol.MediaSource{Kind: "attachment", Data: &id}
		} else if err := core.CheckFilePath(*input.Path); err != nil {
			return protocol.MediaAction{}, err
		}
		return protocol.MediaAction{Kind: "inspect", Data: &protocol.MediaActionInspectPayload{Prompt: input.Prompt, Source: source}}, nil
	}
	if input.Path != nil || input.Attachment != nil {
		return protocol.MediaAction{}, mediaFault(protocol.ErrorCodeInvalidRequest, "generation does not accept input files")
	}
	extension := ".png"
	if kind == protocol.MediaKindVideo {
		extension = ".mp4"
	}
	name := protocol.NewRequestID().String() + extension
	if input.FileName != nil {
		name = *input.FileName
	}
	if !strings.HasSuffix(name, extension) {
		return protocol.MediaAction{}, mediaFault(protocol.ErrorCodeInvalidRequest, "media output has an unsupported extension")
	}
	if strings.ContainsFunc(name, func(r rune) bool { return r == '/' || r == '\\' || r == ':' || r < 0x20 || r >= 0x7f && r <= 0x9f }) {
		return protocol.MediaAction{}, mediaFault(protocol.ErrorCodeInvalidRequest, "media output requires a plain file name")
	}
	target := "assets/generated/" + name
	// The output stays inside the worktree: the path is checked here and the
	// confined file service writes it.
	if err := core.CheckFilePath(target); err != nil {
		return protocol.MediaAction{}, err
	}
	if kind == protocol.MediaKindVideo {
		return protocol.MediaAction{Kind: "video", Data: &protocol.MediaActionVideoPayload{Prompt: input.Prompt, Path: target}}, nil
	}
	return protocol.MediaAction{Kind: "image", Data: &protocol.MediaActionImagePayload{Prompt: input.Prompt, Path: target}}, nil
}

func (node *Node) mediaTool(inv *store.AgentInvocation, spec operations.MediaSpec, kind protocol.MediaKind, route mediaRoute) (engine.Tool, error) {
	var schema map[string]any
	if err := json.Unmarshal(spec.Parameters, &schema); err != nil {
		return engine.Tool{}, err
	}
	operation := spec.Operation
	var mu sync.Mutex
	calls := map[string]*mediaCall{}
	// The binding has no owning package: the tool is the Node's own, and a
	// session rule remembers it by name and operation.
	tool := engine.Tool{Definition: fantasy.FunctionTool{Name: spec.Name, Description: spec.Description, InputSchema: schema}, SchemaJSON: spec.Parameters,
		ReadOnly: spec.ReadOnly, Concurrent: spec.Concurrent, Deferred: true, Approval: !spec.ReadOnly,
		Binding: &store.ApprovalBinding{Tool: spec.Name, Operation: &operation}}
	tool.Prepare = func(_ context.Context, call canonical.ToolRequest) error {
		action, err := mediaAction(kind, call.Arguments)
		if err != nil {
			return err
		}
		target := ""
		switch input := action.Data.(type) {
		case *protocol.MediaActionImagePayload:
			target = input.Path
		case *protocol.MediaActionVideoPayload:
			target = input.Path
		}
		if target != "" {
			confined, err := core.OpenFiles(inv.Worktree.Path, core.RestrictedFiles)
			if err != nil {
				return err
			}
			defer confined.Close()
			// An existing file is refused before the provider is called.
			if existing, err := confined.Open(target); err == nil {
				existing.Close()
				return mediaFault(protocol.ErrorCodeConflict, "a file already exists at the media output path")
			}
		}
		prepared := &mediaCall{request: protocol.NewRequest(node.ID(), protocol.Command{Kind: "use_media", Data: &protocol.CommandUseMediaPayload{Turn: inv.Run.Turn, Session: inv.Session.ID, Worktree: inv.Run.Worktree, Action: action}})}
		// The action was decoded from the same arguments.
		var input mediaArguments
		_ = json.Unmarshal(call.Arguments, &input)
		if input.Job != nil {
			prepared.job = *input.Job
		}
		mu.Lock()
		calls[call.ID] = prepared
		mu.Unlock()
		return nil
	}
	if !spec.ReadOnly {
		tool.ConfigureApproval = func(ctx context.Context, db *sql.Conn, call canonical.ToolRequest, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
			mu.Lock()
			current := calls[call.ID]
			mu.Unlock()
			if current == nil {
				return value, errors.New("native tool was not prepared")
			}
			return store.ConfigureWorkspaceApproval(ctx, db, inv.Run.Turn, value, operation, current.request.Command, nil)
		}
		tool.Approved = func(call canonical.ToolRequest, _ protocol.ApprovalID, request protocol.RequestID) {
			mu.Lock()
			defer mu.Unlock()
			if current := calls[call.ID]; current != nil {
				current.request.ID = request
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
		action := current.request.Command.Data.(*protocol.CommandUseMediaPayload).Action
		binding, err := node.mediaBinding(ctx, inv, route)
		if err != nil {
			return workspaceToolFault(mediaModelFault(err))
		}
		var result engine.ToolResult
		switch input := action.Data.(type) {
		case *protocol.MediaActionInspectPayload:
			result, err = node.inspectMedia(ctx, inv, binding, *input)
		case *protocol.MediaActionImagePayload:
			result, err = node.generateMedia(ctx, inv, binding, route, kind, input.Prompt, input.Path, "")
		case *protocol.MediaActionVideoPayload:
			result, err = node.generateMedia(ctx, inv, binding, route, kind, input.Prompt, input.Path, current.job)
		}
		if err != nil {
			return workspaceToolFault(mediaModelFault(err))
		}
		return result
	}
	return tool, nil
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
	return engine.ToolResult{Value: map[string]any{"text": text.String(), "provider": binding.Provider.Name, "model": binding.Config.Model}}, nil
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

// publishGenerated writes a generated file below the worktree through the
// confined file service, without replacing a file, and tells its watchers.
func (node *Node) publishGenerated(ctx context.Context, root, target string, data []byte, limit uint64) error {
	confined, err := core.OpenFiles(root, core.RestrictedFiles)
	if err != nil {
		return err
	}
	defer confined.Close()
	parents := strings.Split(path.Dir(target), "/")
	for index := range parents {
		if directory := strings.Join(parents[:index+1], "/"); directory != "." {
			if err := confined.CreateDirectory(directory); err != nil {
				return err
			}
		}
	}
	file, err := confined.PrepareFileTarget(context.WithoutCancel(ctx), target, nil, limit)
	if err != nil {
		return err
	}
	defer file.Close()
	sum := blake3.Sum256(data)
	if _, err := file.Write(bytes.NewReader(data), uint64(len(data)), hex.EncodeToString(sum[:])); err != nil {
		return err
	}
	node.watches.changed(root, nil)
	return nil
}

// videoJobFault reports a video job the provider accepted and that left no
// saved file. A job the provider ended, or whose answer is unusable, is a
// definite error. Any other failure leaves the job running at the provider:
// the answer names it so a later call fetches it instead of paying again.
func videoJobFault(output model.MediaOutput, err error) error {
	fault := nodeFault(mediaModelFault(err)).(*protocol.Fault)
	var failure *model.Failure
	if output.Ended || errors.As(err, &failure) && (failure.Kind == model.InvalidResponse || failure.Kind == model.InvalidRequest) {
		return mediaFault(fault.Code, "video job "+output.Job+" produced no video: "+fault.Message)
	}
	return mediaFault(protocol.ErrorCodeOutcomeUnknown, "video job "+output.Job+" was accepted but its result was not saved: "+fault.Message+videoJobResume(output.Job))
}

// videoJobResume tells the model how to fetch a job the provider still holds.
func videoJobResume(job string) string {
	return `; call generate_video again with "job": "` + job + `" to fetch it, and do not submit the prompt again without it`
}

// generateMedia takes the output target, runs the provider call, writes the
// asset through the confined file service without replacing a file, and
// records the result and usage. A job fetches a video an earlier call
// submitted instead of submitting the prompt.
func (node *Node) generateMedia(ctx context.Context, inv *store.AgentInvocation, binding model.Binding, route mediaRoute, kind protocol.MediaKind, prompt, target, job string) (engine.ToolResult, error) {
	limit := uint64(model.MaxMediaImageBytes)
	if kind == protocol.MediaKindVideo {
		limit = model.MaxMediaVideoBytes
	}
	// The target is taken before the provider is asked, so a call that cannot
	// save its file is refused before it is paid for.
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
		var fault *protocol.Fault
		if errors.As(err, &fault) && fault.Code == protocol.ErrorCodeRevisionConflict {
			// An earlier call of the same step saved this file after both were prepared.
			return engine.ToolResult{}, mediaFault(protocol.ErrorCodeConflict, "a file already exists at the media output path")
		}
		return engine.ToolResult{}, err
	}
	defer file.Close()
	var output model.MediaOutput
	switch {
	case kind != protocol.MediaKindVideo:
		output, err = model.GenerateImage(ctx, binding, route.channel, prompt)
	case job != "":
		output, err = model.ResumeVideo(ctx, binding, job, node.mediaPoll)
	default:
		output, err = model.GenerateVideo(ctx, binding, prompt, node.mediaPoll)
	}
	if err != nil && output.Job != "" {
		return engine.ToolResult{}, videoJobFault(output, err)
	}
	if err != nil {
		return engine.ToolResult{}, err
	}
	sum := blake3.Sum256(output.Data)
	_, saved := file.Write(bytes.NewReader(output.Data), uint64(len(output.Data)), hex.EncodeToString(sum[:]))
	if saved != nil && output.Job != "" {
		// The provider still holds the video; its usage is counted by the
		// call that saves it.
		fault := nodeFault(saved).(*protocol.Fault)
		return engine.ToolResult{}, mediaFault(fault.Code, "video job "+output.Job+" finished but its file was not saved: "+fault.Message+videoJobResume(output.Job))
	}
	// The provider call is billed whether or not the file was saved.
	recorded := node.recordMediaUsage(ctx, inv, binding, kind, output.Usage)
	if saved != nil {
		return engine.ToolResult{}, saved
	}
	node.watches.changed(inv.Worktree.Path, nil)
	if recorded != nil {
		return engine.ToolResult{}, mediaFault(protocol.ErrorCodeOutcomeUnknown, "media was saved at "+target+" but its usage could not be recorded")
	}
	data := map[string]any{"path": target, "media_type": output.MediaType, "size": len(output.Data), "provider": binding.Provider.Name, "model": binding.Config.Model}
	if output.Job != "" {
		data["job"] = output.Job
	}
	result := engine.ToolResult{Value: data}
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
