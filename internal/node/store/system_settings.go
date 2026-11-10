package store

import (
	"context"
	"database/sql"
	"time"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// PowerFailed records the system's refusal of the idle-sleep request as an
// error notice carrying its code.
func PowerFailed(ctx context.Context, db *sql.Conn, code int64) ([]protocol.Event, error) {
	sequence, err := NextCounter(ctx, db, CounterNotifications)
	if err != nil {
		return nil, err
	}
	value := protocol.NotificationNotice{ID: protocol.NewNotificationID(), Sequence: sequence, TimestampMs: time.Now().UnixMilli(),
		Content: protocol.NotificationDraft{Kind: protocol.NotificationKindError,
			Details: &protocol.NotificationDetails{Kind: "power", Data: &protocol.NotificationDetailsPowerPayload{Code: code}}}}
	body, err := encode(value)
	if err != nil {
		return nil, err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO notifications(sequence,id,body) VALUES(?1,?2,?3)", sequence, value.ID.String(), body); err != nil {
		return nil, err
	}
	_, err = db.ExecContext(ctx, "DELETE FROM notifications WHERE sequence NOT IN (SELECT sequence FROM notifications ORDER BY sequence DESC LIMIT 200)")
	return []protocol.Event{{Kind: "notification_changed", Data: value}}, err
}

// resetTelemetry replaces the anonymous installation identity.
func resetTelemetry(ctx context.Context, db *sql.Conn, request protocol.Request) (protocol.Output, []protocol.Event, error) {
	if request.Plugin != nil {
		return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "telemetry settings require a controller")
	}
	value, err := ReadTelemetry(ctx, db)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if err := checkRevision(value.Revision, request.Command.Data.(*protocol.CommandResetTelemetryIdentityPayload).ExpectedRevision); err != nil {
		return protocol.Output{}, nil, err
	}
	value.InstallationID = uuid.New()
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
