package store

import (
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"math"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type agentDirectoryEntry struct {
	Session         protocol.SessionID           `json:"session"`
	Title           string                       `json:"title"`
	Project         *protocol.ProjectID          `json:"project"`
	Worktree        protocol.WorktreeID          `json:"worktree"`
	Archived        bool                         `json:"archived"`
	CreationOrder   uint64                       `json:"creation_order"`
	HistoryRevision uint64                       `json:"history_revision"`
	Turn            *protocol.TurnID             `json:"turn"`
	Status          *protocol.ConversationStatus `json:"status"`
	Updated         *int64                       `json:"updated_ms"`
}
type agentDirectoryPage struct {
	Node       protocol.NodeID       `json:"node"`
	Project    *protocol.ProjectID   `json:"project"`
	Revision   string                `json:"revision"`
	Sessions   []agentDirectoryEntry `json:"sessions"`
	NextBefore *uint64               `json:"next_before"`
}

func readAgentDirectory(ctx context.Context, db *sql.Conn, node protocol.NodeID, source protocol.Session, options agentReferenceList) ([]byte, error) {
	limit := uint16(20)
	if options.Limit != nil {
		limit = *options.Limit
	}
	if limit == 0 || limit > 100 || options.Text != nil && len(*options.Text) > 512 || options.Before != nil && (*options.Before == 0 || *options.Before > math.MaxInt64) || options.Since != nil && options.Until != nil && *options.Since > *options.Until {
		return nil, invalid("session directory bounds are invalid")
	}
	revision, err := agentDirectoryRevision(ctx, db, source, options.IncludeCurrent)
	if err != nil {
		return nil, err
	}
	if options.Revision != nil && *options.Revision != revision {
		return nil, fault(protocol.ErrorCodeRevisionConflict, "session directory changed")
	}
	if options.Before != nil {
		if err = referenceCursorSession(ctx, db, *options.Before, source.Project); err != nil {
			return nil, err
		}
	}
	var project any
	if source.Project != nil {
		project = source.Project.String()
	}
	before := uint64(math.MaxInt64)
	if options.Before != nil {
		before = *options.Before
	}
	rows, err := db.QueryContext(ctx, "SELECT rowid,id,worktree,state,history_revision FROM sessions WHERE project IS ?1 AND state!='removed' AND rowid<?2 AND (?3 OR id!=?4) ORDER BY rowid DESC LIMIT 257", project, before, options.IncludeCurrent, source.ID.String())
	if err != nil {
		return nil, err
	}
	type candidate struct {
		sequence            uint64
		id, worktree, state string
		revision            uint64
	}
	candidates := []candidate{}
	for rows.Next() {
		var c candidate
		if err = rows.Scan(&c.sequence, &c.id, &c.worktree, &c.state, &c.revision); err != nil {
			rows.Close()
			return nil, err
		}
		candidates = append(candidates, c)
	}
	if err = rows.Err(); err != nil {
		rows.Close()
		return nil, err
	}
	if err = rows.Close(); err != nil {
		return nil, err
	}
	page := agentDirectoryPage{Node: node, Project: source.Project, Revision: revision, Sessions: []agentDirectoryEntry{}}
	var last *uint64
	for i, c := range candidates {
		if i == 256 || len(page.Sessions) == int(limit) {
			page.NextBefore = last
			break
		}
		sequence := c.sequence
		last = &sequence
		id, e := protocol.ParseSessionID(c.id)
		if e != nil {
			return nil, e
		}
		activity, e := readActivity(ctx, db, id)
		if e != nil {
			return nil, e
		}
		var updated *int64
		if activity.Run != nil {
			updated = activity.Run.FinishedMs
			if updated == nil {
				updated = activity.Run.StartedMs
			}
		}
		if options.Text != nil && !strings.Contains(strings.ToLower(activity.Title), strings.ToLower(strings.TrimSpace(*options.Text))) || options.Since != nil && (updated == nil || *updated < *options.Since) || options.Until != nil && (updated == nil || *updated > *options.Until) {
			continue
		}
		worktree, e := protocol.ParseWorktreeID(c.worktree)
		if e != nil {
			return nil, e
		}
		value := agentDirectoryEntry{Session: id, Title: activity.Title, Project: source.Project, Worktree: worktree, Archived: c.state == "archived", CreationOrder: c.sequence, HistoryRevision: c.revision, Updated: updated}
		if activity.Run != nil {
			value.Turn = &activity.Run.Turn
			value.Status = &activity.Run.Status
		}
		page.Sessions = append(page.Sessions, value)
	}
	return encode(page)
}
func agentDirectoryRevision(ctx context.Context, db *sql.Conn, source protocol.Session, include bool) (string, error) {
	var project any
	if source.Project != nil {
		project = source.Project.String()
	}
	rows, err := db.QueryContext(ctx, `SELECT s.rowid,s.id,s.worktree,s.state,s.history_revision,
 (SELECT body FROM agent_state WHERE owner=s.id AND key=?3),
 (SELECT a.body FROM conversation_turns h JOIN turns t ON t.id=h.turn JOIN agent_runs a ON a.turn=t.id
 WHERE h.session=s.id ORDER BY (json_extract(a.body,'$.status') IN ('running','stopping')) DESC,
 (json_extract(a.body,'$.status')='queued'),t.rowid DESC LIMIT 1)
 FROM sessions s WHERE s.project IS ?1 AND (?2 OR s.id!=?4) ORDER BY s.rowid`, project, include, sessionTitleKey, source.ID.String())
	if err != nil {
		return "", err
	}
	defer rows.Close()
	hasher := blake3.New(32, nil)
	for rows.Next() {
		var sequence, revision int64
		var id, worktree, state string
		var title, body []byte
		if err = rows.Scan(&sequence, &id, &worktree, &state, &revision, &title, &body); err != nil {
			return "", err
		}
		var activity any
		if len(body) > 0 {
			var run protocol.ConversationRun
			if err = json.Unmarshal(body, &run); err != nil {
				return "", err
			}
			activity = []any{run.Turn, run.Status, run.StartedMs, run.FinishedMs}
		}
		var bytes any
		if title != nil {
			values := make([]uint64, len(title))
			for i, b := range title {
				values[i] = uint64(b)
			}
			bytes = values
		}
		encoded, e := encode([]any{sequence, id, worktree, state, revision, bytes, activity})
		if e != nil {
			return "", e
		}
		_, _ = hasher.Write(encoded)
	}
	if err = rows.Err(); err != nil {
		return "", err
	}
	return hex.EncodeToString(hasher.Sum(nil)), nil
}
