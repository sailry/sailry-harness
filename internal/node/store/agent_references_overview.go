package store

import (
	"context"
	"database/sql"
	"errors"
	"math"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	referenceOverviewTurns      = 8
	referenceOverviewCandidates = 8
	referenceOverviewTextParts  = 8
	referenceOverviewInputBytes = 320
	referenceOverviewReplyBytes = 2048
	referenceOverviewErrorBytes = 256
	referenceOverviewPageBytes  = 16 * 1024
)

type referenceOverviewExcerpt struct {
	EntrySequence *uint64 `json:"entry_sequence,omitempty"`
	Text          string  `json:"text"`
	Truncated     bool    `json:"truncated,omitempty"`
}

type referenceOverviewIntent struct {
	Turn protocol.TurnID `json:"turn"`
	referenceOverviewExcerpt
}

type referenceOverviewReply struct {
	referenceOverviewExcerpt
	Finish             *canonical.Finish `json:"finish,omitempty"`
	OutputLimitReached bool              `json:"output_limit_reached,omitempty"`
}

type referenceOverviewError struct {
	Code      protocol.ErrorCode `json:"code"`
	Message   string             `json:"message"`
	Truncated bool               `json:"truncated,omitempty"`
}

type referenceOverviewTurn struct {
	Turn   protocol.TurnID              `json:"turn"`
	Kind   protocol.ConversationRunKind `json:"kind"`
	Status protocol.ConversationStatus  `json:"status"`
	Joined *protocol.TurnID             `json:"joined,omitempty"`
	User   *referenceOverviewExcerpt    `json:"user,omitempty"`
	Reply  *referenceOverviewReply      `json:"reply,omitempty"`
	Error  *referenceOverviewError      `json:"error,omitempty"`
}

type referenceOverview struct {
	Session              protocol.SessionID       `json:"session"`
	Revision             uint64                   `json:"revision"`
	InitialIntent        *referenceOverviewIntent `json:"initial_intent,omitempty"`
	InitialIntentLimited bool                     `json:"initial_intent_limited,omitempty"`
	Turns                []referenceOverviewTurn  `json:"turns"`
	NextBefore           *protocol.TurnID         `json:"next_before"`
}

func readAgentReferenceOverview(ctx context.Context, db *sql.Conn, session protocol.SessionID, revision uint64, before *protocol.TurnID, limit int) ([]byte, error) {
	boundary := int64(math.MaxInt64)
	if before != nil {
		err := db.QueryRowContext(ctx, "SELECT rowid FROM conversation_turns WHERE session=?1 AND turn=?2", session.String(), before.String()).Scan(&boundary)
		if errors.Is(err, sql.ErrNoRows) {
			_, err = referenceTurnStatus(ctx, db, session, *before)
		}
		if err != nil {
			return nil, err
		}
	}
	members, err := referenceOverviewMembers(ctx, db, session, boundary, limit+1, false)
	if err != nil {
		return nil, err
	}
	value := referenceOverview{Session: session, Revision: revision, Turns: []referenceOverviewTurn{}}
	if before == nil {
		early, err := referenceOverviewMembers(ctx, db, session, boundary, referenceOverviewTurns+1, true)
		if err != nil {
			return nil, err
		}
		for _, turn := range early[:min(len(early), referenceOverviewTurns)] {
			input, err := readReferenceOverviewInput(ctx, db, session, turn)
			if err != nil {
				return nil, err
			}
			if input != nil {
				value.InitialIntent = &referenceOverviewIntent{Turn: turn, referenceOverviewExcerpt: *input}
				break
			}
		}
		value.InitialIntentLimited = value.InitialIntent == nil && len(early) > referenceOverviewTurns
	}
	for _, turn := range members[:min(len(members), limit)] {
		item, err := readReferenceOverviewTurn(ctx, db, session, turn)
		if err != nil {
			return nil, err
		}
		value.Turns = append(value.Turns, item)
	}
	for {
		count := len(value.Turns)
		if count < len(members) {
			value.NextBefore = new(value.Turns[count-1].Turn)
		}
		body, err := encode(value)
		if err != nil || len(body) <= referenceOverviewPageBytes {
			return body, err
		}
		if count <= 1 {
			return nil, invalid("overview metadata exceeds the page budget")
		}
		value.Turns = value.Turns[:count-1]
	}
}

func referenceOverviewMembers(ctx context.Context, db *sql.Conn, session protocol.SessionID, boundary int64, limit int, earliest bool) ([]protocol.TurnID, error) {
	const query = "SELECT turn FROM conversation_turns WHERE session=?1 AND rowid<?2 ORDER BY rowid "
	order := "DESC"
	if earliest {
		order = "ASC"
	}
	rows, err := db.QueryContext(ctx, query+order+" LIMIT ?3", session.String(), boundary, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var turns []protocol.TurnID
	for rows.Next() {
		var text string
		if err = rows.Scan(&text); err != nil {
			return nil, err
		}
		turn, err := protocol.ParseTurnID(text)
		if err != nil {
			return nil, err
		}
		turns = append(turns, turn)
	}
	return turns, rows.Err()
}

func readReferenceOverviewTurn(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID) (referenceOverviewTurn, error) {
	value := referenceOverviewTurn{Turn: turn}
	var joined, code, message sql.NullString
	const query = `SELECT json_extract(body,'$.kind'),json_extract(body,'$.status'),json_extract(body,'$.joined'),
 json_extract(body,'$.error.code'),substr(json_extract(body,'$.error.message'),1,?2) FROM agent_runs WHERE turn=?1`
	if err := db.QueryRowContext(ctx, query, turn.String(), referenceOverviewErrorBytes+1).Scan(&value.Kind, &value.Status, &joined, &code, &message); err != nil {
		return value, err
	}
	if joined.Valid {
		id, err := protocol.ParseTurnID(joined.String)
		if err != nil {
			return value, err
		}
		value.Joined = &id
	}
	if code.Valid {
		value.Error = &referenceOverviewError{Code: protocol.ErrorCode(code.String), Message: message.String}
		value.Error.Truncated = referenceTrim(&value.Error.Message, referenceOverviewErrorBytes)
	}
	var err error
	value.User, err = readReferenceOverviewInput(ctx, db, session, turn)
	if err == nil && !activeRunStatus(value.Status) {
		value.Reply, err = readReferenceOverviewReply(ctx, db, session, turn)
	}
	return value, err
}

const referenceOverviewMessageCandidates = `WITH candidates AS MATERIALIZED (
 SELECT e.sequence FROM agent_events e WHERE e.turn=?2 AND json_extract(e.body,'$.kind')='message'
 AND json_extract(e.body,'$.provenance.author')=?3 ORDER BY e.sequence `

const referenceOverviewMessageRows = ` FROM candidates c CROSS JOIN agent_events e ON e.sequence=c.sequence
 CROSS JOIN conversation_events h ON h.session=?1 AND h.sequence=c.sequence `

func readReferenceOverviewInput(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID) (*referenceOverviewExcerpt, error) {
	const query = referenceOverviewMessageCandidates + `ASC LIMIT ?4)
 SELECT e.sequence,substr(json_extract(p.value,'$.text'),1,?5)` + referenceOverviewMessageRows + `
 CROSS JOIN json_each(e.body,'$.content') p
 WHERE json_extract(e.body,'$.role')='user' AND json_extract(p.value,'$.type')='text'
 AND json_extract(p.value,'$.index')=coalesce(json_extract(e.body,'$.metadata.input.text_part'),0)
 AND length(json_extract(p.value,'$.text'))>0 AND ` + visibleCanonical + ` ORDER BY e.sequence LIMIT 1`
	var text string
	var sequence uint64
	err := db.QueryRowContext(ctx, query, session.String(), turn.String(), "user", referenceOverviewCandidates, referenceOverviewInputBytes+1).Scan(&sequence, &text)
	if err == nil {
		truncated := referenceTrim(&text, referenceOverviewInputBytes)
		return &referenceOverviewExcerpt{EntrySequence: &sequence, Text: text, Truncated: truncated}, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return nil, err
	}
	const fallback = `SELECT substr(json_extract(a.input,'$.text'),1,?2) FROM agent_runs a
 WHERE a.turn=?1 AND json_extract(a.body,'$.kind')='task' AND json_extract(a.body,'$.continuation') IS NULL
 AND coalesce(json_extract(a.body,'$.notice'),0)=0 AND json_type(a.input,'$.text')='text'
 AND NOT EXISTS(SELECT 1 FROM agent_events e WHERE e.turn=?1 AND json_extract(e.body,'$.kind')='message'
 AND json_extract(e.body,'$.provenance.author')='user')`
	err = db.QueryRowContext(ctx, fallback, turn.String(), referenceOverviewInputBytes+1).Scan(&text)
	if errors.Is(err, sql.ErrNoRows) || err == nil && text == "" {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	truncated := referenceTrim(&text, referenceOverviewInputBytes)
	return &referenceOverviewExcerpt{Text: text, Truncated: truncated}, nil
}

func readReferenceOverviewReply(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID) (*referenceOverviewReply, error) {
	const query = referenceOverviewMessageCandidates + `DESC LIMIT ?4)
 SELECT e.sequence,json_extract(e.body,'$.finish'),coalesce(json_extract(e.body,'$.metadata.runtime.output_limit_reached'),0)` + referenceOverviewMessageRows + `
 WHERE json_extract(e.body,'$.role')='assistant' AND ` + visibleCanonical + `
 AND NOT EXISTS(SELECT 1 FROM json_each(e.body,'$.content') p WHERE json_extract(p.value,'$.type') IN ('tool_request','tool_response'))
 AND EXISTS(SELECT 1 FROM json_each(e.body,'$.content') p WHERE json_extract(p.value,'$.type') IN ('text','image','document','file'))
 ORDER BY e.sequence DESC LIMIT 1`
	var value referenceOverviewReply
	var sequence uint64
	var finish sql.NullString
	err := db.QueryRowContext(ctx, query, session.String(), turn.String(), "assistant", referenceOverviewCandidates).Scan(&sequence, &finish, &value.OutputLimitReached)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	value.EntrySequence = &sequence
	if finish.Valid {
		value.Finish = new(canonical.Finish(finish.String))
	}
	const parts = `SELECT substr(json_extract(p.value,'$.text'),1,?2)
 FROM agent_events e CROSS JOIN json_each(e.body,'$.content') p
 WHERE e.sequence=?1 AND json_extract(p.value,'$.type')='text' LIMIT ?3`
	rows, err := db.QueryContext(ctx, parts, sequence, referenceOverviewReplyBytes+1, referenceOverviewTextParts+1)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var text strings.Builder
	for i := 0; rows.Next(); i++ {
		if i == referenceOverviewTextParts {
			value.Truncated = true
			break
		}
		var part string
		if err := rows.Scan(&part); err != nil {
			return nil, err
		}
		if text.Len() > 0 {
			text.WriteByte('\n')
		}
		text.WriteString(part)
		if text.Len() > referenceOverviewReplyBytes {
			value.Truncated = true
			break
		}
	}
	value.Text = referencePrefix(text.String(), referenceOverviewReplyBytes)
	return &value, rows.Err()
}
