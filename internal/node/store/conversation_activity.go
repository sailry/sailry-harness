package store

import (
	"context"
	"database/sql"
	"errors"
	"math"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type ConversationActivity struct {
	Runs       []protocol.ConversationRun
	Queue      protocol.ConversationQueue
	Approvals  []protocol.ConversationApproval
	Questions  []protocol.ConversationQuestion
	Children   []protocol.ConversationChild
	Background []protocol.ConversationBackgroundTask
}

// ReadConversationActivity reads the live head's independently published
// metadata on the caller's pinned snapshot. Its anchors follow the stored head
// bounds, not the transport budget of a page containing entry bodies.
func ReadConversationActivity(ctx context.Context, db *sql.Conn, session protocol.SessionID, limit uint16, changedChildren ...protocol.TurnID) (ConversationActivity, error) {
	var value ConversationActivity
	if limit == 0 {
		return value, invalid("history limit must be positive")
	}
	var err error
	if value.Queue, err = ReadQueue(ctx, db, session); err != nil {
		return value, err
	}
	runs, _, err := historyRuns(ctx, db, session, nil, min(limit, 100))
	if err != nil {
		return value, err
	}
	anchors, err := conversationActivityAnchors(ctx, db, session, runs)
	if err != nil {
		return value, err
	}
	ids := make([]string, len(anchors))
	calls := make([][2]string, len(anchors))
	if len(anchors) > 0 {
		owners := historyOwners(runs)
		oldest := uint64(math.MaxUint64)
		for i, anchor := range anchors {
			ids[i] = anchor.id
			calls[i] = [2]string{anchor.turn.String(), anchor.id}
			oldest = min(oldest, owners[anchor.turn].Sequence)
		}
		runs = pruneHistoryRuns(runs, owners, oldest)
	}
	if value.Approvals, value.Questions, err = readConversationInteractions(ctx, db, session, ids); err != nil {
		return value, err
	}
	if value.Children, err = conversationChildrenAt(ctx, db, calls); err != nil {
		return value, err
	}
	if value.Background, err = ReadBackgroundTasks(ctx, db, session); err != nil {
		return value, err
	}
	for i := range runs {
		if runs[i], err = hydratePublicRun(ctx, db, runs[i]); err != nil {
			return value, err
		}
	}
	value.Runs = runs
	for _, turn := range changedChildren {
		if slices.ContainsFunc(value.Children, func(child protocol.ConversationChild) bool { return child.Run.Turn == turn }) {
			continue
		}
		child, found, err := ReadConversationChild(ctx, db, session, turn)
		if err != nil {
			return value, err
		}
		if found {
			value.Children = append(value.Children, child)
		}
	}
	return value, nil
}

type conversationActivityAnchor struct {
	id       string
	turn     protocol.TurnID
	sequence uint64
}

func conversationActivityAnchors(ctx context.Context, db *sql.Conn, session protocol.SessionID, runs []protocol.ConversationRun) ([]conversationActivityAnchor, error) {
	if len(runs) == 0 {
		return nil, nil
	}
	const columns = "SELECT e.id,e.turn,e.sequence," + historyStoredBytes
	query := columns + historyTurnRows
	var selector string
	if len(runs) == 1 {
		selector = runs[0].Turn.String()
	} else {
		query = columns + historyHeadRows
		turns := make([]string, len(runs))
		for i, run := range runs {
			turns[i] = run.Turn.String()
		}
		body, err := encode(turns)
		if err != nil {
			return nil, err
		}
		selector = string(body)
	}
	rows, err := db.QueryContext(ctx, query, session.String(), selector, int64(math.MaxInt64), historyEntryLimit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	anchors := make([]conversationActivityAnchor, 0, historyEntryLimit)
	size := 0
	for rows.Next() {
		var anchor conversationActivityAnchor
		var turn string
		var storedBytes int
		if err = rows.Scan(&anchor.id, &turn, &anchor.sequence, &storedBytes); err != nil {
			return nil, err
		}
		if len(anchors) > 0 && size+storedBytes > historyEntryByteLimit {
			break
		}
		if anchor.turn, err = protocol.ParseTurnID(turn); err != nil {
			return nil, err
		}
		anchors = append(anchors, anchor)
		size += storedBytes
	}
	return anchors, errors.Join(rows.Err(), rows.Close())
}
