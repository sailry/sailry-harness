package store

import (
	"context"
	"database/sql"
	"runtime"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// System notices are committed with the state transition that produced them.

// insertNotice stores one durable notice and keeps the newest entries.
func insertNotice(ctx context.Context, db *sql.Conn, packageName string, content protocol.NotificationDraft, timestamp int64) (protocol.NotificationNotice, error) {
	sequence, err := NextCounter(ctx, db, CounterNotifications)
	if err != nil {
		return protocol.NotificationNotice{}, err
	}
	value := protocol.NotificationNotice{ID: protocol.NewNotificationID(), Sequence: sequence, Package: packageName, Content: content, TimestampMs: timestamp}
	body, err := encode(value)
	if err != nil {
		return value, err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO notifications(sequence,id,body) VALUES(?1,?2,?3)", sequence, value.ID.String(), body); err != nil {
		return value, err
	}
	_, err = db.ExecContext(ctx, "DELETE FROM notifications WHERE sequence NOT IN (SELECT sequence FROM notifications ORDER BY sequence DESC LIMIT ?1)", protocol.NotificationLimit)
	return value, err
}

// runNotice reports a finished main run: completed, failed or interrupted. A
// cancelled run was stopped by its user and a delegated run reports to its
// parent, so neither has a notice. A failure that a diagnostic of the same
// turn already explains is not reported twice.
func runNotice(ctx context.Context, db *sql.Conn, run protocol.ConversationRun) ([]protocol.Event, error) {
	switch run.Status {
	case protocol.ConversationStatusCompleted, protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted:
	default:
		return nil, nil
	}
	session, err := ReadSession(ctx, db, run.Session)
	if err != nil {
		return nil, err
	}
	if session.Delegation != nil {
		return nil, nil
	}
	kind := protocol.NotificationKindSuccess
	if run.Status != protocol.ConversationStatusCompleted {
		kind = protocol.NotificationKindError
		explained, err := runExplained(ctx, db, run)
		if err != nil || explained {
			return nil, err
		}
	}
	details := protocol.NotificationDetailsTask{Turn: run.Turn, Status: run.Status, StartedMs: run.StartedMs, FinishedMs: run.FinishedMs}
	if run.Error != nil {
		details.Error = &run.Error.Code
	}
	timestamp := int64(0)
	if run.FinishedMs != nil {
		timestamp = *run.FinishedMs
	}
	notice, err := insertNotice(ctx, db, "", protocol.NotificationDraft{
		Title: session.Activity.Title, Kind: kind, Session: &run.Session,
		Details: &protocol.NotificationDetails{Kind: "task", Data: &details},
	}, timestamp)
	if err != nil {
		return nil, err
	}
	return []protocol.Event{{Kind: "notification_changed", Data: notice}}, nil
}

// runExplained reports whether a compaction failure that stopped the turn or
// a system diagnostic of the run's own incident already told this failure.
func runExplained(ctx context.Context, db *sql.Conn, run protocol.ConversationRun) (bool, error) {
	failures, err := readBodies[protocol.ConversationCompactionFailure](ctx, db, `SELECT json_extract(body,'$.metadata.compaction.failure') FROM agent_events
		WHERE turn=?1 AND json_extract(body,'$.provenance.author')='system' AND json_type(body,'$.metadata.compaction.failure')='object'`, run.Turn.String())
	if err != nil {
		return false, err
	}
	for _, failure := range failures {
		if !failure.Continued {
			return true, nil
		}
	}
	if run.Error == nil || run.Error.Details == nil || run.Error.Details.Kind != "system_diagnostic" {
		return false, nil
	}
	report, valid := dispatchValue[protocol.NotificationSystemSystemDiagnostic](run.Error.Details.Data)
	if !valid {
		return false, nil
	}
	notices, err := ReadNotifications(ctx, db)
	if err != nil {
		return false, err
	}
	for _, notice := range notices {
		if notice.Content.Details == nil || notice.Content.Details.Kind != "system" {
			continue
		}
		existing, valid := dispatchValue[protocol.NotificationSystemSystemDiagnostic](notice.Content.Details.Data)
		if valid && existing.Incident == report.Incident && existing.Turn != nil && *existing.Turn == run.Turn {
			return true, nil
		}
	}
	return false, nil
}

// compactionNotice reports a compaction failure the system recorded in the
// conversation as a diagnostic of that entry.
func compactionNotice(ctx context.Context, db *sql.Conn, session protocol.SessionID, record canonical.Record, sequence uint64) ([]protocol.Event, error) {
	if record.Provenance.Author != "system" || record.Metadata.Compaction == nil || record.Metadata.Compaction.Failure == nil {
		return nil, nil
	}
	notice, err := insertNotice(ctx, db, "", protocol.NotificationDraft{
		Kind: protocol.NotificationKindError, Session: &session,
		Details: &protocol.NotificationDetails{Kind: "diagnostic", Data: &protocol.NotificationDetailsDiagnostic{
			Event: record.ID, Sequence: sequence, Turn: record.Turn,
			Cause:   protocol.NotificationDetailsCause{Kind: "compaction", Data: record.Metadata.Compaction.Failure},
			Version: link.ProductVersion, Platform: runtime.GOOS + "-" + runtime.GOARCH,
		}},
	}, record.Created.UnixMilli())
	if err != nil {
		return nil, err
	}
	return []protocol.Event{{Kind: "notification_changed", Data: notice}}, nil
}
