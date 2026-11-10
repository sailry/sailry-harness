package store

import (
	"context"
	"database/sql"
	"errors"
	"math"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// OpenCodeIndex opens one worktree's derived code index. The file is a cache:
// a caller may delete it and open it again.
func OpenCodeIndex(ctx context.Context, path string) (*Handle, error) {
	handle, err := OpenHandle(ctx, path)
	if err != nil {
		return nil, err
	}
	if err := handle.Write(ctx, func(connection *sql.Conn) error {
		if _, err := connection.ExecContext(ctx, sqliteSchema(CodeSchema)); err != nil {
			return err
		}
		var version int64
		if err := connection.QueryRowContext(ctx, "SELECT version FROM metadata WHERE singleton=1").Scan(&version); err != nil {
			return err
		}
		if version != 1 {
			return errors.New("unexpected derived index format")
		}
		return nil
	}); err != nil {
		_ = handle.Close()
		return nil, err
	}
	return handle, nil
}

// ReadCodeIndexSettings returns the saved enablement, enabled by default.
func ReadCodeIndexSettings(ctx context.Context, db *sql.Conn) (protocol.CodeIndexSettings, error) {
	value, err := readBody[protocol.CodeIndexSettings](ctx, db, "SELECT body FROM code_index_settings WHERE singleton=1")
	if err != nil {
		return protocol.CodeIndexSettings{}, err
	}
	if value == nil {
		return protocol.CodeIndexSettings{Enabled: true}, nil
	}
	return *value, nil
}

// SaveCodeIndexSettings replaces the settings when both the edited value and
// the stored one carry the expected revision.
func SaveCodeIndexSettings(ctx context.Context, db *sql.Conn, input protocol.CodeIndexSettings, expected uint64) (protocol.CodeIndexSettings, error) {
	current, err := ReadCodeIndexSettings(ctx, db)
	if err != nil {
		return protocol.CodeIndexSettings{}, err
	}
	if input.Revision != expected || current.Revision != expected {
		return protocol.CodeIndexSettings{}, fault(protocol.ErrorCodeRevisionConflict, "code index settings changed before saving")
	}
	if expected >= math.MaxInt64 {
		return protocol.CodeIndexSettings{}, invalid("code index revision is exhausted")
	}
	input.Revision = expected + 1
	body, err := encode(input)
	if err != nil {
		return protocol.CodeIndexSettings{}, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO code_index_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body)
	return input, err
}
