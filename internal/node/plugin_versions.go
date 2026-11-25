package node

import (
	"context"
	"database/sql"
	"io"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) pluginUpdateCurrent(ctx context.Context, name string, expected uint64) (*protocol.PluginInfo, error) {
	var info *protocol.PluginInfo
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		info, err = store.ReadPlugin(ctx, db, name)
		return err
	})
	if err != nil {
		return nil, err
	}
	if info == nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "plugin is not installed"}
	}
	if info.Summary.Revision != expected {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "plugin changed before update check"}
	}
	return info, nil
}

func (node *Node) checkUploadedPluginUpdate(ctx context.Context, caller protocol.NodeID, input protocol.CommandCheckPluginUpdatePayload) (protocol.PluginUpdatesReport, error) {
	current, err := node.pluginUpdateCurrent(ctx, input.Name, input.ExpectedRevision)
	if err != nil {
		return protocol.PluginUpdatesReport{}, err
	}
	if current.Summary.Pinned {
		return plugins.AssessUpdate(*current, protocol.PluginInfo{}, plugins.RuntimeFromContext(ctx))
	}
	var candidate protocol.PluginInfo
	err = node.transfers.UsePluginUpload(ctx, caller, *input.Upload, false, func(reader io.ReaderAt, size int64) error {
		var err error
		candidate, err = node.plugins.manager.InspectArchive(ctx, reader, size, input.Name)
		return err
	})
	if err != nil {
		return protocol.PluginUpdatesReport{}, err
	}
	return plugins.AssessUpdate(*current, candidate, plugins.RuntimeFromContext(ctx))
}

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
