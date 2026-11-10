package store

import (
	"context"
	"database/sql"
	"math"

	"github.com/sailry/sailry-harness/internal/node/python"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func ReadPythonSettings(ctx context.Context, db *sql.Conn) (protocol.PythonSettings, error) {
	value, err := readBody[protocol.PythonSettings](ctx, db, "SELECT body FROM python_settings WHERE singleton=1")
	if err != nil {
		return protocol.PythonSettings{}, err
	}
	if value == nil {
		return protocol.DefaultPythonSettings(), nil
	}
	if value.Revision > math.MaxInt64 {
		return protocol.PythonSettings{}, invalid("invalid Python settings revision")
	}
	if err := python.ValidateIndex(value.Index); err != nil {
		return protocol.PythonSettings{}, err
	}
	return *value, nil
}

func SavePythonSettings(ctx context.Context, db *sql.Conn, input protocol.PythonSettings) (protocol.Output, []protocol.Event, error) {
	if input.Revision > math.MaxInt64 {
		return protocol.Output{}, nil, invalid("invalid Python settings revision")
	}
	if err := python.ValidateIndex(input.Index); err != nil {
		return protocol.Output{}, nil, err
	}
	current, err := ReadPythonSettings(ctx, db)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if err := checkRevision(current.Revision, input.Revision); err != nil {
		return protocol.Output{}, nil, err
	}
	input.Revision, err = nextRevision(input.Revision)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	body, err := encode(input)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO python_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body)
	return protocol.Output{Kind: "python_settings", Data: protocol.PythonInfo{Settings: input, Version: python.Version}},
		[]protocol.Event{{Kind: "python_settings_changed", Data: protocol.EventPythonSettingsChangedPayload{Revision: input.Revision}}}, err
}
