package store

import (
	"cmp"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type conversationGroup struct {
	run   protocol.ConversationRun
	turns []protocol.TurnID
	first uint64
}

// conversationGroups follows recorded execution, not queue admission. Consumed
// guidance belongs to its recipient even when it was admitted after other tasks.
// Runs without a canonical record use their start/finish times; recorded groups
// retain canonical order even when wall-clock timestamps tie or move backwards.
func conversationGroups(ctx context.Context, db *sql.Conn, session protocol.SessionID) ([]conversationGroup, error) {
	rows, err := db.QueryContext(ctx, `SELECT a.body,coalesce((SELECT min(e.sequence) FROM agent_events e
JOIN conversation_events visible ON visible.sequence=e.sequence AND visible.session=?1 WHERE e.turn=a.turn),0)
FROM conversation_turns h JOIN agent_runs a ON a.turn=h.turn WHERE h.session=?1 AND json_extract(a.body,'$.status')!='queued'`, session.String())
	if err != nil {
		return nil, err
	}
	var values []conversationGroup
	positions := map[protocol.TurnID]int{}
	for rows.Next() {
		var body []byte
		var value conversationGroup
		if err = rows.Scan(&body, &value.first); err != nil {
			rows.Close()
			return nil, err
		}
		if err = json.Unmarshal(body, &value.run); err != nil {
			rows.Close()
			return nil, err
		}
		positions[value.run.Turn] = len(values)
		values = append(values, value)
	}
	if err = errors.Join(rows.Err(), rows.Close()); err != nil {
		return nil, err
	}
	groups := make([]conversationGroup, 0, len(values))
	owners := make(map[protocol.TurnID]int, len(values))
	for _, value := range values {
		if value.run.Joined != nil {
			if _, present := positions[*value.run.Joined]; present {
				continue
			}
		}
		owners[value.run.Turn] = len(groups)
		groups = append(groups, conversationGroup{run: value.run, first: value.first})
	}
	for _, value := range values {
		owner := value.run.Turn
		if value.run.Joined != nil {
			if _, present := owners[*value.run.Joined]; present {
				owner = *value.run.Joined
			}
		}
		index, present := owners[owner]
		if !present {
			return nil, invalid("conversation guidance has no execution owner")
		}
		group := &groups[index]
		group.turns = append(group.turns, value.run.Turn)
		if value.first != 0 && (group.first == 0 || value.first < group.first) {
			group.first = value.first
		}
	}
	slices.SortFunc(groups, func(a, b conversationGroup) int {
		if activeRunStatus(a.run.Status) != activeRunStatus(b.run.Status) {
			if activeRunStatus(a.run.Status) {
				return 1
			}
			return -1
		}
		x, y := a.run.StartedMs, b.run.StartedMs
		if x == nil {
			x = a.run.FinishedMs
		}
		if y == nil {
			y = b.run.FinishedMs
		}
		if x == nil && y != nil {
			return 1
		}
		if x != nil && y == nil {
			return -1
		}
		if x != nil && y != nil {
			if order := cmp.Compare(*x, *y); order != 0 {
				return order
			}
		}
		return cmp.Compare(a.run.Sequence, b.run.Sequence)
	})
	var recorded []conversationGroup
	for _, group := range groups {
		if group.first != 0 {
			recorded = append(recorded, group)
		}
	}
	slices.SortFunc(recorded, func(a, b conversationGroup) int { return cmp.Compare(a.first, b.first) })
	index := 0
	for i := range groups {
		if groups[i].first != 0 {
			groups[i] = recorded[index]
			index++
		}
	}
	return groups, nil
}

func conversationGroupIndex(groups []conversationGroup, turn protocol.TurnID) int {
	return slices.IndexFunc(groups, func(group conversationGroup) bool { return slices.Contains(group.turns, turn) })
}

func conversationGroupTurns(groups []conversationGroup) []protocol.TurnID {
	turns := []protocol.TurnID{}
	for _, group := range groups {
		turns = append(turns, group.turns...)
	}
	return turns
}
