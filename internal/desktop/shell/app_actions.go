package shell

import (
	"bytes"
	"encoding/json"
	"fmt"
	"maps"
	"regexp"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type AppActionID string

// AppActionScope is supplied by the host, never by action input.
type AppActionScope struct {
	Node     protocol.NodeID
	Worktree protocol.WorktreeID
	Session  protocol.SessionID
}

type AppActionContext struct {
	AppActionScope
	current func() bool
}

// Current must be checked on the UI thread before applying a deferred result.
func (c AppActionContext) Current() bool { return c.current != nil && c.current() }

// AppAction handles one validated JSON object. Run must validate its own fields
// before acting. PublicNavigation is only for navigation without a data mutation.
type AppAction struct {
	ID               AppActionID
	PublicNavigation bool
	PluginPermission protocol.PluginAction
	Run              func(*ui.Context, *App, AppActionContext, json.RawMessage) error
}

type appActionRegistry map[AppActionID]*AppAction

type appActionWindow struct {
	app      *App
	handlers appActionRegistry
}

var appActionID = regexp.MustCompile(`^[a-z][a-z0-9]*(-[a-z0-9]+)*(\.[a-z][a-z0-9]*(-[a-z0-9]+)*)+$`)

func validAppActionID(id AppActionID) bool {
	return len(id) <= 64 && appActionID.MatchString(string(id))
}

func (registry *appActionRegistry) register(action AppAction) (*AppAction, error) {
	if !validAppActionID(action.ID) || action.Run == nil || !action.PublicNavigation && action.PluginPermission != "" {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid app action declaration"}
	}
	if (*registry)[action.ID] != nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: fmt.Sprintf("app action %q is already registered", action.ID)}
	}
	if *registry == nil {
		*registry = make(appActionRegistry)
	}
	(*registry)[action.ID] = &action
	return &action, nil
}

// RegisterAppAction declares one action for every live window in this assembly.
func (a *Assembly) RegisterAppAction(action AppAction) error {
	_, err := a.appActions.register(action)
	return err
}

// RegisterAppAction binds a live service to this window. Release only removes
// this registration; a duplicate ID never replaces a handler.
func (a *App) RegisterAppAction(action AppAction) (release func(), err error) {
	window := a.appActions
	if window == nil {
		return nil, unavailableAppAction()
	}
	registered, err := window.handlers.register(action)
	if err != nil {
		return nil, err
	}
	return func() {
		if registered != nil && window.handlers[action.ID] == registered {
			delete(window.handlers, action.ID)
		}
		registered = nil
	}, nil
}

// AppActionSource is a host-owned caller bound to one live window and source.
// Bind, invoke and release it on that window's UI thread.
type AppActionSource struct {
	window     *appActionWindow
	scope      AppActionScope
	current    func() bool
	navigation bool
	allows     func(protocol.PluginAction) bool
}

// BindAppActions binds trusted UI to its captured source. current must check the
// source's lifetime and identity; for window-wide actions scope may be empty.
func (a *App) BindAppActions(scope AppActionScope, current func() bool) *AppActionSource {
	return &AppActionSource{window: a.appActions, scope: scope, current: current}
}

// BindAppNavigation binds restricted UI to the same registry. The host validates
// the mounted source and its permissions; only public navigation is callable.
func (a *App) BindAppNavigation(scope AppActionScope, current func() bool, allows func(protocol.PluginAction) bool) *AppActionSource {
	return &AppActionSource{window: a.appActions, scope: scope, current: current, navigation: true, allows: allows}
}

// Close invalidates this source, without unregistering actions or other callers.
func (s *AppActionSource) Close() { *s = AppActionSource{} }

func (s *AppActionSource) active() bool {
	return s.window != nil && s.window.app != nil && !s.window.app.Preview && !s.window.app.DemoOn() &&
		s.current != nil && (!s.navigation || s.scope.Node != (protocol.NodeID{})) && s.current()
}

func (s *AppActionSource) permitted(action *AppAction) bool {
	return !s.navigation || action.PublicNavigation && (action.PluginPermission == "" || s.allows != nil && s.allows(action.PluginPermission))
}

// Invoke selects exactly one action. Absence, refusal and handler failure never
// retry or fall through to a link, shortcut or another action.
func (s *AppActionSource) Invoke(c *ui.Context, id AppActionID, input json.RawMessage) error {
	if !validAppActionID(id) {
		return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid app action ID"}
	}
	if !s.active() {
		return unavailableAppAction()
	}
	window := s.window
	action := window.handlers[id]
	if action == nil {
		return unavailableAppAction()
	}
	if !s.permitted(action) {
		return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "app action is outside the source's navigation permissions"}
	}
	if len(input) > 64*1024 {
		return invalidAppActionInput()
	}
	input = bytes.TrimSpace(input)
	if len(input) == 0 || input[0] != '{' || !json.Valid(input) {
		return invalidAppActionInput()
	}
	source := AppActionContext{AppActionScope: s.scope, current: func() bool {
		return s.active() && window.handlers[id] == action && s.permitted(action)
	}}
	return action.Run(c, window.app, source, input)
}

func invalidAppActionInput() error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "app action input must be a JSON object of at most 64 KiB"}
}

func unavailableAppAction() error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "app action is unavailable"}
}

func (a *App) installAppActions(assembly Assembly) {
	if !a.Preview {
		a.appActions = &appActionWindow{app: a, handlers: maps.Clone(assembly.appActions)}
	}
}

func (a *App) closeAppActions() {
	if window := a.appActions; window != nil {
		window.app, window.handlers = nil, nil
		a.appActions = nil
	}
}
