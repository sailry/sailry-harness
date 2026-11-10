package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// databaseBinding describes where a stored password is sent: the exact saved
// connection and, for a tunnel, the SSH destination as it is saved now. A
// profile identifier alone never authorizes password reuse.
func databaseBinding(ctx context.Context, db *sql.Conn, connection protocol.DatabaseConnection) (databaseCredentialBinding, error) {
	binding := databaseCredentialBinding{Connection: connection}
	if connection.Kind != "ssh" {
		return binding, nil
	}
	target, valid := dispatchValue[protocol.DatabaseConnectionSshPayload](connection.Data)
	if !valid {
		return binding, invalid("invalid database connection")
	}
	profile, err := readBody[protocol.SshProfile](ctx, db, "SELECT body FROM ssh_profiles WHERE id=?1", target.Ssh.String())
	if err != nil {
		return binding, err
	}
	if profile == nil {
		return binding, fault(protocol.ErrorCodeNotConfigured, "database tunnel is unavailable")
	}
	binding.Tunnel = &databaseCredentialTunnel{Host: profile.Host, Port: profile.Port, Username: profile.Username, Authentication: profile.Authentication}
	return binding, nil
}

func captureDatabaseCredential(ctx context.Context, db *sql.Conn, connection protocol.DatabaseConnection) ([]byte, error) {
	binding, err := databaseBinding(ctx, db, connection)
	if err != nil {
		return nil, err
	}
	return encode(binding)
}

// checkDatabaseCredential permits reusing the stored password only for the
// target it was entered for. A changed database connection or SSH host, port,
// username or authentication requires a new password.
func checkDatabaseCredential(ctx context.Context, db *sql.Conn, id protocol.DatabaseID, connection protocol.DatabaseConnection) error {
	var stored []byte
	if err := db.QueryRowContext(ctx, "SELECT password_target FROM database_profiles WHERE id=?1", id.String()).Scan(&stored); err != nil {
		return err
	}
	expected, err := captureDatabaseCredential(ctx, db, connection)
	if err != nil {
		return err
	}
	matches := false
	if stored != nil {
		var current *databaseCredentialBinding
		if err := json.Unmarshal(stored, &current); err != nil {
			return err
		}
		if current != nil {
			// Comparing canonical encodings is independent of stored key order.
			body, err := encode(current)
			if err != nil {
				return err
			}
			matches = bytes.Equal(body, expected)
		}
	}
	if !matches {
		return fault(protocol.ErrorCodeNotConfigured, "database credential target changed; enter a password")
	}
	return nil
}
