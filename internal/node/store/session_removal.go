package store

import (
	"context"
	"database/sql"
	"errors"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// sessionRemovalScope validates a remove_session or remove_sessions command
// and lists every session it removes: the selected roots and their delegated
// descendants.
func sessionRemovalScope(ctx context.Context, db *sql.Conn, command protocol.Command) ([]protocol.SessionID, error) {
	switch input := command.Data.(type) {
	case *protocol.CommandRemoveSessionPayload:
		if _, err := checkedSession(ctx, db, input.Session, input.ExpectedRevision); err != nil {
			return nil, err
		}
		return sessionRemovalTree(ctx, db, []protocol.SessionID{input.Session}, nil)
	case *protocol.CommandRemoveSessionsPayload:
		if len(input.Expected) == 1 {
			entry := input.Expected[0]
			return sessionRemovalScope(ctx, db, protocol.Command{Kind: "remove_session", Data: &protocol.CommandRemoveSessionPayload{Session: entry.Session, ExpectedRevision: entry.Revision}})
		}
		seen := map[protocol.SessionID]bool{}
		var roots, selectedChildren []protocol.SessionID
		for _, entry := range input.Expected {
			if seen[entry.Session] {
				return nil, invalid("session batch contains duplicate sessions")
			}
			seen[entry.Session] = true
			value, err := ReadSession(ctx, db, entry.Session)
			if err == nil {
				err = checkRevision(value.Revision, entry.Revision)
			}
			if err != nil {
				return nil, err
			}
			if value.Delegation == nil {
				roots = append(roots, value.ID)
			} else {
				selectedChildren = append(selectedChildren, value.ID)
			}
		}
		return sessionRemovalTree(ctx, db, roots, selectedChildren)
	}
	return nil, invalid("command is not a session removal")
}

// activeRemovalRuns lists the turns of the sessions that are still executing.
// Queued input is not active: removal discards it.
func activeRemovalRuns(ctx context.Context, db *sql.Conn, ids []protocol.SessionID) ([]protocol.ConversationRun, error) {
	sessions, err := encode(ids)
	if err != nil {
		return nil, err
	}
	return readBodies[protocol.ConversationRun](ctx, db, "SELECT a.body FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session IN (SELECT value FROM json_each(?1)) AND json_extract(a.body,'$.status') IN ('running','stopping') ORDER BY t.rowid", string(sessions))
}

// ErrSessionRemovalActive marks the removing transaction's busy fault when a
// turn of the removal runs. The Node stops and awaits such turns first, but
// the scheduler can still claim queued input after that; the Node then rolls
// the attempt back and stops that turn as well.
var ErrSessionRemovalActive = errors.New("a turn of the session removal is active")

// StopSessionRemoval validates a session removal and stops its running turns
// on the same path as stop_turn. It returns the turns it stopped, which the
// caller hands to the runtime after the commit. A stale or invalid removal
// stops nothing.
func StopSessionRemoval(ctx context.Context, db *sql.Conn, command protocol.Command, now time.Time) ([]protocol.ConversationRun, []protocol.Event, error) {
	ids, err := sessionRemovalScope(ctx, db, command)
	if err != nil {
		return nil, nil, err
	}
	active, err := activeRemovalRuns(ctx, db, ids)
	if err != nil {
		return nil, nil, err
	}
	stopped := []protocol.ConversationRun{}
	events := []protocol.Event{}
	for _, run := range active {
		if run.Status != protocol.ConversationStatusRunning {
			continue
		}
		if _, err = db.ExecContext(ctx, "UPDATE agent_delegations SET stop_requested=1 WHERE turn=?1", run.Turn.String()); err != nil {
			return nil, nil, err
		}
		if run, err = interruptAgentTurn(ctx, db, run.Turn, now); err != nil {
			return nil, nil, err
		}
		stopped = append(stopped, run)
		changed, err := activityEvent(ctx, db, run.Session)
		if err != nil {
			return nil, nil, err
		}
		events = append(events, changed...)
	}
	return stopped, events, nil
}

// ActiveSessionRemoval validates a session removal and lists its turns that
// are still running or stopping.
func ActiveSessionRemoval(ctx context.Context, db *sql.Conn, command protocol.Command) ([]protocol.ConversationRun, error) {
	ids, err := sessionRemovalScope(ctx, db, command)
	if err != nil {
		return nil, err
	}
	return activeRemovalRuns(ctx, db, ids)
}

// discardQueuedRuns ends a removed session's input that was never admitted:
// queued turns, and queued children a stop left for the claim pass to finish.
// They end cancelled outside any queue, so no scheduler can claim them.
func discardQueuedRuns(ctx context.Context, db *sql.Conn, session protocol.SessionID, now time.Time) error {
	runs, err := readBodies[protocol.ConversationRun](ctx, db, "SELECT a.body FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 AND json_extract(a.body,'$.status') IN ('queued','cancelled') AND json_extract(a.body,'$.finished_ms') IS NULL", session.String())
	if err != nil {
		return err
	}
	for _, run := range runs {
		ms := now.UnixMilli()
		run.Status, run.FinishedMs = protocol.ConversationStatusCancelled, &ms
		if _, err = db.ExecContext(ctx, "UPDATE agent_runs SET ready=0 WHERE turn=?1", run.Turn.String()); err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "DELETE FROM agent_pending WHERE turn=?1", run.Turn.String()); err != nil {
			return err
		}
		if err = putAgentRun(ctx, db, run); err != nil {
			return err
		}
	}
	return nil
}

// unreachableTurns lists the turns that still hold checkpoints or attachment
// bindings, whose own session is removed, and that no remaining session lists
// in its history. A fork shares its source's turns, so a removed source's turns
// stay reachable until every fork that lists them is removed or rewound past
// them. A rewound turn of a remaining session keeps its own session and stays.
const unreachableTurns = `SELECT x.turn FROM (SELECT turn FROM file_checkpoints UNION SELECT turn FROM turn_attachments) x
JOIN turns t ON t.id=x.turn JOIN sessions owner ON owner.id=t.session WHERE owner.state='removed'
AND NOT EXISTS(SELECT 1 FROM sessions s WHERE s.state!='removed' AND EXISTS(SELECT 1 FROM conversation_turns h WHERE h.session=s.id AND h.turn=x.turn))`

// pruneRemovedSessions reclaims what only removed sessions can reach: their
// artifacts, the checkpoint text of their turns and the attachments bound to
// those turns. Canonical history stays. It runs in the removing transaction;
// the files follow after the commit.
func pruneRemovedSessions(ctx context.Context, db *sql.Conn) error {
	if err := pruneRemovedArtifacts(ctx, db); err != nil {
		return err
	}
	turns, err := readTexts(ctx, db, unreachableTurns)
	if err != nil {
		return err
	}
	for _, turn := range turns {
		if _, err = db.ExecContext(ctx, "DELETE FROM file_checkpoint_entries WHERE checkpoint IN (SELECT id FROM file_checkpoints WHERE turn=?1)", turn); err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "DELETE FROM file_checkpoints WHERE turn=?1", turn); err != nil {
			return err
		}
		attachments, err := readTexts(ctx, db, "SELECT attachment FROM turn_attachments WHERE turn=?1", turn)
		if err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "DELETE FROM turn_attachments WHERE turn=?1", turn); err != nil {
			return err
		}
		// A replacement turn binds its predecessor's attachments again; only
		// an attachment that loses its last binding here is deleted. Uploads
		// that were never bound belong to a composer and stay.
		for _, id := range attachments {
			if _, err = db.ExecContext(ctx, "DELETE FROM attachments WHERE id=?1 AND NOT EXISTS(SELECT 1 FROM turn_attachments WHERE attachment=?1)", id); err != nil {
				return err
			}
		}
	}
	return nil
}

// pruneRemovedArtifacts deletes each artifact of a removed session unless a
// remaining conversation still shows one of its published revisions.
func pruneRemovedArtifacts(ctx context.Context, db *sql.Conn) error {
	ids, err := readTexts(ctx, db, "SELECT a.id FROM artifacts a JOIN sessions s ON s.id=a.session WHERE s.state='removed'")
	if err != nil {
		return err
	}
	for _, id := range ids {
		revisions, err := readBodies[protocol.ArtifactSummary](ctx, db, "SELECT body FROM artifact_revisions WHERE artifact=?1", id)
		if err != nil {
			return err
		}
		retained := false
		for _, revision := range revisions {
			if retained, err = artifactRetained(ctx, db, revision); err != nil || retained {
				break
			}
		}
		if err != nil {
			return err
		}
		if retained {
			continue
		}
		// The revisions follow through their foreign key.
		if _, err = db.ExecContext(ctx, "DELETE FROM artifacts WHERE id=?1", id); err != nil {
			return err
		}
	}
	return nil
}

func artifactRetained(ctx context.Context, db *sql.Conn, revision protocol.ArtifactSummary) (bool, error) {
	sessions, err := readTexts(ctx, db, "SELECT DISTINCT h.session FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence JOIN sessions s ON s.id=h.session WHERE e.turn=?1 AND s.state!='removed'", revision.Turn.String())
	if err != nil {
		return false, err
	}
	for _, text := range sessions {
		session, err := protocol.ParseSessionID(text)
		if err != nil {
			return false, err
		}
		if published, err := artifactPublished(ctx, db, session, revision); err != nil || published {
			return published, err
		}
	}
	return false, nil
}

// RemovedArtifactOwners lists the removed sessions that keep no artifact; their
// artifact directories hold nothing a conversation can read.
func RemovedArtifactOwners(ctx context.Context, db *sql.Conn) ([]protocol.SessionID, error) {
	ids, err := readTexts(ctx, db, "SELECT s.id FROM sessions s WHERE s.state='removed' AND NOT EXISTS(SELECT 1 FROM artifacts a WHERE a.session=s.id)")
	if err != nil {
		return nil, err
	}
	owners := make([]protocol.SessionID, 0, len(ids))
	for _, text := range ids {
		id, err := protocol.ParseSessionID(text)
		if err != nil {
			return nil, err
		}
		owners = append(owners, id)
	}
	return owners, nil
}

func readTexts(ctx context.Context, db *sql.Conn, query string, args ...any) ([]string, error) {
	rows, err := db.QueryContext(ctx, query, args...)
	if err != nil {
		return nil, err
	}
	var values []string
	for rows.Next() {
		var value string
		if err = rows.Scan(&value); err != nil {
			break
		}
		values = append(values, value)
	}
	return values, errors.Join(err, rows.Err(), rows.Close())
}
