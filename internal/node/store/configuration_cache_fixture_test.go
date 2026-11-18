package store

import (
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Seed the persisted format directly. Production only reads these historical rows.
func seedHistoricalConfiguration(ctx context.Context, db *sql.Conn, category, id string, revision uint64, value any) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	body, err := json.Marshal(value)
	if err != nil {
		return err
	}
	checksum := sha256.Sum256(body)
	args := []any{user.ID.String(), category, id, revision, hex.EncodeToString(checksum[:]), body}
	if _, err := db.ExecContext(ctx, `INSERT INTO configuration_cache_history(user,category,id,revision,digest,body)
VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(user,category,id,revision) DO NOTHING`, args...); err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, `INSERT INTO configuration_cache(user,category,id,revision,digest,body)
VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(user,category,id) DO UPDATE SET
revision=excluded.revision,digest=excluded.digest,body=excluded.body`, args...)
	return err
}

func seedHistoricalRuntimeCredentials(ctx context.Context, db *sql.Conn, credentials ...protocol.RuntimeCredential) error {
	for _, credential := range credentials {
		if err := seedHistoricalConfiguration(ctx, db, "runtime_credentials", credential.CacheID(), credential.Revision, credential); err != nil {
			return err
		}
	}
	return nil
}

func seedHistoricalPluginPackage(ctx context.Context, db *sql.Conn, info protocol.PluginInfo, declared protocol.PluginMcpConfiguration) error {
	body, err := json.Marshal(info)
	if err != nil {
		return err
	}
	mcp, err := json.Marshal(declared)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO configuration_follow_packages(digest,name,body,mcp) VALUES(?1,?2,?3,?4)", info.Summary.Digest, info.Summary.Name, body, mcp)
	return err
}
