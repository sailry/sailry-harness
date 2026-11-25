package live

import (
	"bytes"
	"context"
	"encoding/json"
	"slices"
	"strconv"
	"strings"
	"sync"
	"unicode"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginNativeProjection struct {
	body    json.RawMessage
	source  json.RawMessage
	cursor  uint64
	changed chan struct{}
	err     error
}

type pluginNativeComposer struct {
	source   string
	read     bool
	key      pluginComposerKey
	mu       sync.Mutex
	released bool
	view     pluginNativeProjection
	model    pluginNativeProjection
}

var (
	errPluginComposerSourceRequired    = pluginNativeFault(protocol.ErrorCodeUnavailable, "composer requires a captured source")
	errPluginComposerSourceUnavailable = pluginNativeFault(protocol.ErrorCodeUnavailable, "composer source is unavailable")
	errPluginComposerScopeChanged      = pluginNativeFault(protocol.ErrorCodeUnavailable, "composer scope changed")
	errPluginComposerScopeUnavailable  = pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin composer scope is unavailable")
	errPluginComposerReadDenied        = pluginNativeFault(protocol.ErrorCodePermissionDenied, "composer read is unavailable")
	errPluginModelReadDenied           = pluginNativeFault(protocol.ErrorCodePermissionDenied, "model context is unavailable")
)

func (state *pluginNativeState) composerOperations() {
	source := ""
	if state.scope.Session != nil {
		source = state.scope.Session.String()
	} else if state.scope.Surface.Kind == "composer" {
		if strings.HasPrefix(state.presentation, "draft/") {
			source = state.presentation
		}
	}
	info, available := state.packageInfo()
	state.composer = &pluginNativeComposer{
		source: source,
		read:   available && info.Extension != nil && slices.Contains(info.Extension.Actions, protocol.PluginActionReadConversation),
		view:   pluginNativeProjection{changed: make(chan struct{})},
		model:  pluginNativeProjection{changed: make(chan struct{})},
	}
	state.native.Operations["sailry/ui.readComposer"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		if info, available := state.packageInfo(); !available || info.Extension == nil || !slices.Contains(info.Extension.Actions, protocol.PluginActionReadConversation) {
			return nil, errPluginComposerReadDenied
		}
		state.refreshComposer()
		return state.composerValue(false)
	}
	state.native.Operations["sailry/ui.readModelContext"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		state.refreshComposer()
		return state.composerValue(true)
	}
	state.native.Operations["sailry/ui.nextComposerChange"] = state.nextComposer(false)
	state.native.Operations["sailry/ui.nextModelContext"] = state.nextComposer(true)
	state.native.Operations["sailry/ui.setComposerOption"] = state.setComposerOption
	state.native.Operations["sailry/ui.revealComposerEntry"] = state.revealComposerEntry
	state.refreshComposer()
}

func (state *pluginNativeState) composerSource() (*viewmodel.Conversation, *composer.State, protocol.SessionConfig, error) {
	view, comp, session, err := state.resolveComposer()
	if err != nil {
		return nil, nil, protocol.SessionConfig{}, err
	}
	if view == nil {
		view = &viewmodel.Conversation{ID: state.composer.source, Node: state.runtime.controller.Target().String()}
	}
	return view, comp, state.composerSelection(comp, session), nil
}

func (state *pluginNativeState) resolveComposer() (*viewmodel.Conversation, *composer.State, protocol.Session, error) {
	if state.composer.source == "" || state.native.Context.Err() != nil {
		return nil, nil, protocol.Session{}, errPluginComposerSourceRequired
	}
	runtime := state.runtime
	source := state.composer.source
	node := runtime.controller.Target().String()
	view := runtime.pluginComposerView(source)
	session, exists := runtime.session(source)
	// Delegated sessions render in a detail tab without joining the window's
	// conversation list. Their composer and history are already keyed by ID.
	if view == nil {
		if state.scope.Surface.Kind != "composer" || state.scope.Session == nil || !exists || !runtime.childComposerOpen(session) {
			return nil, nil, protocol.Session{}, errPluginComposerSourceUnavailable
		}
	} else if view.Node != node {
		return nil, nil, protocol.Session{}, errPluginComposerSourceUnavailable
	}
	comp := runtime.App.Composer(source)
	if exists {
		if state.scope.Session == nil || session.ID != *state.scope.Session ||
			state.scope.Worktree != nil && session.Worktree != *state.scope.Worktree {
			return nil, nil, protocol.Session{}, errPluginComposerScopeChanged
		}
	} else if state.scope.Session != nil || comp.Host != "" && comp.Host != node {
		return nil, nil, protocol.Session{}, errPluginComposerScopeChanged
	}
	return view, comp, session, nil
}

func (state *pluginNativeState) composerSelection(comp *composer.State, session protocol.Session) protocol.SessionConfig {
	runtime := state.runtime
	config := protocol.DefaultSessionConfig()
	if state.scope.Session != nil {
		config = session.Config
	} else if base, exists := runtime.composerBases[comp]; exists {
		config = base
	}
	config.Mode = []protocol.WorkMode{"code", "plan", "flagship"}[min(max(int(comp.Mode), 0), 2)]
	config.Permission = []protocol.Permission{"ask", "project", "full"}[min(max(int(comp.Permission), 0), 2)]
	if selected, _, err := runtime.composerConfig(comp, composer.Message{
		Model: comp.Model, Effort: comp.EffortName(nil, comp.Chosen(runtime.models())),
		Features: comp.Features, Mode: comp.Mode, Permission: comp.Permission,
	}); err == nil {
		config = selected
	}
	return config
}

func (r *Runtime) childComposerOpen(session protocol.Session) bool {
	if r.teams == nil || !delegated(session) {
		return false
	}
	id := session.ID.String()
	parent, opened := r.teams.childParents[id]
	return opened && parent == (*session.Delegation).Session && r.App.HasPanelTab(panel.Tab{Kind: panel.Subagent, ID: id})
}

func (state *pluginNativeState) refreshComposer() {
	owner := state.composer
	if owner == nil {
		return
	}
	owner.mu.Lock()
	defer owner.mu.Unlock()
	if owner.released {
		return
	}
	runtime := state.runtime
	revision := runtime.controller.Revision()
	info, available := state.packageInfo()
	key := pluginComposerKey{valid: true}
	var comp *composer.State
	var session protocol.Session
	var readView, readModel bool
	if !available || info.Extension == nil {
		key.failure = errPluginComposerScopeUnavailable
	} else {
		var view *viewmodel.Conversation
		view, comp, session, key.failure = state.resolveComposer()
		if key.failure == nil {
			readView = slices.Contains(info.Extension.Actions, protocol.PluginActionReadConversation) ||
				slices.Contains(info.Extension.Actions, protocol.PluginActionControlConversation)
			readModel = slices.Contains(info.Extension.Actions, protocol.PluginActionProvideModels) || len(info.Extension.ModelResources) > 0
			key = state.composerKey(revision, info.Extension, view, comp, session, readView)
		}
	}
	if owner.key == key {
		return
	}
	owner.key = key

	var projection, model map[string]any
	var viewErr, modelErr error
	if key.failure != nil {
		viewErr, modelErr = key.failure, key.failure
	} else {
		config := state.composerSelection(comp, session)
		if readView {
			projection, viewErr = state.projectComposer(key, config, info)
		} else {
			viewErr = errPluginComposerReadDenied
		}
		if readModel {
			model = state.projectModelContext(key.connected, config, info)
		} else {
			modelErr = errPluginModelReadDenied
		}
	}
	publishNativeProjection(&owner.view, projection, viewErr)
	publishNativeProjection(&owner.model, model, modelErr)
}

func publishNativeProjection(projection *pluginNativeProjection, value map[string]any, failure error) {
	body, err := json.Marshal(value)
	if failure == nil {
		failure = err
	}
	equalError := projection.err == nil && failure == nil ||
		projection.err != nil && failure != nil && projection.err.Error() == failure.Error()
	if equalError && bytes.Equal(projection.source, body) {
		return
	}
	if projection.source != nil {
		projection.cursor++
	}
	projection.source, projection.err = body, failure
	if failure == nil {
		value["cursor"] = strconv.FormatUint(projection.cursor, 10)
		projection.body, projection.err = json.Marshal(value)
	} else {
		projection.body = nil
	}
	close(projection.changed)
	projection.changed = make(chan struct{})
}

func (state *pluginNativeState) releaseComposer() {
	owner := state.composer
	owner.mu.Lock()
	defer owner.mu.Unlock()
	if owner.released {
		return
	}
	owner.released = true
	owner.key = pluginComposerKey{}
	state.operations, state.operationsKey = nil, pluginOperationsKey{}
	for _, projection := range []*pluginNativeProjection{&owner.view, &owner.model} {
		close(projection.changed)
		*projection = pluginNativeProjection{err: errPluginComposerScopeUnavailable}
	}
}

func (state *pluginNativeState) composerValue(model bool) (json.RawMessage, error) {
	state.composer.mu.Lock()
	defer state.composer.mu.Unlock()
	projection := &state.composer.view
	if model {
		projection = &state.composer.model
	}
	return append(json.RawMessage{}, projection.body...), projection.err
}

func (state *pluginNativeState) nextComposer(model bool) plugins.HostOperation {
	return func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		if !model {
			if !state.composer.read {
				return nil, errPluginComposerReadDenied
			}
		}
		seen, err := pluginNativeArgument[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		for {
			if err := state.native.Context.Err(); err != nil {
				return nil, err
			}
			state.composer.mu.Lock()
			projection := &state.composer.view
			if model {
				projection = &state.composer.model
			}
			if failure := projection.err; failure != nil {
				state.composer.mu.Unlock()
				return nil, failure
			}
			if projection.body != nil && strconv.FormatUint(projection.cursor, 10) != seen {
				body := append(json.RawMessage{}, projection.body...)
				state.composer.mu.Unlock()
				return body, nil
			}
			changed := projection.changed
			state.composer.mu.Unlock()
			select {
			case <-ctx.Done():
				return nil, ctx.Err()
			case <-state.native.Context.Done():
				return nil, state.native.Context.Err()
			case <-changed:
			}
		}
	}
}

func (state *pluginNativeState) projectComposer(key pluginComposerKey, config protocol.SessionConfig, info protocol.PluginInfo) (map[string]any, error) {
	runtime := state.runtime
	snapshot := runtime.snapshot()
	var statistics json.RawMessage = json.RawMessage("null")
	compacting, hasMessages := false, key.hasMessages
	if history := key.history; history != nil {
		if history.err != nil {
			return nil, history.err
		}
		statistics, compacting = history.statistics, history.compacting
		hasMessages = hasMessages || history.hasMessages
	}
	// The model and the window are the composer's selection. The statistics
	// carry the model and the window of the last request beside them, and a
	// package compares the two to tell a model changed since.
	modelTools := false
	var model *string
	var contextLimit *uint32
	if config.Model != "" {
		model = &config.Model
	}
	for _, provider := range snapshot.Providers {
		if provider.ID != config.Provider {
			continue
		}
		for _, selected := range provider.Models {
			if selected.ID == config.Model {
				modelTools = selected.Tools
				limit := selected.Context
				contextLimit = &limit
			}
		}
	}
	modes := state.composerModes()
	choices := []map[string]any{}
	var selection *protocol.PluginModesSelection
	selected := client.ProjectModes(config, modes).Selected
	for _, mode := range modes {
		if mode.Enabled {
			choices = append(choices, map[string]any{"package": pluginNativeReference(mode.Package), "id": mode.Declaration.ID, "display": mode.Declaration.Display, "permissions": mode.Declaration.Permissions})
		}
		if selected != nil && mode.Key() == *selected {
			selection = &protocol.PluginModesSelection{Package: mode.Package, ID: mode.Declaration.ID}
		}
	}
	options := map[string]json.RawMessage{}
	if info.Extension.ConversationOptions != nil {
		var schema protocol.PluginSettingsSchema
		if err := json.Unmarshal(*info.Extension.ConversationOptions, &schema); err != nil {
			return nil, err
		}
		for key, field := range schema.Properties {
			if saved, exists := config.Options[info.Summary.Name][key]; exists {
				options[key] = saved
			} else if field.Default != nil {
				options[key] = *field.Default
			}
		}
	}
	busy := state.composerBusy(state.composer.source)
	var command any
	if key.registry != nil {
		for _, entry := range key.registry.Commands() {
			if declared := entry.Declaration.Command; declared != nil && declared.Name == key.command &&
				declared.Kind == protocol.PluginUiCommandKindMessage && entry.State.Enabled {
				command = map[string]any{"name": key.command, "arguments": key.arguments, "package": pluginNativeReference(entry.Key.Package)}
				break
			}
		}
	}
	var modeSelection any
	if selection != nil {
		modeSelection = map[string]any{"package": pluginNativeReference(selection.Package), "id": selection.ID}
	}
	return map[string]any{
		"command": command, "mode": config.Mode, "mode_selection": modeSelection, "modes": choices,
		"session": state.scope.Session, "connected": key.connected, "connection": key.connection.projection(),
		"busy": busy, "configuring": busy && !key.saving, "readonly": !key.connected || key.delegated,
		"model_tools": modelTools, "model": model, "context_limit": contextLimit, "statistics": statistics,
		"compacting": compacting, "has_messages": hasMessages, "sidebar": key.sidebar,
		"options": options, "operations": state.composerOperationsAvailable(config),
	}, nil
}

func (state *pluginNativeState) composerBusy(source string) bool {
	runtime := state.runtime
	if runtime.conversationCreating[source] || runtime.conversationCreations[source] != nil || runtime.composerSaving[source] {
		return true
	}
	ownsTurn := func(id protocol.TurnID) bool {
		if state.scope.Session == nil {
			return false
		}
		history, exists := runtime.conversationState(*state.scope.Session)
		if !exists {
			return false
		}
		return slices.ContainsFunc(history.Snapshot.Page.Runs, func(run protocol.ConversationRun) bool { return run.Turn == id }) ||
			slices.ContainsFunc(history.Snapshot.Page.Queue.Items, func(item protocol.ConversationPending) bool { return item.Turn == id })
	}
	matches := func(command protocol.Command) bool {
		switch command.Kind {
		case "submit_turn":
			value, valid := detail[protocol.CommandSubmitTurnPayload](command.Data)
			return valid && state.scope.Session != nil && value.Session == *state.scope.Session
		case "queue_turn":
			value, valid := detail[protocol.CommandQueueTurnPayload](command.Data)
			return valid && state.scope.Session != nil && value.Session == *state.scope.Session
		case "guide_turn":
			value, valid := detail[protocol.CommandGuideTurnPayload](command.Data)
			return valid && ownsTurn(value.Recipient)
		case "edit_queued_turn":
			value, valid := detail[protocol.CommandEditQueuedTurnPayload](command.Data)
			return valid && ownsTurn(value.Turn)
		case "replace_turn":
			value, valid := detail[protocol.CommandReplaceTurnPayload](command.Data)
			return valid && ownsTurn(value.Turn)
		default:
			return false
		}
	}
	for key, pending := range runtime.busy {
		if !pending {
			continue
		}
		_, encoded, captured := strings.Cut(key, "\x00")
		var command protocol.Command
		if captured && json.Unmarshal([]byte(encoded), &command) == nil && matches(command) {
			return true
		}
	}
	for _, request := range runtime.uncertain {
		if matches(request.Command) {
			return true
		}
	}
	return false
}

func (state *pluginNativeState) composerRegistry() *plugins.Registry {
	root := state.runtime.localRuntime()
	if root.pluginViews == nil {
		return nil
	}
	scope := state.scope
	scope.Surface = protocol.PluginDesktopSurface{Kind: "composer"}
	key := pluginScopeIdentity(state.runtime.controller.Target(), scope)
	if scope.Session == nil {
		key.presentation = state.composer.source
	}
	if owner := root.pluginViews.registries[key]; owner != nil {
		return owner.registry
	}
	return nil
}

func (state *pluginNativeState) projectModelContext(connected bool, config protocol.SessionConfig, info protocol.PluginInfo) map[string]any {
	value := map[string]any{"connected": connected, "provider": nil}
	if !connected {
		return value
	}
	for _, provider := range state.runtime.snapshot().Providers {
		if !provider.Enabled || provider.ID != config.Provider {
			continue
		}
		if context, shown := pluginProviderContext(provider, config.Model, info); shown {
			value["provider"] = context
		}
		break
	}
	return value
}

// pluginProviderContext is what a package is told of an enabled channel: its
// identity, model and sign-in, where the channel came from the package's own
// preset or signs in the way one of the package's model resources declares
// (plugins/providers.rs context).
func pluginProviderContext(provider protocol.ConversationProvider, model string, info protocol.PluginInfo) (map[string]any, bool) {
	if !provider.Enabled || info.Extension == nil {
		return nil, false
	}
	ownsPreset := false
	if provider.Preset != nil && provider.Preset.Source.Kind == "package" {
		source, valid := detail[protocol.ConversationPresetsSourcePackagePayload](provider.Preset.Source.Data)
		ownsPreset = valid && source.Name == info.Summary.Name
	}
	declared := (provider.Authentication == protocol.AuthenticationChatGpt || provider.Authentication == protocol.AuthenticationChatGptOAuth || provider.Authentication == protocol.AuthenticationClaudeOAuth) &&
		slices.ContainsFunc(info.Extension.ModelResources, func(resource protocol.PluginModelAuthResource) bool {
			return resource.Authentication == provider.Authentication
		})
	if !ownsPreset && !declared {
		return nil, false
	}
	return map[string]any{
		"id": provider.ID, "revision": strconv.FormatUint(provider.Revision, 10), "model": model,
		"authentication": provider.Authentication, "preset": provider.Preset,
	}, true
}

func pluginNativeReference(reference protocol.PluginReference) map[string]any {
	return map[string]any{"name": reference.Name, "digest": reference.Digest, "settings_revision": strconv.FormatUint(reference.SettingsRevision, 10)}
}

func pluginNativeLeadingCommand(text string) (string, string, bool) {
	source := strings.TrimLeftFunc(text, unicode.IsSpace)
	if !strings.HasPrefix(source, "/") {
		return "", "", false
	}
	end := strings.IndexFunc(source, unicode.IsSpace)
	if end < 0 {
		end = len(source)
	}
	name := source[1:end]
	if name == "" || len(name) > 64 {
		return "", "", false
	}
	for _, character := range name {
		if !(character >= 'a' && character <= 'z' || character >= '0' && character <= '9' || character == '-' || character == '_') {
			return "", "", false
		}
	}
	return name, strings.TrimLeftFunc(source[end:], unicode.IsSpace), true
}
