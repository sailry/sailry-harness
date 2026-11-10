package plugins

import (
	"context"
	"encoding/json"
	"errors"
	"slices"
	"strconv"
	"sync"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type CardAction struct {
	Kind   string                   `json:"kind"`
	Target *string                  `json:"target,omitempty"`
	Mode   *protocol.PluginCardMode `json:"mode,omitempty"`
}

type CardState struct {
	Cursor     string          `json:"cursor"`
	Props      json.RawMessage `json:"props"`
	Configured bool            `json:"configured"`
	Login      string          `json:"login"`
	CanOpen    bool            `json:"can_open"`
	CanCancel  bool            `json:"can_cancel"`
	Action     *string         `json:"action"`
	Locale     string          `json:"locale,omitempty"`
}

type CardBindings struct {
	Context       context.Context
	Summary       protocol.PluginCardSummary
	Declaration   protocol.PluginCardDeclaration
	Locale        string
	Native        *NativeBinding
	Clock         clockwork.Clock
	ValidateProps func(json.RawMessage) bool
	Status        func(context.Context) (protocol.PluginConnectionStatus, error)
	Action        func(string, CardAction)
	Toast         func(string, bool)
}

type CardHost struct {
	binding CardBindings
	ctx     context.Context
	cancel  context.CancelFunc
	mu      sync.Mutex
	state   CardState
	changed chan struct{}
	closed  bool
	qr      cardQrCache
}

func NewCardHost(binding CardBindings) (*CardHost, error) {
	if binding.Context == nil || binding.Native == nil || binding.ValidateProps == nil || !binding.ValidateProps(binding.Summary.Props) {
		return nil, hostInvalid("invalid captured card properties")
	}
	if binding.Locale == "" {
		binding.Locale = "en"
	}
	if binding.Clock == nil {
		binding.Clock = clockwork.NewRealClock()
	}
	ctx, cancel := context.WithCancel(binding.Context)
	binding.Native.ensure(ctx)
	var initial struct {
		Configured bool `json:"configured"`
	}
	_ = json.Unmarshal(binding.Summary.Props, &initial)
	return &CardHost{
		binding: binding, ctx: ctx, cancel: cancel, changed: make(chan struct{}),
		state: CardState{
			Cursor: "0", Props: slices.Clone(binding.Summary.Props), Configured: initial.Configured,
			Login: "idle", Locale: binding.Locale,
		},
	}, nil
}

func (host *CardHost) Close() error {
	host.mu.Lock()
	if host.closed {
		host.mu.Unlock()
		return nil
	}
	host.closed = true
	host.cancel()
	close(host.changed)
	host.qr = cardQrCache{}
	host.mu.Unlock()
	host.binding.Native.close()
	return nil
}

func (host *CardHost) State() CardState {
	host.mu.Lock()
	defer host.mu.Unlock()
	state := host.state
	state.Props = slices.Clone(state.Props)
	if state.Action != nil {
		state.Action = new(*state.Action)
	}
	return state
}

func (host *CardHost) change(update func(*CardState)) {
	host.mu.Lock()
	if host.closed {
		host.mu.Unlock()
		return
	}
	cursor, _ := strconv.ParseUint(host.state.Cursor, 10, 64)
	update(&host.state)
	host.state.Cursor = strconv.FormatUint(cursor+1, 10)
	close(host.changed)
	host.changed = make(chan struct{})
	host.mu.Unlock()
	if host.binding.Native.Invalidate != nil {
		host.binding.Native.Invalidate()
	}
}

func (host *CardHost) Login(state string, canOpen, canCancel bool) {
	previous := host.State()
	if previous.Login == state && previous.CanOpen == canOpen && previous.CanCancel == canCancel {
		return
	}
	host.change(func(current *CardState) {
		current.Login, current.CanOpen, current.CanCancel = state, canOpen, canCancel
		if state == "connected" {
			current.Configured = true
		}
	})
}

func (host *CardHost) Refresh(identity string) {
	host.change(func(state *CardState) { state.Action = new(identity + ":" + state.Cursor) })
}

func (host *CardHost) Modules() []HostModule {
	wrap := func(operation HostOperation) HostOperation {
		return func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			if host.ctx.Err() != nil {
				return nil, hostUnavailable("card is no longer available")
			}
			operationContext, cancel := context.WithCancel(ctx)
			stop := context.AfterFunc(host.ctx, cancel)
			defer func() { stop(); cancel() }()
			return operation(operationContext, arguments)
		}
	}
	return []HostModule{{Name: "sailry/card", Exports: []HostExport{
		hostSync("state", wrap(func(context.Context, []json.RawMessage) (json.RawMessage, error) {
			return hostJSON(host.State())
		})),
		hostSync("theme", wrap(func(context.Context, []json.RawMessage) (json.RawMessage, error) {
			if host.binding.Native.Theme != nil {
				return host.binding.Native.Theme(), nil
			}
			return ThemeSnapshot(nil), nil
		})),
		hostSync("publishProps", wrap(func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			if len(arguments) == 0 || !host.binding.ValidateProps(arguments[0]) {
				return nil, hostInvalid("card properties do not match their schema")
			}
			props := slices.Clone(arguments[0])
			host.change(func(state *CardState) { state.Props = props })
			return json.RawMessage("null"), nil
		})),
		hostSync("toast", wrap(func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			message, err := hostValue[string](arguments, 0)
			if err != nil || message == "" || len(message) > 1024 {
				return nil, hostInvalid("invalid card toast")
			}
			failed, err := hostOptional[bool](arguments, 1, false)
			if err != nil {
				return nil, err
			}
			if host.binding.Toast == nil {
				return nil, hostUnavailable("card toast is unavailable")
			}
			host.binding.Toast(message, failed)
			return json.RawMessage("null"), nil
		})),
		hostAsync("status", wrap(host.readStatus)),
		hostAsync("nextChange", wrap(host.nextChange)),
	}}}
}

func (host *CardHost) readStatus(ctx context.Context, _ []json.RawMessage) (json.RawMessage, error) {
	if host.binding.Status == nil {
		return nil, hostUnavailable("MCP status could not be read")
	}
	status, err := host.binding.Status(ctx)
	if err != nil {
		return nil, err
	}
	configured := status.Authorization == protocol.PluginConnectionAuthorizationConfigured
	host.change(func(state *CardState) { state.Configured = configured })
	type plain protocol.PluginConnectionStatus
	return hostJSON(struct {
		plain
		Configured bool `json:"configured"`
	}{plain(status), configured})
}

func (host *CardHost) nextChange(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	seen, err := hostValue[string](arguments, 0)
	if err != nil {
		return nil, err
	}
	for {
		host.mu.Lock()
		cursor, changed := host.state.Cursor, host.changed
		host.mu.Unlock()
		if cursor != seen {
			return hostJSON(host.State())
		}
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-host.ctx.Done():
			return nil, hostUnavailable("card is closed")
		case <-changed:
		}
	}
}

func (host *CardHost) Target(name string, mode protocol.PluginCardMode) (protocol.PluginCardTarget, bool) {
	for _, target := range host.binding.Declaration.Capabilities.Browser {
		if target.Name == name && slices.Contains(target.Modes, mode) {
			return target, true
		}
	}
	return protocol.PluginCardTarget{}, false
}

func (host *CardHost) Permits(action CardAction) bool {
	authorization := host.binding.Declaration.McpServer != nil && slices.ContainsFunc(host.binding.Declaration.Capabilities.Browser, func(target protocol.PluginCardTarget) bool {
		return target.Source.Kind == "mcp_authorization"
	})
	switch action.Kind {
	case "connect":
		if !authorization || (action.Target == nil) != (action.Mode == nil) {
			return false
		}
		if action.Target == nil {
			return true
		}
		target, valid := host.Target(*action.Target, *action.Mode)
		return valid && target.Source.Kind == "mcp_authorization"
	case "browser":
		if action.Target == nil || action.Mode == nil {
			return false
		}
		_, valid := host.Target(*action.Target, *action.Mode)
		return valid
	case "cancel":
		return authorization && action.Target == nil && action.Mode == nil
	case "refresh":
		return host.binding.Declaration.Capabilities.Refresh && action.Target == nil && action.Mode == nil
	case "settings":
		return host.binding.Declaration.Capabilities.Settings && action.Target == nil && action.Mode == nil
	}
	return false
}

func decodeCardAction(raw json.RawMessage) (CardAction, error) {
	var action CardAction
	if err := protocol.DecodeObject(raw, &action, []string{"kind"}, true); err != nil {
		return action, err
	}
	if action.Kind == "" {
		return action, errors.New("card action kind is required")
	}
	return action, nil
}
