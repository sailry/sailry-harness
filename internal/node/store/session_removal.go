package store

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

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
