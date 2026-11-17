package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/node/plugins"
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
type pluginFollowConnection struct {
	Package       protocol.PluginReference          `json:"package"`
	ID            protocol.PluginConnectorID        `json:"id"`
	Connector     string                            `json:"connector"`
	Method        protocol.PluginConnectorKind      `json:"method"`
	Enabled       bool                              `json:"enabled"`
	Fields        map[string]string                 `json:"fields"`
	Revision      uint64                            `json:"revision"`
	AuthRevision  uint64                            `json:"auth_revision"`
	GrantRevision uint64                            `json:"grant_revision"`
	Identity      *protocol.PluginConnectorIdentity `json:"identity,omitempty"`
	Bound         *ConnectorBound                   `json:"bound,omitempty"`
	State         protocol.PluginConnectorState     `json:"state"`
	CheckedAtMs   *uint64                           `json:"checked_at_ms,omitempty"`
	Login         *protocol.RequestID               `json:"login,omitempty"`
}

func (value pluginFollowConnection) runtimeRecord() ConnectorRecord {
	return ConnectorRecord{
		ID: value.ID, Plugin: value.Package.Name, Connector: value.Connector, Revision: value.Revision,
		AuthRevision: value.AuthRevision, GrantRevision: value.GrantRevision, Method: value.Method,
		Enabled: value.Enabled, Fields: value.Fields, Identity: value.Identity, Bound: value.Bound,
		State: value.State, CheckedAtMs: value.CheckedAtMs, Login: value.Login,
	}
}

// CacheConfigurationPackage records metadata only after Node has materialized
// and verified the content-addressed package outside the category transaction.
func CacheConfigurationPackage(ctx context.Context, db *sql.Conn, info protocol.PluginInfo, declared protocol.PluginMcpConfiguration) error {
	if !plugins.ValidateName(info.Summary.Name) || len(info.Summary.Digest) != 64 {
		return invalid("invalid followed package identity")
	}
	if len(info.Mcp) > 0 {
		if err := validateMCPConfiguration(info, declared); err != nil {
			return err
		}
	}
	info.Summary.Source = nil
	body, err := encode(info)
	if err != nil {
		return err
	}
	mcp, err := encode(declared)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO configuration_follow_packages(digest,name,body,mcp) VALUES(?1,?2,?3,?4) ON CONFLICT(digest) DO UPDATE SET body=excluded.body,mcp=excluded.mcp WHERE name=excluded.name", info.Summary.Digest, info.Summary.Name, body, mcp)
	return err
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
