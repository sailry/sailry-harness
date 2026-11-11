package live

import (
	"context"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// createPanelResource is the live panel's New for a terminal or a browser
// page. A terminal is a core Node terminal in the selected worktree; a
// browser page is the panel's own once the window has a WebView.
func (r *Runtime) createPanelResource(kind panel.Kind) bool {
	if kind != panel.Terminal && kind != panel.Browser {
		return false
	}
	root := r.localRuntime()
	if kind == panel.Browser && root.browser != nil {
		return false
	}
	owner := root.selectedRuntime()
	if owner == nil || !owner.controller.Connected() || owner.workspace == nil || owner.workspace.selected == (protocol.WorktreeID{}) {
		if kind == panel.Terminal {
			// The panel offers no terminal here (panel.State.Worktree); one
			// asked for all the same names why it cannot start.
			root.terminalUnavailable()
			return true
		}
		root.Report(pluginNativeFault(protocol.ErrorCodeUnavailable, "resource creation requires an available workspace"))
		return true
	}
	if kind == panel.Terminal {
		root.createLiveTerminal(owner, owner.workspace.selected)
		return true
	}
	root.Report(pluginNativeFault(protocol.ErrorCodeUnavailable, "workspace resource renderer is unavailable"))
	return true
}

// bindPluginPanes answers the SDK's pane calls. The only pane resource of
// the v1 contract is a terminal, which the core owns: no package view is a
// workspace pane and none can open one. The other pane calls report the
// binding's ordinary unavailable fault.
func bindPluginPanes(native *plugins.NativeBinding) {
	native.Operations["sailry/ui.pane"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		return json.RawMessage("null"), nil
	}
	native.Operations["sailry/ui.openPane"] = func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		return json.RawMessage("false"), nil
	}
}
