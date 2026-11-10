package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type AgentInstruction struct {
	Text     string   `json:"text"`
	AnyTools []string `json:"any_tools"`
}
type AgentInvocation struct {
	Hooks                protocol.HooksSettings
	Session              protocol.Session
	Worktree             protocol.Worktree
	Run                  protocol.ConversationRun
	Turn                 protocol.QueuedTurn
	Caller               protocol.NodeID
	Message              protocol.ConversationInput
	Provider             *protocol.ConversationProvider
	Secret               *protocol.Secret
	AuthorizationFailure *protocol.Fault
	Instructions         []AgentInstruction
	CompactionPercent    uint8
}
type AgentState struct {
	Session     protocol.Session
	Records     []canonical.Record
	Context     canonical.Context
	LoadedTools []string
	Reviewed    bool
}
type AgentGuidance struct {
	Turn    protocol.QueuedTurn
	Message protocol.ConversationInput
}

func LoadAgentState(ctx context.Context, db *sql.Conn, session protocol.SessionID) (AgentState, error) {
	value := AgentState{Records: []canonical.Record{}, LoadedTools: []string{}}
	var err error
	value.Session, err = ReadSession(ctx, db, session)
	if err != nil {
		return value, err
	}
	rows, err := db.QueryContext(ctx, "SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 ORDER BY h.sequence", session.String())
	if err != nil {
		return value, err
	}
	for rows.Next() {
		var body []byte
		if err = rows.Scan(&body); err != nil {
			rows.Close()
			return value, err
		}
		var record canonical.Record
		if err = json.Unmarshal(body, &record); err != nil {
			rows.Close()
			return value, err
		}
		value.Records = append(value.Records, record)
	}
	if err = errors.Join(rows.Err(), rows.Close()); err != nil {
		return value, err
	}
	rows, err = db.QueryContext(ctx, "SELECT key,body FROM agent_state WHERE owner=?1", session.String())
	if err != nil {
		return value, err
	}
	value.Context.Guides = map[string]canonical.Guide{}
	for rows.Next() {
		var key string
		var body []byte
		if err = rows.Scan(&key, &body); err != nil {
			rows.Close()
			return value, err
		}
		switch key {
		case sessionTitleKey:
			err = json.Unmarshal(body, &value.Context.Title)
		case "sailry.context.policy":
			err = json.Unmarshal(body, &value.Context.Policy)
		case "sailry.context.environment":
			err = json.Unmarshal(body, &value.Context.Environment)
		case "sailry.context.personal":
			err = json.Unmarshal(body, &value.Context.Personal)
		case "sailry.context.loaded_tools":
			err = json.Unmarshal(body, &value.LoadedTools)
		default:
			if strings.HasPrefix(key, "sailry.context.guide:") {
				var guide canonical.Guide
				err = json.Unmarshal(body, &guide)
				value.Context.Guides[key] = guide
			}
		}
		if err != nil {
			rows.Close()
			return value, err
		}
	}
	err = errors.Join(rows.Err(), rows.Close())
	value.Context.LoadedTools = &value.LoadedTools
	for _, record := range value.Records {
		if record.Metadata.Context != nil && record.Metadata.Context.Captured {
			value.Context.Captured = true
		}
		if record.Kind == "configuration" {
			if c, ok := record.Data.(*canonical.Configuration); ok && c.Context.Captured {
				value.Context.Captured = true
			}
		}
	}
	return value, err
}

func activityEvent(ctx context.Context, db *sql.Conn, session protocol.SessionID) ([]protocol.Event, error) {
	summary, err := readActivity(ctx, db, session)
	if err != nil {
		return nil, err
	}
	return []protocol.Event{{Kind: "conversation_changed", Data: protocol.EventConversationChangedPayload{Session: session, Activity: &summary}}}, nil
}

// AppendCanonical preserves immutable record identity and updates only derived
// membership and materialized state in the caller's transaction.
func AppendCanonical(ctx context.Context, db *sql.Conn, session protocol.SessionID, record canonical.Record) (protocol.ConversationEntry, []protocol.Event, error) {
	if err := prepareCanonicalRecord(ctx, db, &record); err != nil {
		return protocol.ConversationEntry{}, nil, err
	}
	if err := record.Validate(); err != nil {
		return protocol.ConversationEntry{}, nil, invalid(err.Error())
	}
	if err := validateCanonicalAssets(ctx, db, record); err != nil {
		return protocol.ConversationEntry{}, nil, err
	}
	if len(record.ID) > 4096 {
		return protocol.ConversationEntry{}, nil, invalid("event identifier is invalid")
	}
	var owner string
	if err := db.QueryRowContext(ctx, "SELECT session FROM turns WHERE id=?1", record.Turn.String()).Scan(&owner); err != nil {
		return protocol.ConversationEntry{}, nil, err
	}
	if owner != session.String() {
		return protocol.ConversationEntry{}, nil, invalid("event turn belongs to another session")
	}
	body, err := encode(record)
	if err != nil {
		return protocol.ConversationEntry{}, nil, err
	}
	if len(body) > protocol.MAX_FRAME_BYTES {
		return protocol.ConversationEntry{}, nil, invalid("event exceeds the history frame limit")
	}
	var previous []byte
	var sequence uint64
	err = db.QueryRowContext(ctx, "SELECT sequence,body FROM agent_events WHERE session=?1 AND id=?2", session.String(), record.ID).Scan(&sequence, &previous)
	if err == nil {
		var a, b any
		if json.Unmarshal(previous, &a) != nil || json.Unmarshal(body, &b) != nil {
			return protocol.ConversationEntry{}, nil, invalid("invalid stored conversation record")
		}
		aa, _ := json.Marshal(a)
		bb, _ := json.Marshal(b)
		if !bytes.Equal(aa, bb) {
			return protocol.ConversationEntry{}, nil, fault(protocol.ErrorCodeConflict, "event identifier already has different content")
		}
		entry, err := projectStoredEntry(ctx, db, record, sequence)
		return entry, nil, err
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return protocol.ConversationEntry{}, nil, err
	}
	if record.Kind == "compaction" {
		c := record.Data.(*canonical.Compaction)
		var present bool
		if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND e.id=?2)", session.String(), c.Boundary).Scan(&present); err != nil {
			return protocol.ConversationEntry{}, nil, err
		}
		if !present {
			return protocol.ConversationEntry{}, nil, invalid("compaction boundary does not belong to this session")
		}
	}
	sequence, err = NextCounter(ctx, db, CounterAgentEvents)
	if err != nil {
		return protocol.ConversationEntry{}, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_events(sequence,session,id,turn,body) VALUES(?1,?2,?3,?4,?5)", sequence, session.String(), record.ID, record.Turn.String(), body); err != nil {
		return protocol.ConversationEntry{}, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO conversation_events(session,sequence) VALUES(?1,?2)", session.String(), sequence); err != nil {
		return protocol.ConversationEntry{}, nil, err
	}
	if err = applyCanonicalState(ctx, db, session, record); err != nil {
		return protocol.ConversationEntry{}, nil, err
	}
	if record.Provenance.Author == "user" {
		if _, err = db.ExecContext(ctx, "DELETE FROM agent_pending WHERE turn=?1", record.Turn.String()); err != nil {
			return protocol.ConversationEntry{}, nil, err
		}
		if err = bumpQueue(ctx, db, session); err != nil {
			return protocol.ConversationEntry{}, nil, err
		}
	}
	entry, err := projectStoredEntry(ctx, db, record, sequence)
	if err != nil {
		return entry, nil, err
	}
	events, err := activityEvent(ctx, db, session)
	return entry, events, err
}

func applyCanonicalState(ctx context.Context, db *sql.Conn, session protocol.SessionID, record canonical.Record) error {
	context := record.Metadata.Context
	if record.Kind == "configuration" {
		if c, ok := record.Data.(*canonical.Configuration); ok {
			context = &c.Context
		}
	}
	changes := map[string]json.RawMessage{}
	put := func(k string, v any) {
		if v == nil {
			return
		}
		b, _ := json.Marshal(v)
		changes[k] = b
	}
	if context != nil {
		if context.Title != nil {
			put(sessionTitleKey, *context.Title)
		}
		if context.Policy != nil {
			put("sailry.context.policy", *context.Policy)
		}
		if context.Environment != nil {
			put("sailry.context.environment", *context.Environment)
		}
		if context.Personal != nil {
			put("sailry.context.personal", *context.Personal)
		}
		for k, v := range context.Guides {
			put(k, v)
		}
	}
	if state := record.Metadata.State; state != nil {
		for k, v := range state.Session {
			changes[k] = v
		}
		for k, v := range state.Application {
			changes["app:"+k] = v
		}
		for k, v := range state.User {
			changes["user:"+k] = v
		}
	}
	for key, body := range changes {
		if strings.HasPrefix(key, "temp:") || key == "sailry.context.loaded_tools" {
			continue
		}
		owner := session.String()
		if strings.HasPrefix(key, "app:") || strings.HasPrefix(key, "user:") {
			owner = ""
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO agent_state(owner,key,body) VALUES(?1,?2,?3) ON CONFLICT(owner,key) DO UPDATE SET body=excluded.body", owner, key, []byte(body)); err != nil {
			return err
		}
	}
	var loaded []string
	if value, err := readBody[[]string](ctx, db, "SELECT body FROM agent_state WHERE owner=?1 AND key='sailry.context.loaded_tools'", session.String()); err != nil {
		return err
	} else if value != nil {
		loaded = *value
	}
	if context != nil && context.LoadedTools != nil {
		if record.Kind == "compaction" {
			loaded = nil
		}
		for _, name := range *context.LoadedTools {
			if !slices.Contains(loaded, name) {
				loaded = append(loaded, name)
			}
		}
	}
	if message := record.Message(); message != nil {
		for _, block := range message.Content {
			if call, ok := block.Data.(*canonical.ToolRequest); ok && !slices.Contains(loaded, call.Name) {
				loaded = append(loaded, call.Name)
			}
		}
	}
	if len(loaded) > 0 || context != nil && context.LoadedTools != nil {
		if loaded == nil {
			loaded = []string{}
		}
		body, err := encode(loaded)
		if err != nil {
			return err
		}
		_, err = db.ExecContext(ctx, "INSERT INTO agent_state(owner,key,body) VALUES(?1,'sailry.context.loaded_tools',?2) ON CONFLICT(owner,key) DO UPDATE SET body=excluded.body", session.String(), body)
		return err
	}
	return nil
}

func rebuildCanonicalState(ctx context.Context, db *sql.Conn, session protocol.SessionID) error {
	var named []byte
	err := db.QueryRowContext(ctx, "SELECT body FROM agent_state WHERE owner=?1 AND key=?2", session.String(), sessionTitleKey).Scan(&named)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return err
	}
	if _, err = db.ExecContext(ctx, "DELETE FROM agent_state WHERE owner=?1 AND key!=?2", session.String(), sessionTitleKey); err != nil {
		return err
	}
	records, err := readBodies[canonical.Record](ctx, db, "SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 ORDER BY h.sequence", session.String())
	if err != nil {
		return err
	}
	for _, record := range records {
		if state := record.Metadata.State; state != nil {
			copy := *state
			copy.Application = nil
			copy.User = nil
			record.Metadata.State = &copy
		}
		if err = applyCanonicalState(ctx, db, session, record); err != nil {
			return err
		}
	}
	if named != nil {
		_, err = db.ExecContext(ctx, "INSERT INTO agent_state(owner,key,body) VALUES(?1,?2,?3) ON CONFLICT(owner,key) DO UPDATE SET body=excluded.body", session.String(), sessionTitleKey, named)
	}
	return err
}
