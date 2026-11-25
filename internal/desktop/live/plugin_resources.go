package live

import (
	"context"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// createPanelBrowser handles New when the live panel has no WebView.
func (r *Runtime) createPanelBrowser(kind panel.Kind) bool {
	if kind != panel.Browser {
		return false
	}
	root := r.localRuntime()
	if root.browser != nil {
		return false
	}
	owner := root.selectedRuntime()
	if owner == nil || !owner.controller.Connected() || owner.workspace == nil || owner.workspace.selected == (protocol.WorktreeID{}) {
		root.Report(pluginNativeFault(protocol.ErrorCodeUnavailable, "resource creation requires an available workspace"))
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
