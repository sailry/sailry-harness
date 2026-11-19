package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"math"
	"strconv"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type SubagentListOptions struct {
	Before *string `json:"before"`
	Limit  *uint16 `json:"limit"`
}

type SubagentPage struct {
	Subagents  []SubagentSummary `json:"subagents"`
	NextBefore *string           `json:"next_before"`
}

type SubagentSummary struct {
	Session    protocol.SessionID          `json:"session"`
	Turn       protocol.TurnID             `json:"turn"`
	Title      string                      `json:"title,omitempty"`
	Role       *subagentRole               `json:"role,omitempty"`
	Status     protocol.ConversationStatus `json:"status"`
	Paused     bool                        `json:"paused,omitempty"`
	StopReason string                      `json:"stop_reason,omitempty"`
}

type subagentRole struct {
	ID   protocol.RoleID `json:"id"`
	Key  string          `json:"key,omitempty"`
	Name string          `json:"name"`
}

// ListSubagents pages by immutable session creation order, not by the execution
// currently selected for that session. Only its founding delegation grants a
// parent access; follow-ups across parent turns do not create new entries.
func ListSubagents(ctx context.Context, db *sql.Conn, parent protocol.TurnID, options SubagentListOptions) (SubagentPage, error) {
	page := SubagentPage{Subagents: []SubagentSummary{}}
	limit := uint16(20)
	if options.Limit != nil {
		limit = *options.Limit
	}
	if limit == 0 || limit > 50 {
		return page, invalid("subagent list limit is invalid")
	}
	upper := int64(math.MaxInt64)
	if options.Before != nil {
		before, err := strconv.ParseInt(*options.Before, 10, 64)
		if err != nil || before < 1 {
			return page, invalid("subagent list cursor is invalid")
		}
		upper = before - 1
	}
	run, err := activeAgentRun(ctx, db, parent)
	if err != nil {
		return page, err
	}
	origin, err := foundingDelegation(ctx, db, run.Session)
	if err != nil {
		return page, err
	}
	if origin != nil {
		return page, fault(protocol.ErrorCodePermissionDenied, "delegated children cannot list subagents")
	}
	capture, err := readTurnCapture(ctx, db, parent)
	if err != nil {
		return page, err
	}
	if !DelegationAvailable(protocol.Session{ID: run.Session}, capture) {
		return page, fault(protocol.ErrorCodePermissionDenied, "delegation is unavailable for the captured conversation configuration")
	}

	rows, err := db.QueryContext(ctx, `WITH children AS (
		SELECT s.id AS session,s.rowid AS sequence,json_extract(d.body,'$.role') AS role,p.id AS parent_turn,
			json_extract(title.body,'$') AS title,coalesce(q.paused,0) AS paused
		FROM turns p JOIN agent_delegations d ON d.parent_turn=p.id JOIN turns t ON t.id=d.turn
		JOIN sessions s ON s.id=t.session LEFT JOIN agent_state title ON title.owner=s.id AND title.key=?3
		LEFT JOIN agent_queues q ON q.session=s.id
		WHERE p.session=?1 AND s.rowid<=?2 AND s.state!='removed' AND json_extract(d.body,'$.session')=?1
		AND d.turn=(SELECT initial.id FROM turns initial JOIN agent_delegations seed ON seed.turn=initial.id
			WHERE initial.session=s.id ORDER BY initial.rowid LIMIT 1)
		ORDER BY s.rowid DESC LIMIT ?4
	)
	SELECT c.session,c.sequence,c.title,c.paused,a.turn,json_extract(a.body,'$.status'),
		coalesce(json_extract(a.body,'$.session')=c.session AND json_extract(a.body,'$.turn')=a.turn
			AND json_extract(origin.body,'$.session')=?1 AND json_extract(origin.body,'$.role') IS c.role,0),
		coalesce(origin.stop_requested,0),c.role,
		CASE WHEN c.role IS NOT NULL THEN
			(SELECT json_object('key',json_extract(r.value,'$.key'),'name',json_extract(r.value,'$.name'))
			 FROM turns p,json_each(p.roles,'$.profiles') r WHERE p.id=c.parent_turn AND json_extract(r.value,'$.id')=c.role LIMIT 1) END
	FROM children c LEFT JOIN agent_runs a ON a.turn=(SELECT t.id FROM turns t JOIN agent_runs a ON a.turn=t.id
		WHERE t.session=c.session AND json_extract(a.body,'$.joined') IS NULL ORDER BY `+childRunOrder+` LIMIT 1)
	LEFT JOIN agent_delegations origin ON origin.turn=a.turn ORDER BY c.sequence DESC`, run.Session.String(), upper, sessionTitleKey, int(limit)+1)
	if err != nil {
		return page, err
	}
	defer rows.Close()
	var last int64
	for rows.Next() {
		if len(page.Subagents) == int(limit) {
			page.NextBefore = new(strconv.FormatInt(last, 10))
			break
		}
		var session string
		var title, turn, status, role sql.NullString
		var paused, owned, stopped bool
		var roleBody []byte
		if err = rows.Scan(&session, &last, &title, &paused, &turn, &status, &owned, &stopped, &role, &roleBody); err != nil {
			return page, err
		}
		if !turn.Valid {
			return page, sql.ErrNoRows
		}
		if !owned {
			return page, fault(protocol.ErrorCodePermissionDenied, "child execution belongs to another owner")
		}
		summary := SubagentSummary{Title: shortTitle(title.String)}
		if summary.Session, err = protocol.ParseSessionID(session); err != nil {
			return page, err
		}
		if summary.Turn, err = protocol.ParseTurnID(turn.String); err != nil {
			return page, err
		}
		if err = summary.Status.UnmarshalText([]byte(status.String)); err != nil {
			return page, err
		}
		summary.Paused = paused && summary.Status == protocol.ConversationStatusQueued
		if role.Valid {
			if roleBody == nil {
				return page, invalid("frozen child role is missing")
			}
			id, err := protocol.ParseRoleID(role.String)
			if err != nil {
				return page, err
			}
			summary.Role = &subagentRole{ID: id}
			if err = json.Unmarshal(roleBody, summary.Role); err != nil {
				return page, err
			}
			summary.Role.Name = shortTitle(summary.Role.Name)
			if len(summary.Role.Key) > 128 {
				summary.Role.Key = ""
			}
		}
		if summary.Status == protocol.ConversationStatusStopping || stopped && (summary.Status == protocol.ConversationStatusInterrupted || summary.Status == protocol.ConversationStatusCancelled) {
			summary.StopReason = "stop_requested"
		}
		page.Subagents = append(page.Subagents, summary)
	}
	return page, rows.Err()
}
