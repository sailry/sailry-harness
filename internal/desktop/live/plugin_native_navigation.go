package live

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginNativeToast struct {
	ID      string `json:"id"`
	Message string `json:"message"`
	Kind    string `json:"kind"`
	Action  *struct {
		ID    string `json:"id"`
		Label string `json:"label"`
	} `json:"action"`
}

func (state *pluginNativeState) navigationOperations() {
	operations := state.native.Operations
	operations["sailry/ui.pageInsets"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		return pluginNativeJSON(map[string]float32{"rail_width": shell.RailWidth})
	}
	operations["sailry/ui.workspaceMode"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		return pluginNativeJSON("main")
	}
	operations["sailry/ui.fileResource"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		return json.RawMessage("null"), nil
	}
	operations["sailry/ui.openSession"] = func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		id, err := pluginNativeArgument[protocol.SessionID](arguments, 0)
		if err != nil {
			return nil, err
		}
		return state.effect(func(*ui.Context) { state.openSession(id) })
	}
	operations["sailry/ui.openActivityTarget"] = func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		kind, err := pluginNativeArgument[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		info, available := state.packageInfo()
		if !available || info.Extension == nil || !slices.Contains(info.Extension.Actions, protocol.PluginActionReadActivity) ||
			state.scope.Surface.Kind != "workspace" || state.scope.Worktree != nil || state.scope.Session != nil ||
			state.scope.Turn != nil || state.scope.Invocation != nil {
			return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "activity navigation requires an unscoped workspace view")
		}
		switch kind {
		case "session":
			id, err := pluginNativeArgument[protocol.SessionID](arguments, 1)
			if err != nil {
				return nil, err
			}
			return state.effect(func(*ui.Context) { state.openSession(id) })
		case "terminal":
			id, err := pluginNativeArgument[protocol.TerminalID](arguments, 1)
			if err != nil {
				return nil, err
			}
			return state.effect(func(*ui.Context) {
				if _, available := state.packageInfo(); !available {
					return
				}
				for _, info := range state.runtime.snapshot().Terminals {
					if info.ID == id && info.Status.Kind != "closed" {
						root := state.runtime.localRuntime()
						if existing := root.packages.terminals[id.String()]; existing != nil && existing.owner != state.runtime {
							return
						}
						root.openLiveTerminal(state.runtime, info)
						root.App.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: id.String()})
						root.App.PanelOpen = true
						return
					}
				}
			})
		default:
			return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid activity target")
		}
	}
	operations["sailry/ui.openSettings"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		return state.effect(func(view *ui.Context) {
			if _, available := state.packageInfo(); !available {
				return
			}
			root := state.runtime.localRuntime()
			root.App.Host = state.runtime.controller.Target().String()
			root.App.Settings.Open(settings.Plugins)
			root.App.Navigate(shell.SettingsPage)
			root.packagesFrame(view)
			root.App.Settings.ShowDetails(settings.DetailPlugin, state.scope.Package.Name)
		})
	}
	operations["sailry/ui.selectSettingsHost"] = func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		text, err := pluginNativeArgument[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		decoded, err := hex.DecodeString(text)
		var id protocol.NodeID
		if err != nil || len(decoded) != len(id) {
			return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid Node ID")
		}
		copy(id[:], decoded)
		if !state.scope.ControllerSettings() {
			return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "settings host selection requires a settings view")
		}
		if state.runtime.runtimeFor(id.String()) == nil {
			return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "settings host is unavailable")
		}
		return state.effect(func(view *ui.Context) {
			root := state.runtime.localRuntime()
			root.App.Host = id.String()
			root.packagesFrame(view)
		})
	}
	operations["sailry/forms.openConfiguration"] = func(ctx context.Context, _ []json.RawMessage) (json.RawMessage, error) {
		if !state.scope.ControllerSettings() {
			return nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "configuration requires this package's settings view")
		}
		return state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
			if info, available := state.packageInfo(); !available || info.Settings == nil {
				return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin configuration is unavailable")
			}
			root := state.runtime.localRuntime()
			if root.App.Page != shell.SettingsPage || root.packageOwner() != state.runtime {
				return nil, pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin settings view is inactive")
			}
			state.runtime.configurePackage(state.scope.Package.Name)
			return json.RawMessage("null"), nil
		})
	}
	operations["sailry/ui.closePanel"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		return state.effect(func(*ui.Context) { state.runtime.App.PanelOpen = false })
	}
	operations["sailry/ui.showWorkspaceDetails"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		return state.effect(func(*ui.Context) { state.runtime.App.PanelOpen = true })
	}
	operations["sailry/ui.writeClipboard"] = func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		text, err := pluginNativeArgument[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		return state.effect(func(view *ui.Context) { view.WriteClipboard(text) })
	}
	operations["sailry/ui.toast"] = state.showToast
	operations["sailry/ui.dismissToast"] = state.dismissToast
	operations["sailry/ui.nextToastEvent"] = func(ctx context.Context, _ []json.RawMessage) (json.RawMessage, error) {
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-state.native.Context.Done():
			return nil, state.native.Context.Err()
		case event := <-state.toastEvents:
			return event, nil
		}
	}
	operations["sailry/ui.canInvokeContribution"] = func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		intent, err := pluginNativeArgument[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		available, err := state.runtime.pluginIntent(ctx, state.scope, state.presentation, intent, nil)
		if err != nil {
			return nil, err
		}
		return pluginNativeJSON(available)
	}
	operations["sailry/ui.invokeContribution"] = func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		intent, err := pluginNativeArgument[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		value := json.RawMessage("null")
		if len(arguments) > 1 {
			value = arguments[1]
		}
		invoked, err := state.runtime.pluginIntent(ctx, state.scope, state.presentation, intent, &value)
		if err != nil {
			return nil, err
		}
		return pluginNativeJSON(invoked)
	}
}

func (state *pluginNativeState) openSession(id protocol.SessionID) {
	if _, available := state.packageInfo(); !available {
		return
	}
	if _, exists := state.runtime.session(id.String()); !exists {
		return
	}
	state.runtime.project()
	conversation := state.runtime.App.Data.Find(id.String())
	if conversation != nil && conversation.Node == state.runtime.controller.Target().String() {
		state.runtime.OpenSession(id.String())
	}
}

func (state *pluginNativeState) showToast(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	value, err := pluginNativeArgument[pluginNativeToast](arguments, 0)
	if err != nil {
		return nil, err
	}
	if value.ID == "" || strings.TrimSpace(value.Message) == "" || len(value.Message) > 4096 ||
		(value.Kind != "info" && value.Kind != "error") ||
		(value.Action != nil && (value.Action.ID == "" || strings.TrimSpace(value.Action.Label) == "" || len(value.Action.Label) > 512)) {
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid plugin notification")
	}
	state.mu.Lock()
	if _, exists := state.toasts[value.ID]; !exists && len(state.toasts) >= 16 {
		state.mu.Unlock()
		return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "plugin notification capacity exhausted")
	}
	state.toastSequence++
	sequence := state.toastSequence
	state.toasts[value.ID] = sequence
	state.mu.Unlock()
	return state.effect(func(view *ui.Context) {
		state.mu.Lock()
		current := state.toasts[value.ID] == sequence
		state.mu.Unlock()
		if !current {
			return
		}
		toast := ui.Toast{
			ID: state.toastID(value.ID), Title: value.Message, Type: value.Kind,
			Timeout: kit.ToastDuration(view),
			OnClose: func() {
				state.mu.Lock()
				if state.toasts[value.ID] == sequence {
					delete(state.toasts, value.ID)
				}
				state.mu.Unlock()
			},
		}
		if value.Action != nil {
			toast.Action = value.Action.Label
			toast.OnAction = func() {
				if state.native.Context.Err() != nil {
					return
				}
				body, err := pluginNativeJSON(map[string]string{"id": value.ID, "action": value.Action.ID})
				if err == nil {
					select {
					case state.toastEvents <- body:
					default:
					}
				}
			}
		}
		view.AddToast(toast)
	})
}

func (state *pluginNativeState) dismissToast(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	id, err := pluginNativeArgument[string](arguments, 0)
	if err != nil {
		return nil, err
	}
	state.mu.Lock()
	delete(state.toasts, id)
	state.mu.Unlock()
	return state.effect(func(view *ui.Context) { view.CloseToast(state.toastID(id)) })
}

func (state *pluginNativeState) toastID(id string) string {
	return fmt.Sprintf("plugin/%p/%s", state, id)
}
