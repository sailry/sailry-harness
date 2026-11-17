package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) prepareCatalogPlugin(ctx context.Context, input protocol.CommandInstallCatalogPluginPayload) (protocol.PluginInfo, error) {
	name := input.Selection.Info.Summary.Name
	var current *protocol.PluginInfo
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		current, err = store.ReadPlugin(ctx, db, name)
		return err
	})
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	conflict := func(message string) error {
		return &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: message}
	}
	if current != nil && current.Summary.Revision != input.ExpectedRevision {
		return protocol.PluginInfo{}, conflict("plugin changed during version selection")
	}
	switch input.Mode {
	case protocol.PluginInstallModeInstall:
		if current != nil {
			return protocol.PluginInfo{}, conflict("plugin is already installed")
		}
	case protocol.PluginInstallModeUpdate:
		if current == nil {
			return protocol.PluginInfo{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "plugin is not installed"}
		}
		report, err := node.plugins.manager.CheckUpdate(ctx, *current, nil, true)
		if err != nil {
			return protocol.PluginInfo{}, err
		}
		if report.Available == nil || report.Available.Kind != "official" {
			return protocol.PluginInfo{}, conflict("no compatible plugin update is available")
		}
		entry, valid := dispatchPayload[protocol.PluginCatalogEntry](report.Available.Data)
		selected := input.Selection.Info.Summary.Version
		if !valid || entry.CompatibleVersion == nil || selected == nil || entry.CompatibleVersion.Version != *selected ||
			entry.CompatibleVersion.Commit != input.Selection.Source.Commit {
			return protocol.PluginInfo{}, conflict("plugin update selection changed; inspect the compatible candidate again")
		}
	case protocol.PluginInstallModeSwitch:
	default:
		return protocol.PluginInfo{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid plugin installation mode"}
	}
	return node.plugins.manager.InstallOfficial(ctx, input.Selection)
}
