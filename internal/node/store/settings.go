package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func ReadPower(ctx context.Context, db *sql.Conn) (protocol.PowerSettings, error) {
	value, err := readBody[protocol.PowerSettings](ctx, db, "SELECT body FROM power_settings WHERE singleton=1")
	if value == nil {
		return protocol.DefaultPowerSettings(), err
	}
	return *value, err
}

func savePower(ctx context.Context, db *sql.Conn, input protocol.PowerSettings) (protocol.Output, []protocol.Event, error) {
	current, err := ReadPower(ctx, db)
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
	_, err = db.ExecContext(ctx, "INSERT INTO power_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body)
	return protocol.Output{Kind: "power_settings", Data: input}, []protocol.Event{{Kind: "power_settings_changed", Data: protocol.EventPowerSettingsChangedPayload{Revision: input.Revision}}}, err
}

func ReadTelemetry(ctx context.Context, db *sql.Conn) (protocol.TelemetrySettings, error) {
	value, err := readBody[protocol.TelemetrySettings](ctx, db, "SELECT body FROM telemetry_settings WHERE singleton=1")
	if err != nil {
		return protocol.TelemetrySettings{}, err
	}
	if value == nil {
		return protocol.TelemetrySettings{}, fault(protocol.ErrorCodeInternal, "telemetry settings are missing")
	}
	return *value, validateTelemetry(*value)
}

func saveTelemetry(ctx context.Context, db *sql.Conn, input protocol.TelemetryUpdate) (protocol.Output, []protocol.Event, error) {
	value, err := ReadTelemetry(ctx, db)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if err := checkRevision(value.Revision, input.Revision); err != nil {
		return protocol.Output{}, nil, err
	}
	value.Errors, value.Metrics = input.Errors, input.Metrics
	value.Revision, err = nextRevision(value.Revision)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	body, err := encode(value)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	_, err = db.ExecContext(ctx, "UPDATE telemetry_settings SET body=?1 WHERE singleton=1", body)
	return protocol.Output{Kind: "telemetry_settings", Data: value}, []protocol.Event{{Kind: "telemetry_settings_changed", Data: protocol.EventTelemetrySettingsChangedPayload{Revision: value.Revision}}}, err
}
