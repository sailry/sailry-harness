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
	source string
	read   bool
	mu     sync.Mutex
	view   pluginNativeProjection
	model  pluginNativeProjection
}

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
			return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "composer read is unavailable")
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
	if state.composer.source == "" || state.native.Context.Err() != nil {
		return nil, nil, protocol.SessionConfig{}, pluginNativeFault(protocol.ErrorCodeUnavailable, "composer requires a captured source")
	}
	view := state.runtime.pluginComposerView(state.composer.source)
	if view == nil || view.Node != state.runtime.controller.Target().String() {
		return nil, nil, protocol.SessionConfig{}, pluginNativeFault(protocol.ErrorCodeUnavailable, "composer source is unavailable")
	}
	comp := state.runtime.App.Composer(view.ID)
	config := protocol.DefaultSessionConfig()
	if session, exists := state.runtime.session(view.ID); exists {
		if state.scope.Session == nil || session.ID != *state.scope.Session ||
			state.scope.Worktree != nil && session.Worktree != *state.scope.Worktree {
			return nil, nil, config, pluginNativeFault(protocol.ErrorCodeUnavailable, "composer scope changed")
		}
		config = session.Config
	} else if state.scope.Session != nil || comp.Host != "" && comp.Host != state.runtime.controller.Target().String() {
		return nil, nil, config, pluginNativeFault(protocol.ErrorCodeUnavailable, "composer scope changed")
	} else if base, exists := state.runtime.composerBases[comp]; exists {
		config = base
	}
	config.Mode = []protocol.WorkMode{"code", "plan", "flagship"}[min(max(int(comp.Mode), 0), 2)]
	config.Permission = []protocol.Permission{"ask", "project", "full"}[min(max(int(comp.Permission), 0), 2)]
	if selected, _, err := state.runtime.composerConfig(comp, composer.Message{
		Model: comp.Model, Effort: comp.EffortName(nil, comp.Chosen(state.runtime.models())),
		Features: comp.Features, Mode: comp.Mode, Permission: comp.Permission,
	}); err == nil {
		config = selected
	}
	return view, comp, config, nil
}

func (state *pluginNativeState) refreshComposer() {
	owner := state.composer
	if owner == nil {
		return
	}
	info, available := state.packageInfo()
	view, comp, config, err := state.composerSource()
	if !available || info.Extension == nil {
		err = pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin composer scope is unavailable")
	}
	var projection, model map[string]any
	var viewErr, modelErr error
	if err != nil {
		viewErr, modelErr = err, err
	} else {
		if slices.Contains(info.Extension.Actions, protocol.PluginActionReadConversation) || slices.Contains(info.Extension.Actions, protocol.PluginActionControlConversation) {
			projection, viewErr = state.projectComposer(view, comp, config, info)
		} else {
			viewErr = pluginNativeFault(protocol.ErrorCodePermissionDenied, "composer read is unavailable")
		}
		if slices.Contains(info.Extension.Actions, protocol.PluginActionProvideModels) || len(info.Extension.ModelResources) > 0 {
			model = state.projectModelContext(config, info)
		} else {
			modelErr = pluginNativeFault(protocol.ErrorCodePermissionDenied, "model context is unavailable")
		}
	}
	owner.mu.Lock()
	publishNativeProjection(&owner.view, projection, viewErr)
	publishNativeProjection(&owner.model, model, modelErr)
	owner.mu.Unlock()
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
	}
	close(projection.changed)
	projection.changed = make(chan struct{})
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
				return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "composer read is unavailable")
			}
		}
		seen, err := pluginNativeArgument[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		for {
			state.composer.mu.Lock()
			projection := &state.composer.view
			if model {
				projection = &state.composer.model
			}
			body, cursor, changed, failure := append(json.RawMessage{}, projection.body...), projection.cursor, projection.changed, projection.err
			state.composer.mu.Unlock()
			if failure != nil {
				return nil, failure
			}
			if body != nil && strconv.FormatUint(cursor, 10) != seen {
				return body, nil
			}
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

func (state *pluginNativeState) projectComposer(view *viewmodel.Conversation, comp *composer.State, config protocol.SessionConfig, info protocol.PluginInfo) (map[string]any, error) {
	runtime := state.runtime
	snapshot := runtime.snapshot()
	var statistics json.RawMessage = json.RawMessage("null")
	compacting, hasMessages := false, len(view.Turns) > 0
	var contextLimit *uint32
	if state.scope.Session != nil {
		if history, exists := runtime.controller.Conversation(*state.scope.Session); exists {
			var err error
			statistics, err = plugins.ComposerStatistics(history.Snapshot.Statistics)
			if err != nil {
				return nil, err
			}
			for _, run := range history.Snapshot.Page.Runs {
				hasMessages = true
				compacting = compacting || run.Kind == protocol.ConversationRunKindCompaction &&
					(run.Status == protocol.ConversationStatusQueued || run.Status == protocol.ConversationStatusRunning || run.Status == protocol.ConversationStatusStopping)
			}
			if history.Snapshot.Statistics != nil {
				contextLimit = history.Snapshot.Statistics.ContextLimit
			}
		}
	}
	modelTools := false
	var model *string
	for _, provider := range snapshot.Providers {
		if provider.ID != config.Provider {
			continue
		}
		for _, selected := range provider.Models {
			if selected.ID == config.Model {
				modelTools = selected.Tools
				selectedID := selected.ID
				model = &selectedID
				if contextLimit == nil {
					limit := selected.Context
					contextLimit = &limit
				}
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
	connection := map[string]any{"route": nil, "rtt_ms": nil}
	if runtime == runtime.localRuntime() && runtime.controller.Connected() {
		connection["route"] = "local"
	} else if network := runtime.localRuntime().network; network != nil {
		for _, peer := range network.snapshot() {
			if peer.address.ID == runtime.controller.Target() {
				connection["route"], connection["rtt_ms"] = peer.connection.Route, peer.connection.RTTMS
			}
		}
	}
	readonly := !runtime.controller.Connected()
	if session, exists := runtime.session(view.ID); exists {
		readonly = readonly || delegated(session)
	}
	busy := state.composerBusy(view.ID)
	var command any
	if name, arguments, found := pluginNativeLeadingCommand(comp.Draft); found {
		if registry := state.composerRegistry(); registry != nil {
			for _, entry := range registry.Commands() {
				if declared := entry.Declaration.Command; declared != nil && declared.Name == name &&
					declared.Kind == protocol.PluginUiCommandKindMessage && entry.State.Enabled {
					command = map[string]any{"name": name, "arguments": arguments, "package": pluginNativeReference(entry.Key.Package)}
					break
				}
			}
		}
	}
	var modeSelection any
	if selection != nil {
		modeSelection = map[string]any{"package": pluginNativeReference(selection.Package), "id": selection.ID}
	}
	return map[string]any{
		"command": command, "mode": config.Mode, "mode_selection": modeSelection, "modes": choices,
		"session": state.scope.Session, "connected": runtime.controller.Connected(), "connection": connection,
		"busy": busy, "configuring": busy && !runtime.composerSaving[view.ID], "readonly": readonly,
		"model_tools": modelTools, "model": model, "context_limit": contextLimit, "statistics": statistics,
		"compacting": compacting, "has_messages": hasMessages, "sidebar": runtime.App.NavShown(),
		"options": options, "operations": state.composerOperationsAvailable(config),
	}, nil
}

func (state *pluginNativeState) composerBusy(source string) bool {
	runtime := state.runtime
	if runtime.conversationCreating[source] || runtime.composerSaving[source] {
		return true
	}
	ownsTurn := func(id protocol.TurnID) bool {
		if state.scope.Session == nil {
			return false
		}
		history, exists := runtime.controller.Conversation(*state.scope.Session)
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
		scope, encoded, captured := strings.Cut(key, "\x00")
		if scope == "create/"+source {
			return true
		}
		var command protocol.Command
		if captured && json.Unmarshal([]byte(encoded), &command) == nil && matches(command) {
			return true
		}
	}
	for key, request := range runtime.uncertain {
		scope, _, _ := strings.Cut(key, "\x00")
		if scope == "create/"+source || matches(request.Command) {
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

func (state *pluginNativeState) projectModelContext(config protocol.SessionConfig, info protocol.PluginInfo) map[string]any {
	value := map[string]any{"connected": state.runtime.controller.Connected(), "provider": nil}
	for _, provider := range state.runtime.snapshot().Providers {
		if !provider.Enabled || provider.ID != config.Provider {
			continue
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
		if value["connected"] == true && (ownsPreset || declared) {
			value["provider"] = map[string]any{
				"id": provider.ID, "revision": strconv.FormatUint(provider.Revision, 10), "model": config.Model,
				"authentication": provider.Authentication, "preset": provider.Preset,
			}
		}
		break
	}
	return value
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
