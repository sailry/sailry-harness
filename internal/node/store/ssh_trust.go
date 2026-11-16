package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func readSshTrust(ctx context.Context, db *sql.Conn, profile *protocol.SshProfile) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	profile.HostKey = nil
	var body []byte
	err = db.QueryRowContext(ctx, "SELECT key FROM ssh_local_trust WHERE user=?1 AND profile=?2 AND host=?3 AND port=?4",
		user.ID.String(), profile.ID.String(), profile.Host, profile.Port).Scan(&body)
	if errors.Is(err, sql.ErrNoRows) {
		return nil
	}
	if err != nil {
		return err
	}
	return json.Unmarshal(body, &profile.HostKey)
}

func saveSshTrust(ctx context.Context, db *sql.Conn, profile protocol.SshProfile, key protocol.SshHostKey) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	body, err := json.Marshal(key)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, `INSERT INTO ssh_local_trust(user,profile,host,port,key) VALUES(?1,?2,?3,?4,?5)
ON CONFLICT(user,profile,host,port) DO UPDATE SET key=excluded.key`,
		user.ID.String(), profile.ID.String(), profile.Host, profile.Port, body)
	return err
}
