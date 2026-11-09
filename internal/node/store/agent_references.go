package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type AgentHistorySource struct {
	Caller  protocol.NodeID
	Session protocol.SessionID
	Turn    protocol.TurnID
}
type agentReferenceList struct {
	Text           *string `json:"text"`
	Since          *int64  `json:"since_ms"`
	Until          *int64  `json:"until_ms"`
	Before         *uint64 `json:"before"`
	Revision       *string `json:"revision"`
	IncludeCurrent bool    `json:"include_current"`
	Limit          *uint16 `json:"limit"`
}
type agentReferenceRead struct {
	Session     protocol.SessionID `json:"session"`
	Before      *protocol.TurnID   `json:"before"`
	Turn        *protocol.TurnID   `json:"turn"`
	Revision    *uint64            `json:"revision"`
	BeforeEntry *uint64            `json:"before_entry"`
	Limit       *uint64            `json:"limit"`
}
type agentReferenceSearch struct {
	Session       protocol.SessionID `json:"session"`
	Text          string             `json:"text"`
	CaseSensitive bool               `json:"case_sensitive"`
	Before        *uint64            `json:"before"`
	Revision      *uint64            `json:"revision"`
	Limit         *uint16            `json:"limit"`
}

func ReadAgentReference(ctx context.Context, db *sql.Conn, node protocol.NodeID, source AgentHistorySource, kind string, arguments json.RawMessage) (json.RawMessage, error) {
	run, err := activeAgentRun(ctx, db, source.Turn)
	if err != nil {
		return nil, err
	}
	var caller []byte
	if err = db.QueryRowContext(ctx, "SELECT caller FROM turns WHERE id=?1", source.Turn.String()).Scan(&caller); err != nil {
		return nil, err
	}
	if run.Session != source.Session || string(caller) != string(source.Caller[:]) {
		return nil, fault(protocol.ErrorCodePermissionDenied, "history read does not belong to this invocation")
	}
	session, err := ReadSession(ctx, db, source.Session)
	if err != nil {
		return nil, err
	}
	visible := func(target protocol.SessionID) error {
		value, e := ReadSession(ctx, db, target)
		if e != nil {
			return e
		}
		if !sameProject(value.Project, session.Project) {
			return fault(protocol.ErrorCodeWrongTarget, "conversation belongs to another project")
		}
		return nil
	}
	switch kind {
	case "list_sessions":
		var options agentReferenceList
		if err = protocol.DecodeObject(arguments, &options, []string{}, true); err != nil {
			return nil, invalid("invalid session directory arguments")
		}
		return readAgentDirectory(ctx, db, node, session, options)
	case "read_session":
		var options agentReferenceRead
		if err = protocol.DecodeObject(arguments, &options, []string{"session"}, true); err != nil {
			return nil, invalid("invalid session history arguments")
		}
		if err = visible(options.Session); err != nil {
			return nil, err
		}
		limit := uint64(5)
		if options.Limit != nil {
			limit = *options.Limit
		}
		if limit == 0 || limit > 9007199254740991 {
			return nil, invalid("history limit must be a positive safe integer")
		}
		if err = checkReferenceRevision(ctx, db, options.Session, options.Revision, options.Before != nil || options.Turn != nil); err != nil {
			return nil, err
		}
		if options.Turn != nil {
			if options.Before != nil {
				return nil, invalid("turn reads use before_entry")
			}
			history, e := ReadConversationTurn(ctx, db, options.Session, *options.Turn, *options.Revision, options.BeforeEntry, uint16(min(limit, 100)))
			if e != nil {
				return nil, e
			}
			return boundedReferenceTurn(ctx, db, options.Session, history)
		}
		if options.BeforeEntry != nil {
			return nil, invalid("entry cursor requires a turn")
		}
		history, e := ReadConversation(ctx, db, options.Session, options.Before, uint16(min(limit, 100)))
		if e != nil {
			return nil, e
		}
		return boundedReferenceConversation(ctx, db, options.Session, history)
	case "search_session":
		var options agentReferenceSearch
		if err = protocol.DecodeObject(arguments, &options, []string{"session", "text"}, true); err != nil {
			return nil, invalid("invalid session search arguments")
		}
		if err = visible(options.Session); err != nil {
			return nil, err
		}
		if err = checkReferenceRevision(ctx, db, options.Session, options.Revision, options.Before != nil); err != nil {
			return nil, err
		}
		limit := uint16(20)
		if options.Limit != nil {
			limit = *options.Limit
		}
		page, e := searchAgentConversation(ctx, db, options.Session, protocol.ConversationSearchQuery{Text: options.Text, CaseSensitive: options.CaseSensitive, Before: options.Before, Limit: limit})
		if e != nil {
			return nil, e
		}
		return encode(page)
	}
	return nil, invalid("unknown session history query")
}
func sameProject(a, b *protocol.ProjectID) bool {
	return a == nil && b == nil || a != nil && b != nil && *a == *b
}
func checkReferenceRevision(ctx context.Context, db *sql.Conn, session protocol.SessionID, expected *uint64, required bool) error {
	if required && expected == nil {
		return invalid("history continuations require a revision")
	}
	if expected != nil {
		actual, err := historyRevision(ctx, db, session)
		if err != nil {
			return err
		}
		return checkRevision(actual, *expected)
	}
	return nil
}
func referenceCursorSession(ctx context.Context, db *sql.Conn, before uint64, project *protocol.ProjectID) error {
	if before == 0 || before > math.MaxInt64 {
		return invalid("session directory bounds are invalid")
	}
	var text string
	if err := db.QueryRowContext(ctx, "SELECT id FROM sessions WHERE rowid=?1 AND state!='removed'", before).Scan(&text); errors.Is(err, sql.ErrNoRows) {
		return fault(protocol.ErrorCodeNotFound, "session cursor is unavailable")
	} else if err != nil {
		return err
	}
	id, err := protocol.ParseSessionID(text)
	if err != nil {
		return err
	}
	session, err := ReadSession(ctx, db, id)
	if err != nil {
		return err
	}
	if !sameProject(session.Project, project) {
		return fault(protocol.ErrorCodeWrongTarget, "conversation belongs to another project")
	}
	return nil
}
