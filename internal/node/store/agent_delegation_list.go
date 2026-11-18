package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
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

	rows, err := db.QueryContext(ctx, `SELECT s.id,s.rowid,d.body,json_extract(title.body,'$'),coalesce(q.paused,0)
		FROM turns p JOIN agent_delegations d ON d.parent_turn=p.id JOIN turns t ON t.id=d.turn
		JOIN sessions s ON s.id=t.session LEFT JOIN agent_state title ON title.owner=s.id AND title.key=?3
		LEFT JOIN agent_queues q ON q.session=s.id
		WHERE p.session=?1 AND s.rowid<=?2 AND s.state!='removed' AND json_extract(d.body,'$.session')=?1
		AND d.turn=(SELECT initial.id FROM turns initial JOIN agent_delegations seed ON seed.turn=initial.id
			WHERE initial.session=s.id ORDER BY initial.rowid LIMIT 1)
		ORDER BY s.rowid DESC LIMIT ?4`, run.Session.String(), upper, sessionTitleKey, int(limit)+1)
	if err != nil {
		return page, err
	}
	type candidate struct {
		session  string
		sequence int64
		origin   []byte
		title    sql.NullString
		paused   bool
	}
	children := make([]candidate, 0, int(limit))
	for rows.Next() {
		if len(children) == int(limit) {
			page.NextBefore = new(strconv.FormatInt(children[len(children)-1].sequence, 10))
			break
		}
		var child candidate
		if err = rows.Scan(&child.session, &child.sequence, &child.origin, &child.title, &child.paused); err != nil {
			return page, errors.Join(err, rows.Close())
		}
		children = append(children, child)
	}
	if err = errors.Join(rows.Err(), rows.Close()); err != nil {
		return page, err
	}

	for _, child := range children {
		session, err := protocol.ParseSessionID(child.session)
		if err != nil {
			return page, err
		}
		current, err := ownedChild(ctx, db, parent, session, nil)
		if err != nil {
			return page, err
		}
		summary := SubagentSummary{Session: session, Turn: current.Turn, Status: current.Status, Title: shortTitle(child.title.String), Paused: child.paused && current.Status == protocol.ConversationStatusQueued}
		var founding protocol.Delegation
		if err = json.Unmarshal(child.origin, &founding); err != nil {
			return page, err
		}
		role, err := roleFromDelegation(ctx, db, founding)
		if err != nil {
			return page, err
		}
		if role != nil {
			summary.Role = &subagentRole{ID: role.ID, Name: shortTitle(role.Name)}
			if len(role.Key) <= 128 {
				summary.Role.Key = role.Key
			}
		}
		page.Subagents = append(page.Subagents, summary)
	}
	return page, markSubagentStops(ctx, db, page.Subagents)
}

// A queue pause is not turn-specific and every cancellation can set it. Only
// expose a stop transition or a successful Stop receipt for this exact turn.
func markSubagentStops(ctx context.Context, db *sql.Conn, entries []SubagentSummary) error {
	const requested = "stop_requested"
	indices := map[string]int{}
	var turns []string
	for i := range entries {
		switch entries[i].Status {
		case protocol.ConversationStatusStopping:
			entries[i].StopReason = requested
		case protocol.ConversationStatusCancelled, protocol.ConversationStatusInterrupted:
			turn := entries[i].Turn.String()
			indices[turn] = i
			turns = append(turns, turn)
		}
	}
	if len(turns) == 0 {
		return nil
	}
	body, err := encode(turns)
	if err != nil {
		return err
	}
	rows, err := db.QueryContext(ctx, `SELECT DISTINCT json_extract(result,'$.Ok.data.turn') FROM requests
		WHERE status='completed' AND json_extract(result,'$.Ok.kind')='run'
		AND json_extract(result,'$.Ok.data.status') IN ('stopping','cancelled')
		AND json_extract(result,'$.Ok.data.turn') IN (SELECT value FROM json_each(?1))
		AND CASE WHEN json_valid(body) THEN json_extract(body,'$.command.kind')='stop_turn'
			AND json_extract(body,'$.command.data.turn')=json_extract(result,'$.Ok.data.turn') END`, string(body))
	if err != nil {
		return err
	}
	for rows.Next() {
		var turn string
		if err = rows.Scan(&turn); err != nil {
			return errors.Join(err, rows.Close())
		}
		entries[indices[turn]].StopReason = requested
	}
	return errors.Join(rows.Err(), rows.Close())
}
