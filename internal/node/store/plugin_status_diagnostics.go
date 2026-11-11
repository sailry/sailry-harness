package store

import (
	"context"
	"database/sql"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func RecordPluginStatusDiagnostic(ctx context.Context, db *sql.Conn, report protocol.NotificationSystemSystemDiagnostic, timestamp int64) ([]protocol.Event, error) {
	notice, created, err := recordDiagnostic(ctx, db, report, timestamp)
	if err != nil || !created {
		return nil, err
	}
	return []protocol.Event{{Kind: "notification_changed", Data: *notice}}, nil
}

// reportDiagnostic persists a controller's system report. A cancellation is
// not an incident, and a repeated incident answers with its first notice.
func reportDiagnostic(ctx context.Context, db *sql.Conn, request protocol.Request, timestamp int64) (protocol.Output, []protocol.Event, error) {
	if request.Plugin != nil {
		return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "system diagnostics require a controller")
	}
	notice, created, err := recordDiagnostic(ctx, db, *request.Command.Data.(*protocol.NotificationSystemSystemDiagnostic), timestamp)
	if err != nil || notice == nil {
		return protocol.Output{Kind: "notifications_updated"}, nil, err
	}
	output := protocol.Output{Kind: "notification", Data: *notice}
	if !created {
		return output, nil, nil
	}
	return output, []protocol.Event{{Kind: "notification_changed", Data: *notice}}, nil
}

// recordDiagnostic returns the incident's notice and whether this call created
// it. A cancellation has no notice.
func recordDiagnostic(ctx context.Context, db *sql.Conn, report protocol.NotificationSystemSystemDiagnostic, timestamp int64) (*protocol.NotificationNotice, bool, error) {
	if !pluginStatusDiagnosticValid(report) {
		return nil, false, invalid("invalid system diagnostic")
	}
	if report.Code == protocol.ErrorCodeCancelled || report.Reason == protocol.NotificationSystemDiagnosticReasonCancelled && report.Code != protocol.ErrorCodeOutcomeUnknown {
		return nil, false, nil
	}
	var session *protocol.Session
	if report.Session != nil {
		value, err := ReadSession(ctx, db, *report.Session)
		if err != nil {
			return nil, false, err
		}
		session = &value
	}
	if report.Worktree != nil {
		worktrees, err := ReadWorktrees(ctx, db)
		if err != nil {
			return nil, false, err
		}
		found := false
		for _, worktree := range worktrees {
			found = found || worktree.ID == *report.Worktree
		}
		if !found {
			return nil, false, fault(protocol.ErrorCodeNotFound, "diagnostic worktree is unavailable")
		}
		if session != nil && session.Worktree != *report.Worktree {
			return nil, false, invalid("diagnostic resources do not share an owner")
		}
	}
	if report.Turn != nil {
		run, err := ReadAgentRun(ctx, db, *report.Turn)
		if err != nil {
			return nil, false, err
		}
		if report.Session == nil || *report.Session != run.Session {
			return nil, false, invalid("diagnostic turn belongs to another session")
		}
	}
	notices, err := ReadNotifications(ctx, db)
	if err != nil {
		return nil, false, err
	}
	for _, notice := range notices {
		if notice.Content.Details == nil || notice.Content.Details.Kind != "system" {
			continue
		}
		existing, valid := dispatchValue[protocol.NotificationSystemSystemDiagnostic](notice.Content.Details.Data)
		if valid && existing.Incident == report.Incident {
			return &notice, false, nil
		}
	}
	sequence, err := NextCounter(ctx, db, CounterNotifications)
	if err != nil {
		return nil, false, err
	}
	kind := protocol.NotificationKindError
	if report.Code == protocol.ErrorCodeOutcomeUnknown {
		kind = protocol.NotificationKindWarning
	}
	notice := protocol.NotificationNotice{
		ID: protocol.NewNotificationID(), Sequence: sequence, TimestampMs: timestamp,
		Content: protocol.NotificationDraft{
			Kind: kind, Session: report.Session, Details: &protocol.NotificationDetails{Kind: "system", Data: report},
		},
	}
	body, err := encode(notice)
	if err != nil {
		return nil, false, err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO notifications(sequence,id,body) VALUES(?1,?2,?3)", sequence, notice.ID.String(), body); err != nil {
		return nil, false, err
	}
	if _, err := db.ExecContext(ctx, "DELETE FROM notifications WHERE sequence NOT IN (SELECT sequence FROM notifications ORDER BY sequence DESC LIMIT 200)"); err != nil {
		return nil, false, err
	}
	return &notice, true, nil
}

func pluginStatusDiagnosticValid(report protocol.NotificationSystemSystemDiagnostic) bool {
	token := func(value string, maximum int) bool {
		return value != "" && len(value) <= maximum && !strings.ContainsFunc(value, func(character rune) bool {
			return !(character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' ||
				character >= '0' && character <= '9' || strings.ContainsRune("._-+", character))
		})
	}
	return token(report.Version, 64) && token(report.Platform, 64) &&
		(report.Plugin == nil || token(report.Plugin.Name, 256) && token(report.Plugin.Version, 64)) &&
		(report.Operation == nil || token(*report.Operation, 128)) &&
		(report.BudgetMs == nil || *report.BudgetMs <= 86400000) &&
		(report.ElapsedMs == nil || *report.ElapsedMs <= 86400000)
}
