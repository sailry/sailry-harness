package store

import (
	"context"
	"database/sql"
	"encoding/base64"
	"encoding/json"
	"errors"
	"math"
	"regexp"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func HydrateCanonicalPayload(ctx context.Context, db *sql.Conn, payload canonical.Payload) (json.RawMessage, error) {
	if len(payload.Assets) == 0 {
		return payload.Value, nil
	}
	value := agentJSON(payload.Value)
	for _, slot := range payload.Assets {
		body, err := GetBlob(ctx, db, slot.Asset.Digest)
		if err != nil {
			return nil, err
		}
		var replacement any
		switch slot.Encoding {
		case canonical.Base64:
			replacement = base64.StdEncoding.EncodeToString(body)
		case canonical.Bytes:
			items := make([]uint64, len(body))
			for i, b := range body {
				items[i] = uint64(b)
			}
			replacement = items
		default:
			return nil, invalid("binary slot encoding is invalid")
		}
		if slot.Pointer == "" {
			value = replacement
			continue
		}
		if !strings.HasPrefix(slot.Pointer, "/") {
			return nil, invalid("binary slot pointer is invalid")
		}
		parts := strings.Split(slot.Pointer[1:], "/")
		for i, p := range parts {
			parts[i] = strings.ReplaceAll(strings.ReplaceAll(p, "~1", "/"), "~0", "~")
		}
		if err = replaceJSONSlot(value, parts, replacement); err != nil {
			return nil, err
		}
	}
	return encode(value)
}
func replaceJSONSlot(value any, parts []string, replacement any) error {
	if len(parts) == 0 {
		return invalid("binary slot pointer is invalid")
	}
	key := parts[0]
	switch object := value.(type) {
	case map[string]any:
		if _, ok := object[key]; !ok {
			return invalid("binary slot does not exist")
		}
		if len(parts) == 1 {
			object[key] = replacement
			return nil
		}
		return replaceJSONSlot(object[key], parts[1:], replacement)
	case []any:
		index, err := strconv.Atoi(key)
		if err != nil || index < 0 || index >= len(object) {
			return invalid("binary slot does not exist")
		}
		if len(parts) == 1 {
			object[index] = replacement
			return nil
		}
		return replaceJSONSlot(object[index], parts[1:], replacement)
	}
	return invalid("binary slot does not exist")
}
func utf8TextPage(text string, offset, limit uint64) (string, *uint64, error) {
	if offset > uint64(len(text)) || limit == 0 {
		return "", nil, invalid("output page is invalid")
	}
	if offset < uint64(len(text)) && !utf8.RuneStart(text[offset]) {
		return "", nil, invalid("output page offset splits a UTF-8 character")
	}
	end := uint64(len(text))
	if limit < end-offset {
		end = offset + limit
	}
	for end > offset && end < uint64(len(text)) && !utf8.RuneStart(text[end]) {
		end--
	}
	if end == offset && end < uint64(len(text)) {
		_, width := utf8.DecodeRuneInString(text[offset:])
		end += uint64(width)
	}
	var next *uint64
	if end < uint64(len(text)) {
		next = &end
	}
	return text[offset:end], next, nil
}
func readAgentToolDetail(ctx context.Context, db *sql.Conn, input protocol.CommandReadToolDetailPayload) (protocol.ToolDetailPage, error) {
	value := protocol.DefaultToolDetailPage()
	value.Reference = protocol.ToolDetailReference{Session: input.Session, Turn: input.Turn, Entry: input.Entry, Index: input.Index, Call: input.Call, Revision: input.ExpectedRevision}
	if input.Entry == "" || len(input.Entry) > 4096 {
		return value, invalid("tool detail entry is invalid")
	}
	if input.Limit == 0 || input.Limit > 256*1024 || len(input.ExpectedRevision) != 64 || !agentHex(input.ExpectedRevision) || input.Call != nil && (*input.Call == "" || len(*input.Call) > 4096) {
		return value, invalid("tool detail page is invalid")
	}
	if _, err := visibleAgentRun(ctx, db, input.Session, input.Turn); err != nil {
		return value, err
	}
	var body []byte
	var seq uint64
	err := db.QueryRowContext(ctx, "SELECT e.sequence,e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND e.turn=?2 AND e.id=?3", input.Session.String(), input.Turn.String(), input.Entry).Scan(&seq, &body)
	if errors.Is(err, sql.ErrNoRows) {
		return value, fault(protocol.ErrorCodeNotFound, "tool detail does not exist")
	}
	if err != nil {
		return value, err
	}
	var record canonical.Record
	if err = json.Unmarshal(body, &record); err != nil {
		return value, err
	}
	message := record.Message()
	raw, err := reviewRaw(record, input.Index)
	if err != nil {
		return value, err
	}
	if message != nil {
		for _, block := range message.Content {
			if block.Index == input.Index {
				raw, err = storedAgentTool(ctx, db, record, seq, block)
				if err != nil {
					return value, err
				}
				break
			}
		}
	}
	if raw == nil || !equalString(raw.ID, input.Call) {
		return value, fault(protocol.ErrorCodeNotFound, "tool detail call does not match")
	}

	revision, err := raw.revision()
	if err != nil {
		return value, err
	}
	if revision != input.ExpectedRevision {
		return value, fault(protocol.ErrorCodeRevisionConflict, "tool detail changed")
	}
	detail := protocol.DefaultToolDetail()
	detail.Arguments = raw.Arguments
	detail.Result = raw.Result
	detail.Images = raw.Images
	detail.Resolved = raw.resolved(true)
	detail.Content = raw.content()
	encoded, err := encode(detail)
	if err != nil {
		return value, err
	}
	if input.Cursor != nil {
		value.Offset = *input.Cursor
	}
	value.TotalBytes = uint64(len(encoded))
	value.Text, value.NextOffset, err = utf8TextPage(string(encoded), value.Offset, input.Limit)
	if err == nil && uint64(len(value.Text)) > input.Limit {
		return value, invalid("tool detail limit splits a Unicode character")
	}
	return value, err
}
func readAgentToolOutput(ctx context.Context, db *sql.Conn, input protocol.CommandReadToolOutputPayload) (protocol.ToolOutputOutputPage, error) {
	value := protocol.DefaultToolOutputOutputPage()
	value.Output = input.Output
	value.Offset = input.Offset
	if input.Stream != nil {
		return value, fault(protocol.ErrorCodeUnavailable, "process output streams are unavailable")
	}
	if input.Output.Call == "" || len(input.Output.Call) > 4096 || len(input.Output.Revision) != 64 || !agentHex(input.Output.Revision) {
		return value, invalid("tool output reference is invalid")
	}
	run, err := activeAgentRun(ctx, db, input.Turn)
	if err != nil {
		return value, err
	}
	records, err := readBodies[canonical.Record](ctx, db, "SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 ORDER BY h.sequence DESC", run.Session.String())
	if err != nil {
		return value, err
	}
	found := false
	for _, record := range records {
		if m := record.Message(); m != nil {
			for _, block := range m.Content {
				response, ok := block.Data.(*canonical.ToolResponse)
				if !ok || response.ID != input.Output.Call {
					continue
				}
				found = true
				if response.Revision == input.Output.Revision {
					hydrated, e := HydrateCanonicalPayload(ctx, db, response.Result)
					if e != nil {
						return value, e
					}
					value.Turn = record.Turn
					value.TotalBytes = uint64(len(hydrated))
					value.Text, value.NextOffset, e = utf8TextPage(string(hydrated), input.Offset, min(input.Limit, 32*1024))
					return value, e
				}
			}
		}
	}
	if found {
		return value, fault(protocol.ErrorCodeRevisionConflict, "canonical tool output changed")
	}
	return value, fault(protocol.ErrorCodeNotFound, "canonical tool output is unavailable")
}
func searchAgentConversation(ctx context.Context, db *sql.Conn, session protocol.SessionID, query protocol.ConversationSearchQuery) (protocol.ConversationSearchPage, error) {
	value := protocol.DefaultConversationSearchPage()
	value.Session = session
	if strings.TrimSpace(query.Text) == "" || len(query.Text) > 512 || query.Limit == 0 || query.Limit > 100 || query.Before != nil && (*query.Before == 0 || *query.Before > math.MaxInt64) {
		return value, invalid("conversation search bounds are invalid")
	}
	var err error
	if value.Revision, err = historyRevision(ctx, db, session); err != nil {
		return value, err
	}
	before := uint64(math.MaxInt64)
	if query.Before != nil {
		before = *query.Before
		var owner sql.NullString
		if err = db.QueryRowContext(ctx, "SELECT (SELECT h.session FROM conversation_events h WHERE h.sequence=e.sequence AND h.session=?2) FROM agent_events e WHERE e.sequence=?1", before, session.String()).Scan(&owner); errors.Is(err, sql.ErrNoRows) {
			return value, fault(protocol.ErrorCodeNotFound, "search cursor no longer exists")
		}
		if err != nil {
			return value, err
		}
		if !owner.Valid {
			return value, fault(protocol.ErrorCodeWrongTarget, "search cursor belongs to another session")
		}
	}
	pattern := regexp.QuoteMeta(query.Text)
	if !query.CaseSensitive {
		pattern = "(?i)" + pattern
	}
	matcher, err := regexp.Compile(pattern)
	if err != nil {
		return value, invalid("conversation search text is invalid")
	}
	rows, err := db.QueryContext(ctx, "SELECT e.sequence,e.body,t.rowid,"+visibleCanonical+" FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence JOIN turns t ON t.id=e.turn WHERE h.session=?1 AND h.sequence<?2 ORDER BY h.sequence DESC LIMIT 257", session.String(), before)
	if err != nil {
		return value, err
	}
	defer rows.Close()
	count, size := 0, 0
	var last *uint64
	for rows.Next() {
		if count == 256 || size >= 4*1024*1024 || len(value.Matches) == int(query.Limit) {
			value.NextBefore = last
			break
		}
		var sequence, turnSequence uint64
		var body []byte
		var visible bool
		if err = rows.Scan(&sequence, &body, &turnSequence, &visible); err != nil {
			return value, err
		}
		count++
		size += len(body)
		n := sequence
		last = &n
		if !visible {
			continue
		}
		var record canonical.Record
		if err = json.Unmarshal(body, &record); err != nil {
			return value, err
		}
		entry, e := CanonicalEntry(record, sequence)
		if e != nil {
			return value, e
		}
		for index, part := range entry.Parts {
			if part.Kind != "text" {
				continue
			}
			text, ok := part.Data.(string)
			if !ok {
				continue
			}
			match := matcher.FindStringIndex(text)
			if match == nil {
				continue
			}
			start, end := match[0], match[1]
			left := []rune(text[:start])
			right := []rune(text[end:])
			prefix := string(left[max(0, len(left)-96):])
			suffix := string(right[:min(len(right), 96)])
			snippet := prefix + text[start:end] + suffix
			value.Matches = append(value.Matches, protocol.ConversationSearchMatch{Turn: record.Turn, TurnSequence: turnSequence, Entry: record.ID, Sequence: sequence, Part: uint64(index), Author: record.Provenance.Author, Snippet: snippet, Highlight: protocol.Range[uint64]{Start: uint64(len(prefix)), End: uint64(len(prefix) + end - start)}})
			break
		}
	}
	return value, rows.Err()
}
