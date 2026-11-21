package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"testing"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAgentReferenceOverviewAndEntryLookup(t *testing.T) {
	t.Parallel()
	storage, node, sourceSession, sourceTurn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	source := AgentHistorySource{Caller: node, Session: sourceSession.ID, Turn: sourceTurn}
	var session protocol.Session
	var turns []protocol.TurnID
	var firstInput, finalReply, hiddenReply protocol.ConversationEntry
	var other protocol.User
	const initialText = "Keep the original migration plan. "
	const answer = "The new constraints are documented."
	const failure = "Provider rejected the final request. "
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		output, _, err := createSession(ctx, db, node, "", protocol.CommandCreateSessionPayload{Config: &sourceSession.Config})
		if err != nil {
			return err
		}
		session = output.Data.(protocol.Session)
		other, err = CreateUser(ctx, db, "Other historian")
		if err != nil {
			return err
		}
		for i := range 10 {
			at := now.Add(time.Duration(i+1) * time.Second)
			turn, err := delegationFixtureParent(ctx, db, node, session, at)
			if err != nil {
				return err
			}
			turns = append(turns, turn)
			input := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, fmt.Sprintf("Question %d", i))), at)
			input.Provenance.Author = "user"
			if i == 0 {
				input.Data = canonical.NewMessage(canonical.User, canonical.Text(0, "SYNTHETIC_REFERENCE"), canonical.Text(1, initialText+strings.Repeat("界", 300*1024)))
				input.Metadata.Input = &canonical.Input{Turn: &turn, Source: ptr(canonical.InputUser), TextPart: ptr(uint64(1)), CapabilityPart: ptr(uint64(0))}
			}
			entry, _, err := AppendCanonical(ctx, db, session.ID, input)
			if err != nil {
				return err
			}
			if i == 0 {
				firstInput = entry
			}
			status := protocol.ConversationStatusCompleted
			var failureValue *protocol.Fault
			switch i {
			case 8:
				payload, err := encode(map[string]any{"nested": map[string]string{"text": "TOOL_PAYLOAD_" + strings.Repeat("x", 32*1024)}})
				if err != nil {
					return err
				}
				if _, err = appendActivityFixtureToolResult(ctx, db, session.ID, turn, payload, at); err != nil {
					return err
				}
				finalReply, err = appendActivityFixtureText(ctx, db, session.ID, turn, answer, at)
				if err != nil {
					return err
				}
				hiddenReply, err = appendActivityFixtureText(ctx, db, session.ID, turn, "HIDDEN_ASSISTANT_TEXT", at)
				if err != nil {
					return err
				}
				if err = hideActivityFixtureEntry(ctx, db, session.ID, turn, hiddenReply.ID, at); err != nil {
					return err
				}
				call := canonical.ToolRequest{ID: "intermediate-lookup", Name: "fixture_tool", Arguments: json.RawMessage(`{"text":"TOOL_ARGUMENT"}`)}
				preamble := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "INTERMEDIATE_PREAMBLE"), canonical.Block{Index: 1, Kind: "tool_request", Data: &call}), at)
				preamble.Provenance.Author = "assistant"
				if _, _, err = AppendCanonical(ctx, db, session.ID, preamble); err != nil {
					return err
				}
			case 9:
				status = protocol.ConversationStatusFailed
				failureValue = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: failure + strings.Repeat("x", 600)}
			default:
				if _, err = appendActivityFixtureText(ctx, db, session.ID, turn, fmt.Sprintf("Final reply %d", i), at); err != nil {
					return err
				}
			}
			_, _, err = FinishTurn(ctx, db, turn, status, failureValue, at)
			if err != nil {
				return err
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	read := func(arguments string) json.RawMessage {
		t.Helper()
		body, err := queryAgentHistoryFixture(ctx, storage, node, source, arguments)
		if err != nil {
			t.Fatal(err)
		}
		return body
	}
	detail := func(output json.RawMessage, sequence uint64, id string) {
		t.Helper()
		var result struct {
			Entry struct {
				ID       string `json:"id"`
				Sequence uint64 `json:"sequence"`
			} `json:"entry"`
		}
		if err := json.Unmarshal(output, &result); err != nil || result.Entry.ID != id || result.Entry.Sequence != sequence {
			t.Fatalf("entry lookup = %s, error = %v", output, err)
		}
	}
	body := read(fmt.Sprintf(`{"session":%q,"mode":"overview","limit":2}`, session.ID.String()))
	if len(body) > 16*1024 {
		t.Fatalf("overview exceeded the page bound: %d bytes", len(body))
	}
	for _, marker := range []string{"SYNTHETIC_REFERENCE", "TOOL_PAYLOAD_", "TOOL_ARGUMENT", "HIDDEN_ASSISTANT_TEXT", "INTERMEDIATE_PREAMBLE"} {
		if strings.Contains(string(body), marker) {
			t.Errorf("overview exposed %q", marker)
		}
	}
	var head referenceOverview
	if err := json.Unmarshal(body, &head); err != nil {
		t.Fatal(err)
	}
	if head.Session != session.ID || head.Revision == 0 || head.NextBefore == nil || *head.NextBefore != turns[8] || len(head.Turns) != 2 {
		t.Fatalf("wrong overview page: %+v", head)
	}
	intent := head.InitialIntent
	if intent == nil || intent.Turn != turns[0] || !strings.HasPrefix(intent.Text, initialText) || !utf8.ValidString(intent.Text) || !intent.Truncated || len(intent.Text) > 320 || intent.EntrySequence == nil || *intent.EntrySequence != firstInput.Sequence {
		t.Fatalf("missing or unbounded early intent: %+v", intent)
	}
	failed, complete := head.Turns[0], head.Turns[1]
	if failed.Turn != turns[9] || failed.Status != protocol.ConversationStatusFailed || failed.Reply != nil || failed.Error == nil || failed.Error.Code != protocol.ErrorCodeUnavailable || !strings.HasPrefix(failed.Error.Message, failure) || !failed.Error.Truncated {
		t.Fatalf("overview lost the run failure: %+v", failed)
	}
	if complete.Turn != turns[8] || complete.Reply == nil || complete.Reply.Text != answer || complete.Reply.EntrySequence == nil || *complete.Reply.EntrySequence != finalReply.Sequence {
		t.Fatalf("overview lost the final public reply: %+v", complete.Reply)
	}
	lookup := func(turn protocol.TurnID, sequence uint64) string {
		return fmt.Sprintf(`{"session":%q,"turn":%q,"revision":%d,"entry_sequence":%d}`, session.ID.String(), turn.String(), head.Revision, sequence)
	}
	detail(read(lookup(intent.Turn, *intent.EntrySequence)), firstInput.Sequence, firstInput.ID)
	detail(read(lookup(complete.Turn, *complete.Reply.EntrySequence)), finalReply.Sequence, finalReply.ID)
	if faultBody := read(fmt.Sprintf(`{"session":%q,"turn":%q,"revision":%d}`, session.ID.String(), failed.Turn.String(), head.Revision)); !strings.Contains(string(faultBody), failure+strings.Repeat("x", 600)) {
		t.Fatalf("run detail lost error evidence: %s", faultBody)
	}
	check := func(readCtx context.Context, caller AgentHistorySource, arguments string, want protocol.ErrorCode) {
		t.Helper()
		_, err := queryAgentHistoryFixture(readCtx, storage, node, caller, arguments)
		var failure *protocol.Fault
		if !errors.As(err, &failure) || failure.Code != want {
			t.Fatalf("history arguments %s returned %v, want %v", arguments, err, want)
		}
	}
	check(ctx, source, lookup(complete.Turn, hiddenReply.Sequence), protocol.ErrorCodeNotFound)
	check(ctx, source, lookup(complete.Turn, firstInput.Sequence), protocol.ErrorCodeNotFound)
	check(ctx, source, fmt.Sprintf(`{"session":%q,"mode":"overview","before":%q}`, session.ID.String(), head.NextBefore.String()), protocol.ErrorCodeInvalidRequest)
	check(ctx, source, fmt.Sprintf(`{"session":%q,"mode":"overview","before":%q,"revision":%d}`, session.ID.String(), sourceTurn.String(), head.Revision), protocol.ErrorCodeWrongTarget)

	next := fmt.Sprintf(`{"session":%q,"mode":"overview","before":%q,"revision":%d,"limit":100}`, session.ID.String(), head.NextBefore.String(), head.Revision)
	var older referenceOverview
	if err := json.Unmarshal(read(next), &older); err != nil {
		t.Fatal(err)
	}
	if older.NextBefore != nil || older.InitialIntent != nil || len(older.Turns) != 8 {
		t.Fatalf("wrong overview continuation: %+v", older)
	}
	for i, turn := range older.Turns {
		want := 7 - i
		if turn.Turn != turns[want] || turn.Reply == nil || turn.Reply.Text != fmt.Sprintf("Final reply %d", want) {
			t.Errorf("older turn %d = %+v", want, turn)
		}
	}
	for _, args := range []string{
		fmt.Sprintf(`{"session":%q,"mode":"overview"}`, session.ID.String()),
		fmt.Sprintf(`{"session":%q,"mode":"usage","turn":%q}`, session.ID.String(), complete.Turn.String()),
	} {
		wrong := source
		wrong.Caller[0] ^= 1
		check(ctx, wrong, args, protocol.ErrorCodePermissionDenied)
		check(WithUser(ctx, other), source, args, protocol.ErrorCodePermissionDenied)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		_, _, err := rewindConversation(ctx, db, protocol.CommandRewindConversationPayload{
			Session: session.ID, Through: &turns[7], ExpectedHead: turns[9], ExpectedRevision: head.Revision,
		}, false)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	check(ctx, source, next, protocol.ErrorCodeRevisionConflict)
	check(ctx, source, lookup(complete.Turn, finalReply.Sequence), protocol.ErrorCodeRevisionConflict)
	check(ctx, source, fmt.Sprintf(`{"session":%q,"mode":"usage","turn":%q,"revision":%d}`, session.ID.String(), complete.Turn.String(), head.Revision), protocol.ErrorCodeRevisionConflict)
	check(ctx, source, fmt.Sprintf(`{"session":%q,"mode":"usage","turn":%q}`, session.ID.String(), complete.Turn.String()), protocol.ErrorCodeWrongTarget)
}

func TestAgentReferenceTurnUsageWithoutRevision(t *testing.T) {
	t.Parallel()
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	var sourceTurn protocol.TurnID
	var entries []protocol.ConversationEntry
	var requestIDs []string
	var expectedBoundary *protocol.ConversationResponsesBoundary
	tokens := []canonical.Usage{
		{Input: ptr(uint64(40)), Output: ptr(uint64(8)), CacheRead: ptr(uint64(6)), CacheWrite: ptr(uint64(9)), Reasoning: ptr(uint64(2)), Total: ptr(uint64(48))},
		{Input: ptr(uint64(12)), Output: ptr(uint64(2)), CacheRead: ptr(uint64(0))},
		{Input: ptr(uint64(10)), Output: ptr(uint64(5)), CacheRead: ptr(uint64(1)), CacheWrite: ptr(uint64(3)), Total: ptr(uint64(15))},
		{Input: ptr(uint64(500)), Total: ptr(uint64(500))},
		{},
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		input := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Measure this turn")), now)
		input.Provenance.Author = "user"
		if _, _, err := AppendCanonical(ctx, db, session.ID, input); err != nil {
			return err
		}
		for i, usage := range tokens {
			message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Unrelated to usage projection"))
			message.Model = ptr("fixture-model")
			message.Usage = &usage
			record := canonical.NewRecord(turn, "message", &message, now.Add(time.Duration(i+1)*time.Second))
			record.Provenance.Author = "assistant"
			if i < 2 {
				id := protocol.NewRequestID().String()
				requestIDs = append(requestIDs, id)
				request := protocol.DefaultConversationObservationRequest()
				request.ID, request.Turn, request.Model, request.TimestampMs = id, turn, *message.Model, record.Created.UnixMilli()
				attempt := protocol.DefaultConversationObservationAttempt()
				attempt.Index, attempt.Transport, attempt.Outcome = 1, "http", protocol.ConversationObservationOutcomeSucceeded
				if i == 1 {
					expectedBoundary = &protocol.ConversationResponsesBoundary{
						PreviousRequestID: requestIDs[0], PreviousAttempt: 1, PreviousCachedInput: tokens[0].CacheRead,
						InstructionsEqual: true, ToolsEqual: true, CacheOptionsEqual: true, OtherFieldsEqual: true,
						SharedInputItems: 2, PreviousInputItems: 2, InputItems: 4,
					}
					attempt.ResponsesBoundary = expectedBoundary
				}
				request.Attempts = []protocol.ConversationObservationAttempt{attempt}
				record.Metadata.Execution = &canonical.Execution{Observation: &request}
			}
			switch i {
			case 1:
				runtime := canonical.DefaultRuntime()
				runtime.UserVisible, runtime.AgentVisible = false, false
				record.Metadata.Runtime = &runtime
			case 2:
				record.Kind = "compaction"
				record.Data = &canonical.Compaction{Boundary: input.ID, Summary: message, Start: now, End: now}
				record.Provenance.Author = "system"
			}
			entry, _, err := AppendCanonical(ctx, db, session.ID, record)
			if err != nil {
				return err
			}
			entries = append(entries, entry)
		}
		if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now.Add(time.Minute)); err != nil {
			return err
		}
		var err error
		sourceTurn, err = delegationFixtureParent(ctx, db, node, session, now.Add(time.Minute))
		if err != nil {
			return err
		}
		message := canonical.NewMessage(canonical.Assistant)
		message.Usage = &canonical.Usage{Input: ptr(uint64(1100)), Output: ptr(uint64(99))}
		other := canonical.NewRecord(sourceTurn, "message", &message, now.Add(time.Minute))
		other.Provenance.Author = "assistant"
		_, _, err = AppendCanonical(ctx, db, session.ID, other)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	source := AgentHistorySource{Caller: node, Session: session.ID, Turn: sourceTurn}
	read := func(arguments string) referenceUsage {
		t.Helper()
		body, err := queryAgentHistoryFixture(ctx, storage, node, source, arguments)
		if err != nil {
			t.Fatal(err)
		}
		if strings.Contains(string(body), "Unrelated to usage projection") {
			t.Fatalf("usage included a message body: %s", body)
		}
		var result referenceUsage
		if err := json.Unmarshal(body, &result); err != nil {
			t.Fatal(err)
		}
		return result
	}
	got := read(fmt.Sprintf(`{"session":%q,"mode":"usage","turn":%q,"limit":2}`, session.ID.String(), turn.String()))
	want := referenceUsageTokens{Input: ptr(uint64(562)), Output: ptr(uint64(15)), CachedInput: ptr(uint64(7)), CacheWrite: ptr(uint64(12)), Reasoning: ptr(uint64(2)), ReportedTotal: ptr(uint64(563))}
	if got.Session != session.ID || got.Turn != turn || got.Status != protocol.ConversationStatusCompleted || got.Revision == 0 || got.ReportedResponses != 4 || got.MeasuredResponses != 3 || !reflect.DeepEqual(got.Usage, want) {
		t.Fatalf("turn usage returned %+v, want totals %+v", got, want)
	}
	if len(got.Requests) != 2 || got.NextBefore == nil || *got.NextBefore != entries[2].Sequence {
		t.Fatalf("usage head did not return two recent requests and a cursor: %+v", got)
	}
	older := read(fmt.Sprintf(`{"session":%q,"mode":"usage","turn":%q,"before_entry":%d,"revision":%d}`, session.ID.String(), turn.String(), *got.NextBefore, got.Revision))
	if len(older.Requests) != 2 || older.NextBefore != nil || older.ReportedResponses != got.ReportedResponses || !reflect.DeepEqual(older.Usage, got.Usage) {
		t.Fatalf("usage continuation lost requests or changed the turn totals: %+v", older)
	}
	for i, row := range append(got.Requests, older.Requests...) {
		index := 3 - i
		usage := tokens[index]
		want := referenceUsageTokens{Input: usage.Input, Output: usage.Output, CachedInput: usage.CacheRead, CacheWrite: usage.CacheWrite, Reasoning: usage.Reasoning, ReportedTotal: usage.Total}
		if row.EntrySequence != entries[index].Sequence || row.EntryPublic != (index != 1) || row.TimestampMs != now.Add(time.Duration(index+1)*time.Second).UnixMilli() || row.Model != "fixture-model" || !reflect.DeepEqual(row.Usage, want) {
			t.Errorf("request %d returned %+v, want sequence %d and usage %+v", index, row, entries[index].Sequence, want)
		}
		if index < 2 {
			if row.RequestID != requestIDs[index] || row.Attempt != 1 || index == 0 && row.ResponsesBoundary != nil || index == 1 && !reflect.DeepEqual(row.ResponsesBoundary, expectedBoundary) {
				t.Errorf("request %d lost its measured adjacency: %+v", index, row)
			}
		} else if row.RequestID != "" || row.Attempt != 0 || row.ResponsesBoundary != nil {
			t.Errorf("request %d invented observation fields: %+v", index, row)
		}
	}
	_, err := queryAgentHistoryFixture(ctx, storage, node, source, fmt.Sprintf(`{"session":%q,"mode":"usage","turn":%q,"before_entry":%d}`, session.ID.String(), turn.String(), *got.NextBefore))
	var invalidQuery *protocol.Fault
	if !errors.As(err, &invalidQuery) || invalidQuery.Code != protocol.ErrorCodeInvalidRequest {
		t.Fatalf("usage cursor without revision returned %v", err)
	}
}

func queryAgentHistoryFixture(ctx context.Context, storage *Store, node protocol.NodeID, source AgentHistorySource, arguments string) (json.RawMessage, error) {
	var body json.RawMessage
	err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		body, err = ReadAgentReference(ctx, db, node, source, "read_session", json.RawMessage(arguments))
		return err
	})
	return body, err
}
