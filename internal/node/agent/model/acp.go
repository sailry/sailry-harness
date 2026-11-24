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
	// Sessions is the Node's directory for the records of which agent
	// session holds each Sailry session (acp_session.go). Empty keeps none.
	Sessions string
	// stall replaces the silence limit in tests.
	stall time.Duration
}

type acpModel struct {
	binding Binding
	capture ACPCapture
}

const acpSystemPrefix, acpSystemSuffix = "<system>\n", "\n</system>\n\n"

// SummarySystemBytes accounts for the single summary message and ACP framing
// when the agent cannot carry system text separately. Zero means no byte cap.
func SummarySystemBytes(languageModel fantasy.LanguageModel, request string) int {
	if model, ok := languageModel.(*acpModel); ok {
		overhead := len(acpSystemPrefix) + len(acpDiscipline) + len("\n\n") + len(acpSystemSuffix) + len(request)
		return max(1, int(model.capture.Adapter.Acp.ReplayBytes)-overhead)
	}
	return 0
}

func NewACP(binding Binding, capture ACPCapture) (fantasy.LanguageModel, error) {
	if !PluginBound(binding.Provider) || capture.Adapter.Acp == nil || capture.Pool == nil ||
		(**binding.Provider.Binding).Adapter.Adapter != capture.Adapter.ID || capture.Adapter.Acp.Program != capture.Program.ID {
		return nil, &Failure{Kind: NotConfigured, Message: "captured model adapter is unavailable"}
	}
	if capture.stall == 0 {
		capture.stall = acpStallTimeout
	}
	return &acpModel{binding: binding, capture: capture}, nil
}

// acpStallTimeout is how long an ACP agent may stay silent. The agent runs its
// own tools inside one prompt turn, so a long command can leave the session
// quiet longer than a model stream's stallTimeout.
const acpStallTimeout = 600 * time.Second

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

// acpAgent is one running program. It serves several Sailry sessions at once,
// each through its own agent session (acpSlot), as far as the package's
// declared concurrency allows.
type acpAgent struct {
	conn    *acp.Connection
	process *program.Process
	images  bool
	// resume, load and close are the session methods the agent offers.
	resume, load, close bool
	// metaSystem: the agent takes the system text in `session/new`.
	metaSystem bool
	// identity binds this process to its captured adapter and launch inputs.
	// Only a digest is retained; credentials never become a pool key or log.
	identity string

	mu sync.Mutex
	// slots holds the agent session of each Sailry session the program
	// serves; the zero ID holds requests without a session.
	slots map[protocol.SessionID]*acpSlot
	// sinks receive the updates of each agent session whose prompt is in
	// flight.
	sinks map[string]func(reasoning bool, text string)
	// commands are the commands each agent session advertised.
	commands map[string]map[string]bool
}

// acpSlotCapacity bounds the agent sessions one program keeps; the least
// recently used free one is closed first.
const acpSlotCapacity = 64

// acpSlot is the agent session that holds one Sailry session's history. One
// request uses a slot at a time: agent.mu guards busy and used, and the
// request that holds the slot owns the rest.
type acpSlot struct {
	owner   protocol.SessionID
	session string
	offers  []acpOffer
	thought acpThought
	// system and seen identify what the session has been told.
	system   string
	seen     []string
	prefixed bool
	selected string
	level    string
	busy     bool
	used     time.Time
}

// take reserves the slot of owner for one request: its free stored slot,
// else a new one. A request without a Sailry session, or one whose session's
// stored slot another request holds, gets a slot without an owner that is not
// stored: its agent session ends with the request. take returns the agent
// sessions of the slots a new one evicted.
func (agent *acpAgent) take(owner protocol.SessionID) (*acpSlot, []string) {
	agent.mu.Lock()
	defer agent.mu.Unlock()
	if owner == (protocol.SessionID{}) {
		return &acpSlot{busy: true}, nil
	}
	if slot := agent.slots[owner]; slot != nil {
		if slot.busy {
			return &acpSlot{busy: true}, nil
		}
		slot.busy = true
		return slot, nil
	}
	slot := &acpSlot{owner: owner, busy: true}
	agent.slots[owner] = slot
	var evicted []string
	for len(agent.slots) > acpSlotCapacity {
		var oldest *acpSlot
		for _, held := range agent.slots {
			if !held.busy && (oldest == nil || held.used.Before(oldest.used)) {
				oldest = held
			}
		}
		if oldest == nil {
			break
		}
		delete(agent.slots, oldest.owner)
		delete(agent.commands, oldest.session)
		if oldest.session != "" {
			evicted = append(evicted, oldest.session)
		}
	}
	return slot, evicted
}

// give returns a slot that take reserved.
func (agent *acpAgent) give(slot *acpSlot) {
	agent.mu.Lock()
	slot.busy, slot.used = false, time.Now()
	agent.mu.Unlock()
}

// holds reports whether the program keeps a free slot for owner.
func (agent *acpAgent) holds(owner protocol.SessionID) bool {
	agent.mu.Lock()
	defer agent.mu.Unlock()
	slot := agent.slots[owner]
	return slot != nil && !slot.busy
}

type acpInitialized struct {
	AgentCapabilities struct {
		PromptCapabilities struct {
			Image bool `json:"image"`
		} `json:"promptCapabilities"`
		LoadSession         bool `json:"loadSession"`
		SessionCapabilities struct {
			Resume *struct{} `json:"resume"`
			Close  *struct{} `json:"close"`
		} `json:"sessionCapabilities"`
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

type acpUsage struct {
	InputTokens       *int64 `json:"inputTokens"`
	OutputTokens      *int64 `json:"outputTokens"`
	CachedReadTokens  *int64 `json:"cachedReadTokens"`
	CachedWriteTokens *int64 `json:"cachedWriteTokens"`
	ThoughtTokens     *int64 `json:"thoughtTokens"`
	TotalTokens       *int64 `json:"totalTokens"`
}

// plus adds the usage of another prompt of the same reply.
func (usage *acpUsage) plus(other *acpUsage) *acpUsage {
	if usage == nil {
		return other
	}
	if other == nil {
		return usage
	}
	sum := func(a, b *int64) *int64 {
		if a == nil {
			return b
		}
		if b == nil {
			return a
		}
		total := *a + *b
		return &total
	}
	return &acpUsage{InputTokens: sum(usage.InputTokens, other.InputTokens), OutputTokens: sum(usage.OutputTokens, other.OutputTokens),
		CachedReadTokens: sum(usage.CachedReadTokens, other.CachedReadTokens), CachedWriteTokens: sum(usage.CachedWriteTokens, other.CachedWriteTokens),
		ThoughtTokens: sum(usage.ThoughtTokens, other.ThoughtTokens), TotalTokens: sum(usage.TotalTokens, other.TotalTokens)}
}

type acpPrompted struct {
	StopReason string    `json:"stopReason"`
	Usage      *acpUsage `json:"usage"`
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
				Commands []struct {
					Name string `json:"name"`
				} `json:"availableCommands"`
			} `json:"update"`
		}
		if json.Unmarshal(params, &update) != nil {
			return nil, acp.NewInvalidParams(nil)
		}
		if update.Update.Kind == "available_commands_update" {
			// The list can arrive before the request that opened its session
			// returns, so it is kept by session.
			agent.mu.Lock()
			delete(agent.commands, update.SessionID)
			for _, command := range update.Update.Commands {
				agent.advertise(update.SessionID, strings.TrimPrefix(command.Name, "/"))
			}
			agent.mu.Unlock()
			return nil, nil
		}
		// Each prompt in flight receives only its own session's updates.
		agent.mu.Lock()
		sink := agent.sinks[update.SessionID]
		agent.mu.Unlock()
		if sink == nil || update.Update.Content.Type != "text" || update.Update.Content.Text == "" {
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

func (model *acpModel) specification(selected string) program.Spec {
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
	return spec
}

func (model *acpModel) identity(spec program.Spec, argument string) string {
	// Host entries have map semantics. Ignore variables the process service
	// never inherits, so environment ordering and unrelated Node settings do
	// not discard a compatible process.
	host := map[string]string{}
	for _, entry := range spec.Host {
		if name, value, ok := strings.Cut(entry, "="); ok && program.Inherited(name) {
			host[name] = value
		}
	}
	spec.Host = nil
	return acpDigest(struct {
		Spec           program.Spec
		Host           map[string]string
		Adapter        protocol.PluginModelAdapterDeclaration
		Provider       protocol.ProviderID
		Binding        **protocol.ConversationPresetsBinding
		Endpoint       string
		Authentication protocol.Authentication
		Credential     *protocol.CredentialRef
		Secret         *protocol.Secret
		Version, Model string
	}{spec, host, model.capture.Adapter, model.binding.Provider.ID, model.binding.Provider.Binding,
		model.binding.Provider.Endpoint, model.binding.Provider.Authentication, model.binding.Provider.Credential,
		model.binding.Secret, model.capture.Version, argument})
}

// start runs the captured specification and completes the protocol handshake.
func (model *acpModel) start(ctx context.Context, spec program.Spec, identity string) (*program.Process, *acpAgent, error) {
	process, err := program.Start(spec)
	if errors.Is(err, program.ErrNotInstalled) {
		return nil, nil, &Failure{Kind: NotConfigured, Message: fmt.Sprintf("%s is not installed or not on the search path", spec.Command)}
	}
	if err != nil {
		return nil, nil, &Failure{Kind: Unavailable, Message: fmt.Sprintf("%s could not be started", spec.Command)}
	}
	agent := &acpAgent{process: process, identity: identity, slots: map[protocol.SessionID]*acpSlot{}, sinks: map[string]func(bool, string){}}
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
	capabilities := initialized.AgentCapabilities
	agent.images = capabilities.PromptCapabilities.Image && model.capture.Adapter.Capabilities.Vision
	agent.resume, agent.load, agent.close = capabilities.SessionCapabilities.Resume != nil, capabilities.LoadSession, capabilities.SessionCapabilities.Close != nil
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

// open starts an agent session for slot and reads the models it offers. An
// agent that requires authentication is asked once to use its declared method.
func (model *acpModel) open(ctx context.Context, agent *acpAgent, slot *acpSlot, system string) error {
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
	slot.session = session.SessionID
	slot.offers, slot.thought = acpOffers(session)
	slot.system, slot.seen, slot.prefixed, slot.selected, slot.level = system, nil, false, "", ""
	return nil
}

// acpOffers reads the models and thought levels a session offers.
func acpOffers(session acpSession) ([]acpOffer, acpThought) {
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
	return offers, thought
}

// choose applies the requested model and effort to the session. A level is
// a model variant where the package declares suffixes, else the session's
// thought-level option.
func (model *acpModel) choose(ctx context.Context, agent *acpAgent, slot *acpSlot, effort string) error {
	if model.capture.Adapter.Acp.ModelArgument {
		return nil
	}
	requested := model.binding.Config.Model
	find := func(id string) *acpOffer {
		for index := range slot.offers {
			if slot.offers[index].ID == id {
				return &slot.offers[index]
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
		if offer = find(requested); offer != nil && slices.Contains(slot.thought.Values, effort) {
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
	if slot.selected != offer.ID {
		var err error
		if offer.Option != "" {
			_, err = acpCall[json.RawMessage](ctx, agent, acp.AgentMethodSessionSetConfigOption, map[string]any{"sessionId": slot.session, "configId": offer.Option, "value": offer.ID})
		} else {
			_, err = acpCall[json.RawMessage](ctx, agent, "session/set_model", map[string]any{"sessionId": slot.session, "modelId": offer.ID})
		}
		if err != nil {
			return model.failure(ctx, agent, err)
		}
		slot.selected, slot.level = offer.ID, ""
	}
	if level != "" && slot.level != level {
		if _, err := acpCall[json.RawMessage](ctx, agent, acp.AgentMethodSessionSetConfigOption, map[string]any{"sessionId": slot.session, "configId": slot.thought.Option, "value": level}); err != nil {
			return model.failure(ctx, agent, err)
		}
		slot.level = level
	}
	return nil
}

// launch is the specification a request starts and the identity that binds
// a running program to it.
func (model *acpModel) launch(discovery bool) (program.Spec, string) {
	argument := ""
	if model.capture.Adapter.Acp.ModelArgument && !discovery {
		argument = model.binding.Config.Model
	}
	spec := model.specification(argument)
	return spec, model.identity(spec, argument)
}

// lease takes a compatible program that keeps a free slot for the session,
// else a compatible one with room for another request, else a new one.
func (model *acpModel) lease(ctx context.Context, discovery bool) (*program.Entry, *acpAgent, error) {
	spec, identity := model.launch(discovery)
	session := model.binding.Session
	rank := func(entry *program.Entry) int {
		agent, ok := entry.Value.(*acpAgent)
		switch {
		case !ok || agent.identity != identity:
			return 0
		case !discovery && session != (protocol.SessionID{}) && agent.holds(session):
			return 2
		}
		return 1
	}
	// A program serves as many requests at once as the package declares,
	// each in its own agent session. When the package runs its most programs
	// and all are full, a request, such as one of many parallel delegations,
	// waits for a running one to finish.
	entry, err := model.capture.Pool.Lease(ctx, model.capture.Owner, int(model.capture.Adapter.Acp.Concurrency), rank, func() (*program.Process, any, error) {
		process, agent, err := model.start(ctx, spec, identity)
		return process, agent, err
	})
	if err != nil {
		return nil, nil, err
	}
	return entry, entry.Value.(*acpAgent), nil
}

// reserve takes the slot of the request's Sailry session in agent and closes
// the agent sessions it evicted where the agent offers that.
func (model *acpModel) reserve(ctx context.Context, agent *acpAgent) *acpSlot {
	slot, evicted := agent.take(model.binding.Session)
	for _, session := range evicted {
		if agent.close {
			_, _ = acpCall[json.RawMessage](ctx, agent, acp.AgentMethodSessionClose, map[string]any{"sessionId": session})
		}
	}
	return slot
}

// discard ends the agent session of a slot that is not stored. A program
// that is retired takes its sessions with it.
func (model *acpModel) discard(ctx context.Context, agent *acpAgent, session string, live bool) {
	if session == "" {
		return
	}
	agent.mu.Lock()
	delete(agent.commands, session)
	agent.mu.Unlock()
	if live && agent.close {
		_, _ = acpCall[json.RawMessage](context.WithoutCancel(ctx), agent, acp.AgentMethodSessionClose, map[string]any{"sessionId": session})
	}
}

type acpEvent struct {
	reasoning bool
	text      string
}

func (model *acpModel) stream(ctx context.Context, request protocol.PluginModelAdapterWireRequest, yield func(fantasy.StreamPart) bool) (result error) {
	settings := model.capture.Adapter.Acp
	system, digests, err := acpHistory(request)
	if err != nil {
		return err
	}
	entry, agent, err := model.lease(ctx, false)
	if err != nil {
		return err
	}
	keep := false
	defer func() { model.capture.Pool.Release(entry, keep) }()
	slot := model.reserve(ctx, agent)
	defer agent.give(slot)
	if slot.owner == (protocol.SessionID{}) {
		defer func() { model.discard(ctx, agent, slot.session, keep) }()
	}
	// A session this slot opened for its Sailry session holds a history that
	// a new session replaces. A request without a session claims nothing.
	stale := slot.session != "" && slot.owner != (protocol.SessionID{})

	// A session that has seen a prefix of this history receives the rest,
	// whether this program holds it or resumes it; any other history is
	// replayed into a new session.
	sent := 0
	switch {
	case slot.session != "" && slot.system == system && acpExtends(slot.seen, digests):
		sent = len(slot.seen)
	case model.resume(ctx, agent, slot, system, digests):
		sent = len(slot.seen)
	default:
		previous := slot.session
		if err := model.open(ctx, agent, slot, system); err != nil {
			return err
		}
		if stale && agent.close {
			// The replaced session's context is no longer used; the agent may
			// release it. A failure leaves only an unused session behind.
			_, _ = acpCall[json.RawMessage](ctx, agent, acp.AgentMethodSessionClose, map[string]any{"sessionId": previous})
		}
	}
	keep = true
	if err := model.choose(ctx, agent, slot, request.Reasoning.Effort.Kind); err != nil {
		return err
	}
	text, images, err := acpPrompt(request.Messages[sent:], sent == 0)
	if err != nil {
		return err
	}
	if !agent.metaSystem && !slot.prefixed {
		text = acpSystemPrefix + system + acpSystemSuffix + text
	}
	// The bound applies to a replay. A session that continues receives only
	// new messages, whatever its history has grown to.
	if sent == 0 && len(text) > int(settings.ReplayBytes) {
		return &Failure{Kind: ContextOverflow, Message: fmt.Sprintf("The conversation is larger than %s accepts in one prompt", model.capture.Program.Command)}
	}
	if len(images) > 0 && !agent.images {
		return &Failure{Kind: Unsupported, Message: fmt.Sprintf("%s does not accept images", model.capture.Program.Command)}
	}
	blocks := []any{map[string]any{"type": "text", "text": text}}
	for _, image := range images {
		blocks = append(blocks, map[string]any{"type": "image", "mimeType": image.MediaType, "data": image.Data})
	}
	// The prompt text omits the JSON-RPC envelope and attachments, so it does
	// not establish the serialized request size.
	capturePrepared(ctx, []byte(text), false)

	// From here the session's content is uncertain until the reply is whole.
	slot.seen, slot.prefixed = nil, true
	model.keep(agent, slot)
	// One SDK goroutine delivers the notifications of every session in the
	// program, so the sink only queues: a slow consumer of this reply must not
	// hold back the others. The SDK answers a prompt after handling the
	// notifications sent before the answer, so the queue holds the whole reply
	// once the answer arrives.
	var queue struct {
		sync.Mutex
		events []acpEvent
	}
	wake := make(chan struct{}, 1)
	drain := func() []acpEvent {
		queue.Lock()
		defer queue.Unlock()
		events := queue.events
		queue.events = nil
		return events
	}
	session := slot.session
	agent.mu.Lock()
	agent.sinks[session] = func(reasoning bool, text string) {
		queue.Lock()
		queue.events = append(queue.events, acpEvent{reasoning, text})
		queue.Unlock()
		select {
		case wake <- struct{}{}:
		default:
		}
	}
	agent.mu.Unlock()
	defer func() {
		agent.mu.Lock()
		delete(agent.sinks, session)
		agent.mu.Unlock()
	}()
	type outcome struct {
		prompted acpPrompted
		err      error
	}
	done := make(chan outcome, 1)
	send := func(blocks []any) {
		go func() {
			prompted, err := acp.SendRequest[acpPrompted](agent.conn, context.WithoutCancel(ctx), acp.AgentMethodSessionPrompt, map[string]any{"sessionId": session, "prompt": blocks})
			done <- outcome{prompted, err}
		}()
	}
	send(blocks)
	// abandon asks the agent to stop and kills it when it does not.
	abandon := func() {
		_ = agent.conn.SendNotification(context.Background(), acp.AgentMethodSessionCancel, map[string]any{"sessionId": session})
		settle := time.NewTimer(acpSettle)
		defer settle.Stop()
		for {
			select {
			case <-wake:
				drain()
			case <-done:
				return
			case <-settle.C:
				keep = false
				return
			}
		}
	}

	// Without tools no reply contains a call: a summary may quote the tags.
	parser, accumulator := &acpParser{plain: len(request.Tools) == 0}, pluginAccumulator{secrets: model.secrets(), completeCalls: true}
	emit := func(deltas ...protocol.PluginModelAdapterWireDelta) (bool, error) {
		parts, err := accumulator.push(deltas)
		if err != nil {
			return false, err
		}
		if accumulator.finished {
			ended, err := accumulator.finish()
			if err != nil {
				return false, err
			}
			parts = append(parts, ended...)
		}
		for _, part := range parts {
			if !yield(part) {
				return false, nil
			}
		}
		return true, nil
	}
	calls, resends, spent := uint64(0), 0, (*acpUsage)(nil)
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
	deliver := func(events []acpEvent) (bool, error) {
		for _, event := range events {
			var open bool
			var err error
			if event.reasoning {
				open, err = emit(protocol.PluginModelAdapterWireDelta{Kind: "reasoning", Data: &protocol.PluginModelAdapterWireDeltaReasoningPayload{Text: &event.text}})
			} else {
				open, err = answer(event.text)
			}
			if err != nil || !open {
				return open, err
			}
		}
		return true, nil
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
		case <-wake:
			stall.Reset(model.capture.stall)
			if open, err := deliver(drain()); err != nil || !open {
				abandon()
				return err
			}
		case finished := <-done:
			if finished.err != nil {
				keep = false
				return model.failure(ctx, agent, finished.err)
			}
			// The reply's last chunks were queued before its answer.
			if open, err := deliver(drain()); err != nil || !open {
				return err
			}
			// A call the agent wrote wrong is asked for again in the same
			// session; the calls read before it stand.
			if malformed := parser.pending(); malformed != nil && finished.prompted.StopReason == "end_turn" {
				if resends == acpResends {
					return errACPCall
				}
				resends, spent = resends+1, spent.plus(finished.prompted.Usage)
				parser.resume(malformed)
				stall.Reset(model.capture.stall)
				send([]any{map[string]any{"type": "text", "text": acpCorrection(malformed)}})
				continue
			}
			// Every prompt of the reply counts, its corrections included.
			finished.prompted.Usage = spent.plus(finished.prompted.Usage)
			return model.finish(agent, slot, digests, parser, finished.prompted, emit)
		}
	}
}

// finish ends the reply: its stop reason, its usage and what the session
// now holds.
func (model *acpModel) finish(agent *acpAgent, slot *acpSlot, digests []string, parser *acpParser, prompted acpPrompted, emit func(...protocol.PluginModelAdapterWireDelta) (bool, error)) error {
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
	if err != nil && (reason != protocol.PluginModelAdapterWireFinishLength || parser.failed != nil) {
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
	if usage := prompted.Usage; usage != nil {
		// The agent counts cached tokens beside its input and thought tokens
		// inside its output; a prompt's input is all three input counts.
		count := func(values ...*int64) *uint64 {
			var total uint64
			for _, value := range values {
				if value != nil && *value > 0 {
					total += uint64(*value)
				}
			}
			if values[0] == nil {
				return nil
			}
			return &total
		}
		deltas = append(deltas, protocol.PluginModelAdapterWireDelta{Kind: "usage", Data: &protocol.PluginModelAdapterWireDeltaUsagePayload{
			InputTokens: count(usage.InputTokens, usage.CachedReadTokens, usage.CachedWriteTokens), OutputTokens: count(usage.OutputTokens),
			CachedInputTokens: count(usage.CachedReadTokens), ReasoningTokens: count(usage.ThoughtTokens),
		}})
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
		slot.seen = append(slices.Clone(digests), acpReply(parser.text.String(), parser.calls))
		model.keep(agent, slot)
	}
	return nil
}
