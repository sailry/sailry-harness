package store

import (
	"context"
	"database/sql"
	"fmt"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func storageNode(ctx context.Context, db *sql.Conn) (protocol.NodeID, error) {
	var raw []byte
	if err := db.QueryRowContext(ctx, "SELECT identity FROM node WHERE singleton=1").Scan(&raw); err != nil {
		return protocol.NodeID{}, err
	}
	if len(raw) != 32 {
		return protocol.NodeID{}, fmt.Errorf("invalid storage Node identity")
	}
	return protocol.NodeID(raw), nil
}

func databasePath(connection protocol.DatabaseConnection) string {
	switch connection.Kind {
	case "sqlite":
		if value, ok := dispatchValue[protocol.DatabaseConnectionSqlitePayload](connection.Data); ok {
			return value.Path
		}
	case "socket":
		if value, ok := dispatchValue[protocol.DatabaseConnectionSocketPayload](connection.Data); ok {
			return value.Path
		}
	}
	return ""
}

func bindDatabasePath(profile *protocol.DatabaseProfile, node protocol.NodeID) error {
	path := databasePath(profile.Connection)
	if path == "" {
		if profile.Location != nil {
			return invalid("network database cannot specify a local file location")
		}
		return nil
	}
	if profile.Location == nil {
		profile.Location = &protocol.HostPath{Node: node, Path: path}
	}
	if profile.Location.Node == (protocol.NodeID{}) || profile.Location.Path != path || len(path) > 4096 || strings.ContainsRune(path, 0) {
		return invalid("database file location does not match its connection")
	}
	return nil
}

func bindSshAuthentication(profile *protocol.SshProfile, credential *protocol.SshCredential, node protocol.NodeID) error {
	if profile.Authentication != protocol.SshAuthenticationKeyPath && profile.Authentication != protocol.SshAuthenticationAgent {
		return nil
	}
	if profile.AuthenticationNode == nil {
		profile.AuthenticationNode = &node
	}
	if profile.Authentication == protocol.SshAuthenticationAgent || credential == nil {
		return nil
	}
	value, ok := dispatchValue[protocol.SshCredentialKeyPathPayload](credential.Data)
	if !ok {
		return invalid("SSH key file credential is invalid")
	}
	if value.Location == nil {
		value.Location = &protocol.HostPath{Node: *profile.AuthenticationNode, Path: value.Path.Expose()}
	}
	if value.Location.Node != *profile.AuthenticationNode || value.Location.Path != value.Path.Expose() ||
		value.Location.Path == "" || len(value.Location.Path) > 4096 || strings.ContainsRune(value.Location.Path, 0) {
		return invalid("SSH key file location does not match its authentication")
	}
	credential.Data = &value
	return nil
}

func checkHostPath(node protocol.NodeID, location *protocol.HostPath) error {
	if location != nil && location.Node != node {
		return fault(protocol.ErrorCodeWrongTarget, "local file resource belongs to Node "+location.Node.String())
	}
	return nil
}
