package model

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"time"

	"charm.land/fantasy"
	acp "github.com/coder/acp-go-sdk"
	"github.com/sailry/sailry-harness/internal/node/program"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The Agent Client Protocol route uses a package's declared program as a
// plain model: Sailry keeps its loop, tools and history, the agent's own
// tools are refused, and tool calls travel as text (acp_text.go).

// acpStart bounds a program's start and each session request.
const acpStart = 60 * time.Second

// acpSettle is how long a cancelled prompt may take to end before its
// program is killed.
const acpSettle = 5 * time.Second

// ACPCapture is what the Node captured for one request to an agent program.
type ACPCapture struct {
	Adapter protocol.PluginModelAdapterDeclaration
	Program protocol.PluginProgramDeclaration
	// Owner is the package; Dir is the program's directory in its data.
	Owner, Dir, Version string
	Host                []string
	Pool                *program.Pool
	// stall replaces the silence limit in tests.
	stall time.Duration
}

// ACPBound reports whether the adapter uses an agent program.
func ACPBound(declaration protocol.PluginModelAdapterDeclaration) bool { return declaration.Acp != nil }

type acpModel struct {
	binding Binding
	capture ACPCapture
}

func NewACP(binding Binding, capture ACPCapture) (fantasy.LanguageModel, error) {
	if !PluginBound(binding.Provider) || capture.Adapter.Acp == nil || capture.Pool == nil ||
		(**binding.Provider.Binding).Adapter.Adapter != capture.Adapter.ID || capture.Adapter.Acp.Program != capture.Program.ID {
		return nil, &Failure{Kind: NotConfigured, Message: "captured model adapter is unavailable"}
	}
	if capture.stall == 0 {
		capture.stall = stallTimeout
	}
	return &acpModel{binding: binding, capture: capture}, nil
}

func (model *acpModel) Provider() string { return "sailry-plugin" }
func (model *acpModel) Model() string    { return model.binding.Config.Model }

func (*acpModel) GenerateObject(context.Context, fantasy.ObjectCall) (*fantasy.ObjectResponse, error) {
	return nil, &Failure{Kind: Unsupported, Message: "structured generation is not configured"}
}

func (*acpModel) StreamObject(context.Context, fantasy.ObjectCall) (fantasy.ObjectStreamResponse, error) {
	return nil, &Failure{Kind: Unsupported, Message: "structured generation is not configured"}
}

func (model *acpModel) Generate(ctx context.Context, call fantasy.Call) (*fantasy.Response, error) {
	return (&pluginModel{binding: model.binding, capture: PluginCapture{Declaration: model.capture.Adapter}}).generate(ctx, model.Stream, call)
}

func (model *acpModel) Stream(ctx context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	request, err := (&pluginModel{binding: model.binding, capture: PluginCapture{Declaration: model.capture.Adapter}}).request(call)
	if err != nil {
		return nil, err
	}
	return func(yield func(fantasy.StreamPart) bool) {
		if err := model.stream(ctx, request, yield); err != nil {
			failure := Classify(err, model.secrets()...)
			if ctx.Err() != nil {
				failure = &Failure{Kind: Cancelled, Message: "model request was cancelled"}
			}
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: failure})
		}
	}, nil
}

func (model *acpModel) secrets() []protocol.Secret {
	if model.binding.Secret == nil {
		return nil
	}
	return []protocol.Secret{*model.binding.Secret}
}

// acpOffer is one model the agent offers and how a session selects it:
// through the named session option, or by `session/set_model`.
type acpOffer struct{ ID, Name, Option string }

type acpThought struct {
	Option string
	Values []string
}

// acpAgent is one running program and the one session it serves.
type acpAgent struct {
	conn    *acp.Connection
	process *program.Process
	images  bool
	// metaSystem: the agent takes the system text in `session/new`.
	metaSystem bool
	// argument is the model the program was started with.
	argument string
	// owner is the Sailry session whose history the agent's session holds.
	owner   protocol.SessionID
	claimed bool
	session string
	offers  []acpOffer
	thought acpThought
	// system and seen identify what the session has been told.
	system   string
	seen     []string
	prefixed bool
	selected string
	level    string

	mu   sync.Mutex
	sink func(reasoning bool, text string)
}

type acpInitialized struct {
	AgentCapabilities struct {
		PromptCapabilities struct {
			Image bool `json:"image"`
		} `json:"promptCapabilities"`
		Meta map[string]any `json:"_meta"`
	} `json:"agentCapabilities"`
}

type acpOption struct {
	ID       string          `json:"id"`
	Category string          `json:"category"`
	Type     string          `json:"type"`
	Options  json.RawMessage `json:"options"`
}

type acpSession struct {
	SessionID string `json:"sessionId"`
	Models    struct {
		AvailableModels []struct {
			ModelID string `json:"modelId"`
			Name    string `json:"name"`
		} `json:"availableModels"`
	} `json:"models"`
	ConfigOptions []acpOption `json:"configOptions"`
}

type acpPrompted struct {
	StopReason string `json:"stopReason"`
	Usage      *struct {
		InputTokens       *int64 `json:"inputTokens"`
		OutputTokens      *int64 `json:"outputTokens"`
		CachedReadTokens  *int64 `json:"cachedReadTokens"`
		CachedWriteTokens *int64 `json:"cachedWriteTokens"`
		ThoughtTokens     *int64 `json:"thoughtTokens"`
		TotalTokens       *int64 `json:"totalTokens"`
	} `json:"usage"`
}

// acpChoices flattens a select option's values, grouped or not.
func acpChoices(raw json.RawMessage) (values, names []string) {
	var entries []struct {
		Value   string          `json:"value"`
		Name    string          `json:"name"`
		Options json.RawMessage `json:"options"`
	}
	if json.Unmarshal(raw, &entries) != nil {
		return nil, nil
	}
	for _, entry := range entries {
		if len(entry.Options) > 0 {
			nested, labels := acpChoices(entry.Options)
			values, names = append(values, nested...), append(names, labels...)
		} else if entry.Value != "" {
			values, names = append(values, entry.Value), append(names, entry.Name)
		}
	}
	return values, names
}

// handle answers the agent. Sailry is no workspace for it: every permission
// is refused and no file or terminal method exists.
func (agent *acpAgent) handle(_ context.Context, method string, params json.RawMessage) (any, *acp.RequestError) {
	switch method {
	case acp.ClientMethodSessionUpdate:
		var update struct {
			SessionID string `json:"sessionId"`
			Update    struct {
				Kind    string `json:"sessionUpdate"`
				Content struct {
					Type string `json:"type"`
					Text string `json:"text"`
				} `json:"content"`
			} `json:"update"`
		}
		if json.Unmarshal(params, &update) != nil {
			return nil, acp.NewInvalidParams(nil)
		}
		agent.mu.Lock()
		sink, session := agent.sink, agent.session
		agent.mu.Unlock()
		if sink == nil || update.SessionID != session || update.Update.Content.Type != "text" || update.Update.Content.Text == "" {
			return nil, nil
		}
		switch update.Update.Kind {
		case "agent_message_chunk":
			sink(false, update.Update.Content.Text)
		case "agent_thought_chunk":
			sink(true, update.Update.Content.Text)
		}
		return nil, nil
	case acp.ClientMethodSessionRequestPermission:
		var request struct {
			Options []struct {
				OptionID string `json:"optionId"`
				Kind     string `json:"kind"`
			} `json:"options"`
		}
		_ = json.Unmarshal(params, &request)
		for _, kind := range []string{"reject_once", "reject_always"} {
			for _, option := range request.Options {
				if option.Kind == kind {
					return map[string]any{"outcome": map[string]any{"outcome": "selected", "optionId": option.OptionID}}, nil
				}
			}
		}
		return map[string]any{"outcome": map[string]any{"outcome": "cancelled"}}, nil
	}
	return nil, acp.NewMethodNotFound(method)
}

func acpCall[T any](ctx context.Context, agent *acpAgent, method string, params any) (T, error) {
	bounded, cancel := context.WithTimeout(ctx, acpStart)
	defer cancel()
	return acp.SendRequest[T](agent.conn, bounded, method, params)
}

func (model *acpModel) fill(value, selected string) string {
	value = strings.ReplaceAll(value, "{dir}", model.capture.Dir)
	value = strings.ReplaceAll(value, "{model}", selected)
	if strings.Contains(value, "{key}") {
		key := ""
		if model.binding.Secret != nil {
			key = model.binding.Secret.Expose()
		}
		value = strings.ReplaceAll(value, "{key}", key)
	}
	return value
}

// start runs the program and completes the protocol handshake.
func (model *acpModel) start(ctx context.Context, selected string) (*program.Process, *acpAgent, error) {
	declared := model.capture.Program
	spec := program.Spec{Command: declared.Command, Dir: model.capture.Dir, Host: model.capture.Host, Env: map[string]string{}, Log: model.capture.Owner + "/" + declared.ID}
	for _, argument := range declared.Args {
		// An argument that names the model exists only once one is chosen.
		if selected == "" && strings.Contains(argument, "{model}") {
			continue
		}
		spec.Args = append(spec.Args, model.fill(argument, selected))
	}
	for name, value := range declared.Env {
		if strings.Contains(value, "{key}") && model.binding.Secret == nil {
			continue
		}
		spec.Env[name] = model.fill(value, selected)
	}
	for _, file := range declared.Files {
		spec.Files = append(spec.Files, program.File{Path: filepath.FromSlash(file.Path), Content: file.Content})
	}
	process, err := program.Start(spec)
	if errors.Is(err, program.ErrNotInstalled) {
		return nil, nil, &Failure{Kind: NotConfigured, Message: fmt.Sprintf("%s is not installed or not on the search path", declared.Command)}
	}
	if err != nil {
		return nil, nil, &Failure{Kind: Unavailable, Message: fmt.Sprintf("%s could not be started", declared.Command)}
	}
	agent := &acpAgent{process: process, argument: selected}
	agent.conn = acp.NewConnection(agent.handle, process.Stdin, process.Stdout)
	initialized, err := acpCall[acpInitialized](ctx, agent, acp.AgentMethodInitialize, map[string]any{
		"protocolVersion": 1,
		"clientInfo":      map[string]any{"name": "sailry", "title": "Sailry", "version": model.capture.Version},
		"clientCapabilities": map[string]any{
			"fs": map[string]any{"readTextFile": false, "writeTextFile": false}, "terminal": false,
		},
	})
	if err != nil {
		return process, nil, model.failure(ctx, agent, err)
	}
	agent.images = initialized.AgentCapabilities.PromptCapabilities.Image && model.capture.Adapter.Capabilities.Vision
	if path := model.capture.Adapter.Acp.SystemMeta; path != nil {
		var value any = initialized.AgentCapabilities.Meta
		for _, name := range strings.Split(*path, ".") {
			object, _ := value.(map[string]any)
			value = object[name]
		}
		agent.metaSystem = value == true
	}
	return process, agent, nil
}

// failure turns a protocol error into the model failure the loop reads.
func (model *acpModel) failure(ctx context.Context, agent *acpAgent, err error) error {
	if ctx.Err() != nil {
		return ctx.Err()
	}
	command := model.capture.Program.Command
	select {
	case <-agent.conn.Done():
		message := command + " ended unexpectedly"
		if line := agent.process.Diagnostic(); line != "" {
			message += ": " + line
		}
		return &Failure{Kind: Network, Message: message}
	default:
	}
	var failed *acp.RequestError
	if !errors.As(err, &failed) {
		return &Failure{Kind: Unavailable, Message: command + " did not answer"}
	}
	detail := failed.Message
	if data, _ := json.Marshal(failed.Data); len(data) > 0 && string(data) != "null" {
		detail += " " + string(data)
	}
	lower := strings.ToLower(detail)
	switch {
	case failed.Code == -32000:
		return &Failure{Kind: Authentication, Message: model.capture.Adapter.Acp.SignIn}
	case errors.Is(err, context.DeadlineExceeded) || strings.Contains(lower, "deadline exceeded"):
		return &Failure{Kind: Unavailable, Message: command + " did not answer"}
	case strings.Contains(lower, "429") || strings.Contains(lower, "exhausted") || strings.Contains(lower, "rate limit"):
		return &Failure{Kind: RateLimited, Message: command + ": " + failed.Message}
	case failed.Code == -32602:
		return &Failure{Kind: InvalidRequest, Message: command + ": " + failed.Message}
	}
	return &Failure{Kind: Other, Message: command + ": " + failed.Message}
}

// open starts a session of the agent and reads the models it offers. An
// agent that requires authentication is asked once to use its declared method.
func (model *acpModel) open(ctx context.Context, agent *acpAgent, system string) error {
	params := map[string]any{"cwd": model.capture.Dir, "mcpServers": []any{}}
	if agent.metaSystem && system != "" {
		var meta any = system
		names := strings.Split(*model.capture.Adapter.Acp.SystemMeta, ".")
		for index := len(names) - 1; index >= 0; index-- {
			meta = map[string]any{names[index]: meta}
		}
		params["_meta"] = meta
	}
	session, err := acpCall[acpSession](ctx, agent, acp.AgentMethodSessionNew, params)
	var failed *acp.RequestError
	if method := model.capture.Adapter.Acp.AuthMethod; err != nil && method != nil && errors.As(err, &failed) && failed.Code == -32000 {
		if _, err = acpCall[json.RawMessage](ctx, agent, acp.AgentMethodAuthenticate, map[string]any{"methodId": *method}); err == nil {
			session, err = acpCall[acpSession](ctx, agent, acp.AgentMethodSessionNew, params)
		}
	}
	if err != nil {
		return model.failure(ctx, agent, err)
	}
	if session.SessionID == "" {
		return &Failure{Kind: InvalidResponse, Message: model.capture.Program.Command + " opened no session"}
	}
	offers, thought := []acpOffer{}, acpThought{}
	for _, option := range session.ConfigOptions {
		if option.Type != "select" {
			continue
		}
		values, names := acpChoices(option.Options)
		switch {
		case option.Category == "model" || option.ID == "model":
			for index, value := range values {
				offers = append(offers, acpOffer{ID: value, Name: names[index], Option: option.ID})
			}
		case option.Category == "thought_level" || option.ID == "thought_level":
			thought = acpThought{Option: option.ID, Values: values}
		}
	}
	for _, offered := range session.Models.AvailableModels {
		if offered.ModelID != "" && !slices.ContainsFunc(offers, func(offer acpOffer) bool { return offer.ID == offered.ModelID }) {
			offers = append(offers, acpOffer{ID: offered.ModelID, Name: offered.Name})
		}
	}
	agent.mu.Lock()
	agent.session = session.SessionID
	agent.mu.Unlock()
	agent.offers, agent.thought = offers, thought
	agent.system, agent.seen, agent.prefixed, agent.selected, agent.level = system, nil, false, "", ""
	return nil
}

// choose applies the requested model and effort to the session. A level is
// a model variant where the package declares suffixes, else the session's
// thought-level option.
func (model *acpModel) choose(ctx context.Context, agent *acpAgent, effort string) error {
	if model.capture.Adapter.Acp.ModelArgument {
		return nil
	}
	requested := model.binding.Config.Model
	find := func(id string) *acpOffer {
		for index := range agent.offers {
			if agent.offers[index].ID == id {
				return &agent.offers[index]
			}
		}
		return nil
	}
	suffixes := model.capture.Adapter.Acp.Efforts
	offer, level := (*acpOffer)(nil), ""
	if suffix, declared := suffixes[effort]; declared {
		offer = find(requested + suffix)
	}
	if offer == nil {
		if offer = find(requested); offer != nil && slices.Contains(agent.thought.Values, effort) {
			level = effort
		}
	}
	for _, fallback := range []string{"medium", "low", "high", "minimal", "xhigh", "max"} {
		if suffix, declared := suffixes[fallback]; offer == nil && declared {
			offer = find(requested + suffix)
		}
	}
	if offer == nil {
		return &Failure{Kind: NotConfigured, Message: fmt.Sprintf("%s does not offer the model %s", model.capture.Program.Command, requested)}
	}
	if agent.selected != offer.ID {
		var err error
		if offer.Option != "" {
			_, err = acpCall[json.RawMessage](ctx, agent, acp.AgentMethodSessionSetConfigOption, map[string]any{"sessionId": agent.session, "configId": offer.Option, "value": offer.ID})
		} else {
			_, err = acpCall[json.RawMessage](ctx, agent, "session/set_model", map[string]any{"sessionId": agent.session, "modelId": offer.ID})
		}
		if err != nil {
			return model.failure(ctx, agent, err)
		}
		agent.selected, agent.level = offer.ID, ""
	}
	if level != "" && agent.level != level {
		if _, err := acpCall[json.RawMessage](ctx, agent, acp.AgentMethodSessionSetConfigOption, map[string]any{"sessionId": agent.session, "configId": agent.thought.Option, "value": level}); err != nil {
			return model.failure(ctx, agent, err)
		}
		agent.level = level
	}
	return nil
}

// lease takes the program that serves the session, else an unclaimed one,
// else a new one.
func (model *acpModel) lease(ctx context.Context, discovery bool) (*program.Entry, *acpAgent, error) {
	argument := ""
	if model.capture.Adapter.Acp.ModelArgument && !discovery {
		argument = model.binding.Config.Model
	}
	// The session's own program is preferred over one no session has used.
	rank := func(entry *program.Entry) int {
		agent, ok := entry.Value.(*acpAgent)
		switch {
		case !ok || agent.argument != argument:
			return 0
		case !discovery && agent.claimed && agent.owner == model.binding.Session:
			return 2
		case discovery || !agent.claimed:
			return 1
		}
		return 0
	}
	entry, err := model.capture.Pool.Lease(model.capture.Owner, rank, func() (*program.Process, any, error) {
		process, agent, err := model.start(ctx, argument)
		return process, agent, err
	})
	if errors.Is(err, program.ErrBusy) {
		return nil, nil, &Failure{Kind: Unavailable, Message: fmt.Sprintf("every %s process is busy", model.capture.Program.Command)}
	}
	if err != nil {
		return nil, nil, err
	}
	return entry, entry.Value.(*acpAgent), nil
}

type acpEvent struct {
	reasoning bool
	text      string
}

func (model *acpModel) stream(ctx context.Context, request protocol.PluginModelAdapterWireRequest, yield func(fantasy.StreamPart) bool) (result error) {
	settings := model.capture.Adapter.Acp
	system, err := acpSystem(request)
	if err != nil {
		return err
	}
	digests := make([]string, len(request.Messages))
	for index, message := range request.Messages {
		if reply, assistant := acpStoredReply(message); assistant {
			digests[index] = reply
		} else {
			digests[index] = acpDigest(message)
		}
	}
	entry, agent, err := model.lease(ctx, false)
	if err != nil {
		return err
	}
	keep := false
	defer func() { model.capture.Pool.Release(entry, keep) }()
	agent.owner, agent.claimed = model.binding.Session, true

	// A session that has seen a prefix of this history receives the rest;
	// any other history is replayed into a new session.
	sent := 0
	if agent.session != "" && agent.system == system && len(agent.seen) > 0 && len(agent.seen) < len(digests) && slices.Equal(agent.seen, digests[:len(agent.seen)]) {
		sent = len(agent.seen)
	} else if err := model.open(ctx, agent, system); err != nil {
		return err
	}
	keep = true
	if err := model.choose(ctx, agent, request.Reasoning.Effort.Kind); err != nil {
		return err
	}
	text, images, err := acpPrompt(request.Messages[sent:], sent == 0)
	if err != nil {
		return err
	}
	if !agent.metaSystem && !agent.prefixed {
		text = "<system>\n" + system + "\n</system>\n\n" + text
	}
	if len(text) > int(settings.ReplayBytes) {
		return &Failure{Kind: ContextOverflow, Message: fmt.Sprintf("The conversation is larger than %s accepts in one prompt", model.capture.Program.Command)}
	}
	if len(images) > 0 && !agent.images {
		return &Failure{Kind: Unsupported, Message: fmt.Sprintf("%s does not accept images", model.capture.Program.Command)}
	}
	blocks := []any{map[string]any{"type": "text", "text": text}}
	for _, image := range images {
		blocks = append(blocks, map[string]any{"type": "image", "mimeType": image.MediaType, "data": image.Data})
	}
	capturePrepared(ctx, []byte(text))

	// From here the session's content is uncertain until the reply is whole.
	agent.seen, agent.prefixed = nil, true
	events, finished := make(chan acpEvent), make(chan struct{})
	defer close(finished)
	agent.mu.Lock()
	agent.sink = func(reasoning bool, text string) {
		select {
		case events <- acpEvent{reasoning, text}:
		case <-finished:
		}
	}
	agent.mu.Unlock()
	defer func() {
		agent.mu.Lock()
		agent.sink = nil
		agent.mu.Unlock()
	}()
	type outcome struct {
		prompted acpPrompted
		err      error
	}
	done := make(chan outcome, 1)
	go func() {
		prompted, err := acp.SendRequest[acpPrompted](agent.conn, context.WithoutCancel(ctx), acp.AgentMethodSessionPrompt, map[string]any{"sessionId": agent.session, "prompt": blocks})
		done <- outcome{prompted, err}
	}()
	// abandon asks the agent to stop and kills it when it does not.
	abandon := func() {
		_ = agent.conn.SendNotification(context.Background(), acp.AgentMethodSessionCancel, map[string]any{"sessionId": agent.session})
		settle := time.NewTimer(acpSettle)
		defer settle.Stop()
		for {
			select {
			case <-events:
			case <-done:
				return
			case <-settle.C:
				keep = false
				return
			}
		}
	}

	parser, accumulator := &acpParser{}, pluginAccumulator{secrets: model.secrets()}
	emit := func(deltas ...protocol.PluginModelAdapterWireDelta) (bool, error) {
		parts, err := accumulator.push(deltas)
		if err != nil {
			return false, err
		}
		for _, part := range parts {
			if !yield(part) {
				return false, nil
			}
		}
		return true, nil
	}
	calls := uint64(0)
	answer := func(text string) (bool, error) {
		out, found, err := parser.feed(text)
		if err != nil {
			return false, err
		}
		var deltas []protocol.PluginModelAdapterWireDelta
		if out != "" {
			deltas = append(deltas, protocol.PluginModelAdapterWireDelta{Kind: "text", Data: &protocol.PluginModelAdapterWireDeltaTextPayload{Text: out}})
		}
		for _, call := range found {
			deltas = append(deltas, protocol.PluginModelAdapterWireDelta{Kind: "tool_call", Data: &protocol.PluginModelAdapterWireDeltaToolCallPayload{Index: calls, ID: &call.ID, Name: &call.Name, ArgumentsDelta: &call.Arguments}})
			calls++
		}
		return emit(deltas...)
	}
	stall := time.NewTimer(model.capture.stall)
	defer stall.Stop()
	for {
		select {
		case <-ctx.Done():
			abandon()
			return ctx.Err()
		case <-stall.C:
			keep = false
			return &Failure{Kind: Network, Message: fmt.Sprintf("Model stream sent no data for %d seconds", int(model.capture.stall.Seconds()))}
		case event := <-events:
			stall.Reset(model.capture.stall)
			var open bool
			if event.reasoning {
				open, err = emit(protocol.PluginModelAdapterWireDelta{Kind: "reasoning", Data: &protocol.PluginModelAdapterWireDeltaReasoningPayload{Text: &event.text}})
			} else {
				open, err = answer(event.text)
			}
			if err != nil || !open {
				abandon()
				return err
			}
		case finished := <-done:
			if finished.err != nil {
				keep = false
				return model.failure(ctx, agent, finished.err)
			}
			return model.finish(agent, digests, parser, finished.prompted, emit, &keep)
		}
	}
}

// finish ends the reply: its stop reason, its usage and what the session
// now holds.
func (model *acpModel) finish(agent *acpAgent, digests []string, parser *acpParser, prompted acpPrompted, emit func(...protocol.PluginModelAdapterWireDelta) (bool, error), keep *bool) error {
	command := model.capture.Program.Command
	reason := protocol.PluginModelAdapterWireFinishStop
	switch prompted.StopReason {
	case "end_turn":
	case "max_tokens":
		reason = protocol.PluginModelAdapterWireFinishLength
	case "refusal":
		reason = protocol.PluginModelAdapterWireFinishContentFilter
	case "cancelled":
		return &Failure{Kind: Cancelled, Message: command + " cancelled the request"}
	default:
		reason = protocol.PluginModelAdapterWireFinishOther
	}
	rest, err := parser.finish()
	if err != nil && reason != protocol.PluginModelAdapterWireFinishLength {
		return err
	}
	for _, marker := range model.capture.Adapter.Acp.FailureMarkers {
		if strings.Contains(parser.text.String(), strings.TrimSpace(marker)) {
			message := command + " reported a failure"
			if line := agent.process.Diagnostic(); line != "" {
				message += ": " + line
			}
			return &Failure{Kind: Unavailable, Message: message}
		}
	}
	var deltas []protocol.PluginModelAdapterWireDelta
	if rest != "" {
		deltas = append(deltas, protocol.PluginModelAdapterWireDelta{Kind: "text", Data: &protocol.PluginModelAdapterWireDeltaTextPayload{Text: rest}})
	}
	if len(parser.calls) > 0 && reason == protocol.PluginModelAdapterWireFinishStop {
		reason = protocol.PluginModelAdapterWireFinishToolCalls
	}
	deltas = append(deltas, protocol.PluginModelAdapterWireDelta{Kind: "finish", Data: &protocol.PluginModelAdapterWireDeltaFinishPayload{Reason: reason}})
	if open, err := emit(deltas...); err != nil || !open {
		return err
	}
	// A reply that is whole extends what the session holds; one that ended
	// early leaves it unknown, and the next step replays.
	if reason == protocol.PluginModelAdapterWireFinishStop || reason == protocol.PluginModelAdapterWireFinishToolCalls {
		agent.seen = append(slices.Clone(digests), acpReply(parser.text.String(), parser.calls))
	}
	return nil
}
