package live

import (
	"context"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/desktop/plugins"
)

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
