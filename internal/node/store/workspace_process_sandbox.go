package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func ReadSandboxSettings(ctx context.Context, db *sql.Conn) (protocol.SandboxSettings, error) {
	value, err := readBody[protocol.SandboxSettings](ctx, db, "SELECT body FROM sandbox_settings WHERE singleton=1")
	if value == nil {
		return protocol.DefaultSandboxSettings(), err
	}
	return *value, err
}
func SaveSandboxSettings(ctx context.Context, db *sql.Conn, input protocol.SandboxSettings) (protocol.SandboxSettings, error) {
	current, err := ReadSandboxSettings(ctx, db)
	if err != nil {
		return protocol.SandboxSettings{}, err
	}
	if err := checkRevision(current.Revision, input.Revision); err != nil {
		return protocol.SandboxSettings{}, err
	}
	input.Revision, err = nextRevision(input.Revision)
	if err != nil {
		return protocol.SandboxSettings{}, err
	}
	body, err := encode(input)
	if err != nil {
		return protocol.SandboxSettings{}, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO sandbox_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body)
	return input, err
}
