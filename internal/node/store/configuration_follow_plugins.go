package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginFollowSettings struct {
	Package protocol.PluginReference        `json:"package"`
	Values  map[string]json.RawMessage      `json:"values"`
	MCP     protocol.PluginMcpConfiguration `json:"mcp"`
	Allowed map[string]bool                 `json:"allowed"`
}
type pluginFollowSecrets struct {
	Slots map[string]protocol.Secret `json:"slots"`
}

func followedPackage(ctx context.Context, db *sql.Conn, reference protocol.PluginReference) (protocol.PluginInfo, protocol.PluginMcpConfiguration, error) {
	var info protocol.PluginInfo
	var declared protocol.PluginMcpConfiguration
	var body, mcp []byte
	err := db.QueryRowContext(ctx, "SELECT body,mcp FROM configuration_follow_packages WHERE digest=?1 AND name=?2", reference.Digest, reference.Name).Scan(&body, &mcp)
	if errors.Is(err, sql.ErrNoRows) {
		err = fault(protocol.ErrorCodeNotConfigured, "plugin package content is pending")
	}
	if err != nil {
		return info, declared, err
	}
	if err = json.Unmarshal(body, &info); err != nil {
		return info, declared, err
	}
	err = json.Unmarshal(mcp, &declared)
	return info, declared, err
}
