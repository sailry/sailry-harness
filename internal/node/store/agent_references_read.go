package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func readAgentReferenceHistory(ctx context.Context, db *sql.Conn, options agentReferenceRead) ([]byte, error) {
	limit := uint64(5)
	if options.Limit != nil {
		limit = *options.Limit
	}
	if limit == 0 || limit > 9007199254740991 {
		return nil, invalid("history limit must be a positive safe integer")
	}
	mode := "detail"
	if options.Mode != nil {
		mode = *options.Mode
	}
	switch mode {
	case "overview":
		if options.Turn != nil || options.BeforeEntry != nil || options.EntrySequence != nil {
			return nil, invalid("overview reads use a turn page cursor")
		}
		revision, err := readReferenceSnapshotRevision(ctx, db, options.Session, options.Revision, options.Before != nil)
		if err != nil {
			return nil, err
		}
		return readAgentReferenceOverview(ctx, db, options.Session, revision, options.Before, int(min(limit, referenceOverviewTurns)))
	case "usage":
		if options.Turn == nil || options.Before != nil || options.EntrySequence != nil {
			return nil, invalid("usage reads require one turn and use before_entry for older requests")
		}
		if options.Limit == nil {
			limit = referenceUsageDefaultLimit
		}
		revision, err := readReferenceSnapshotRevision(ctx, db, options.Session, options.Revision, options.BeforeEntry != nil)
		if err != nil {
			return nil, err
		}
		return readAgentReferenceUsage(ctx, db, options.Session, *options.Turn, revision, options.BeforeEntry, int(min(limit, usageRequestPage)))
	case "detail":
	default:
		return nil, invalid("unknown session history mode")
	}
	if err := checkReferenceRevision(ctx, db, options.Session, options.Revision, options.Before != nil || options.Turn != nil); err != nil {
		return nil, err
	}
	if options.EntrySequence != nil {
		if options.Turn == nil || options.Before != nil || options.BeforeEntry != nil || options.Limit != nil || *options.EntrySequence == 0 || *options.EntrySequence > math.MaxInt64 {
			return nil, invalid("entry lookup requires a turn, revision and a positive entry_sequence, without cursors or a limit")
		}
		return readAgentReferenceEntry(ctx, db, options.Session, *options.Turn, *options.Revision, *options.EntrySequence)
	}
	if options.Turn != nil {
		if options.Before != nil {
			return nil, invalid("turn reads use before_entry")
		}
		history, err := ReadConversationTurn(ctx, db, options.Session, *options.Turn, *options.Revision, options.BeforeEntry, uint16(min(limit, 100)))
		if err != nil {
			return nil, err
		}
		return boundedReferenceTurn(ctx, db, options.Session, history)
	}
	if options.BeforeEntry != nil {
		return nil, invalid("entry cursor requires a turn")
	}
	history, err := ReadConversation(ctx, db, options.Session, options.Before, uint16(min(limit, 100)))
	if err != nil {
		return nil, err
	}
	return boundedReferenceConversation(ctx, db, options.Session, history)
}

func readReferenceSnapshotRevision(ctx context.Context, db *sql.Conn, session protocol.SessionID, expected *uint64, required bool) (uint64, error) {
	if required && expected == nil {
		return 0, invalid("history continuations require a revision")
	}
	actual, err := historyRevision(ctx, db, session)
	if err == nil && expected != nil {
		err = checkRevision(actual, *expected)
	}
	return actual, err
}

func referenceTurnStatus(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID) (protocol.ConversationStatus, error) {
	var status protocol.ConversationStatus
	var visible bool
	err := db.QueryRowContext(ctx, `SELECT json_extract(body,'$.status'),
 EXISTS(SELECT 1 FROM conversation_turns WHERE session=?1 AND turn=?2) FROM agent_runs WHERE turn=?2`, session.String(), turn.String()).Scan(&status, &visible)
	if errors.Is(err, sql.ErrNoRows) {
		return status, fault(protocol.ErrorCodeNotFound, "turn does not exist")
	}
	if err == nil && !visible {
		err = fault(protocol.ErrorCodeWrongTarget, "turn is not in this conversation")
	}
	return status, err
}

type referenceEntry struct {
	Revision uint64         `json:"revision"`
	Entry    map[string]any `json:"entry"`
	referenceTruncation
}

func readAgentReferenceEntry(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID, revision, sequence uint64) ([]byte, error) {
	if _, err := referenceTurnStatus(ctx, db, session, turn); err != nil {
		return nil, err
	}
	var body []byte
	const query = "SELECT " + historyEntryBody + ` FROM agent_events e
 CROSS JOIN conversation_events h ON h.sequence=e.sequence AND h.session=?1
 WHERE e.turn=?2 AND e.sequence=?3 AND ` + visibleCanonical
	err := db.QueryRowContext(ctx, query, session.String(), turn.String(), sequence).Scan(&body)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, fault(protocol.ErrorCodeNotFound, "public entry is not in this turn")
	}
	if err != nil {
		return nil, err
	}
	var record canonical.Record
	if err = json.Unmarshal(body, &record); err != nil {
		return nil, err
	}
	entry, err := projectStoredEntry(ctx, db, record, sequence, nil)
	if err != nil {
		return nil, err
	}
	value := referenceEntry{Revision: revision, referenceTruncation: newReferenceTruncation()}
	if value.Entry, err = boundedReferenceEntry(ctx, db, session, entry, &value.referenceTruncation); err != nil {
		return nil, err
	}
	return encode(value)
}
