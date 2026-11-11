package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// pluginExternalBrowserAction names the software action a managed browser
// call needs. Browser authority follows the package as it is installed now,
// while code and settings keep the admitted revision.
func pluginExternalBrowserAction(ctx context.Context, db *sql.Conn, request protocol.Request) (protocol.PluginAction, bool, error) {
	input, ok := request.Command.Data.(*protocol.CommandUseExternalBrowserPayload)
	if !ok {
		return "", false, nil
	}
	if !request.Plugin.ControllerSettings() {
		current, err := ReadPlugin(ctx, db, request.Plugin.Package.Name)
		if err != nil {
			return "", false, err
		}
		if current == nil || !current.Summary.Enabled {
			return "", false, fault(protocol.ErrorCodeNotConfigured, "plugin is disabled")
		}
	}
	if input.Action.ReadOnly() {
		return protocol.PluginActionReadExternalBrowser, true, nil
	}
	return protocol.PluginActionControlExternalBrowser, true, nil
}
