package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// PersonalKnowledgeChanged updates derived registrations after an accepted
// personal write set. The package currently installed by the controller decides
// indexing; an old executor capture cannot reinstall or enable that package.
// It runs in the source transaction and never executes the original command.
func PersonalKnowledgeChanged(ctx context.Context, db *sql.Conn, name string) (bool, error) {
	info, err := ReadPlugin(ctx, db, name)
	if err != nil || info == nil || !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" {
		return false, err
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return false, err
	}
	ctx = WithCapturedUser(ctx, user.ID)
	request := protocol.Request{
		Plugin: &protocol.PluginContext{Package: plugins.PackageReference(*info)},
		Command: protocol.Command{Kind: "write_plugin_value", Data: &protocol.CommandWritePluginValuePayload{
			Scope: protocol.PluginStorageScopeNode,
		}},
	}
	return KnowledgeStorageChanged(ctx, db, request)
}
