package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"reflect"
	"slices"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/tokens"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestContextPressureBuiltinHistoryReplay(t *testing.T) {
	t.Parallel()
	m := &scriptedModel{}
	f := newFixtureWithTimeout(t, m, 20*time.Second)
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(f.ctx, "UPDATE providers SET body=json_set(body,'$.provider.models[0].context',64000)")
		return err
	}); err != nil {
		t.Fatal(err)
	}
	session, archivedTurn, revision := pressureArchiveFixture(t, f)
	complete := finish()
	complete.ProviderMetadata = fantasy.ProviderMetadata{model.UsageMetadataKey: &model.UsageMetadata{
		Reported: true, Input: new(int64(1200)), Total: new(int64(1220)),
	}}
	query := func(id, tool, args string) script {
		return response(call(id, callToolName, fmt.Sprintf(`{"name":%q,"arguments":%s}`, tool, args)), complete)
	}
	const searchID, headID, turnID = "history-search", "history-head", "history-turn"
	const headLimit = 20 * 1024
	m.scripts = []script{
		response(call("discover", "tool_search", `{"query":"select:read_session,search_session"}`), complete),
		query(searchID, "search_session", fmt.Sprintf(`{"session":%q,"text":"archive-marker","limit":72}`, session.String())),
		query(headID, "read_session", fmt.Sprintf(`{"session":%q,"limit":1}`, session.String())),
		query(turnID, "read_session", fmt.Sprintf(`{"session":%q,"turn":%q,"revision":%d,"limit":100}`, session.String(), archivedTurn.String(), revision)),
		response(text("I found the archived pages."), complete),
		func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
			var head, search pressureHistoryReceipt
			if json.Unmarshal([]byte(pressureToolText(request, headID)), &head) != nil || head.Processed != "read_session" ||
				json.Unmarshal([]byte(pressureToolText(request, searchID)), &search) != nil || search.Processed != "search_session" {
				t.Error("short follow-up did not carry both recoverable history receipts")
				response(text("Archive receipts unavailable."), complete)(ctx, request, yield)
				return
			}
			read := func(id string, ref protocol.ToolOutputOutputReference, offset, limit int) fantasy.StreamPart {
				return call(id, "read_tool_output", fmt.Sprintf(`{"output":{"call":%q,"revision":%q},"offset":%d,"limit":%d}`, ref.Call, ref.Revision, offset, limit))
			}
			response(read("head-start", head.Output, 0, headLimit), read("head-end", head.Output, headLimit, 32768),
				read("search-original", search.Output, 0, 32768), complete)(ctx, request, yield)
		},
		response(text("The archive records the inspection."), complete),
	}
	f.start()
	first := f.submit("Find and read the archived inspection records.")
	f.waitRun(first, protocol.ConversationStatusCompleted)
	stored := pressureStoredResults(f.state().Records)
	f.waitRun(f.submit("What did the archive say?"), protocol.ConversationStatusCompleted)
	if got := m.count(); got != len(m.scripts) {
		t.Fatalf("model requests = %d, want %d", got, len(m.scripts))
	}

	initial, followup, readback := m.calls[4], m.calls[5], m.calls[6]
	if len(followup.Prompt) < len(initial.Prompt) || !reflect.DeepEqual(initial.Prompt, followup.Prompt[:len(initial.Prompt)]) {
		t.Fatal("the follow-up rewrote the original projected prompt prefix")
	}
	var beforeBytes, afterBytes int
	var beforeTokens, afterTokens uint64
	for _, id := range []string{headID, searchID, turnID} {
		original := stored[id]
		if original == nil || original.Status != canonical.ToolSuccess {
			t.Fatalf("builtin %s did not save a successful canonical result", id)
		}
		projected := pressureToolText(followup, id)
		beforeBytes += len(original.Result.Value)
		afterBytes += len(projected)
		beforeTokens += tokens.Text(string(original.Result.Value))
		afterTokens += tokens.Text(projected)
		if id == turnID {
			if projected != string(original.Result.Value) {
				t.Fatal("the newest history query lost evidence")
			}
			continue
		}
		var receipt pressureHistoryReceipt
		if err := json.Unmarshal([]byte(projected), &receipt); err != nil || receipt.Processed != original.Name ||
			receipt.Output.Call != original.ID || receipt.Output.Revision != original.Revision {
			t.Fatalf("invalid canonical receipt for %s", id)
		}
		if id == headID {
			if receipt.Page.Revision == nil || *receipt.Page.Revision != revision || !slices.Contains(receipt.Missing, archivedTurn) ||
				!receipt.Truncated || len(receipt.TruncatedEntries) == 0 {
				t.Fatalf("read_session receipt lost its page revision or truncation metadata: %s", projected)
			}
		} else if receipt.Revision == nil || *receipt.Revision != revision || receipt.NextBefore == nil {
			t.Fatalf("search_session receipt lost its continuation metadata: %s", projected)
		}
		if pressureToolText(readback, id) != projected {
			t.Fatalf("a later tool batch changed the receipt for %s", id)
		}
	}
	t.Logf("builtin history queries before a short follow-up: %d -> %d tool bytes, %d -> %d local o200k tokens", beforeBytes, afterBytes, beforeTokens, afterTokens)
	if beforeBytes < toolOutputPressureBytes || afterBytes*2 >= beforeBytes || afterTokens*2 >= beforeTokens {
		t.Fatal("the short follow-up still carries more than half the original query output")
	}
	var turnReceipt pressureHistoryReceipt
	if err := json.Unmarshal([]byte(pressureToolText(readback, turnID)), &turnReceipt); err != nil || turnReceipt.Processed != "read_session" ||
		turnReceipt.Revision == nil || *turnReceipt.Revision != revision || turnReceipt.NextBefore == nil || turnReceipt.Run.Turn != archivedTurn ||
		turnReceipt.Output.Call != turnID || turnReceipt.Output.Revision != stored[turnID].Revision {
		t.Fatal("the next budget boundary lost the formerly newest turn page or its continuation cursor")
	}

	state := f.state()
	after := pressureStoredResults(state.Records)
	for _, id := range []string{headID, searchID, turnID} {
		if !reflect.DeepEqual(stored[id], after[id]) {
			t.Fatalf("the stored canonical response for %s changed", id)
		}
	}
	for _, record := range state.Records {
		if message := record.Message(); record.Kind == "compaction" || message != nil && mainReplyOverHalfWindow(record, message) {
			t.Fatal("fixture crossed the model-window or working-set threshold")
		}
	}
	readPage := func(id, source string, offset uint64) protocol.ToolOutputOutputPage {
		t.Helper()
		result, original := after[id], stored[source]
		var page protocol.ToolOutputOutputPage
		if result == nil || result.Status != canonical.ToolSuccess || json.Unmarshal(result.Result.Value, &page) != nil ||
			page.Turn != first || page.Output.Call != original.ID || page.Output.Revision != original.Revision ||
			page.Offset != offset || page.TotalBytes != uint64(len(original.Result.Value)) {
			t.Fatalf("read_tool_output %s did not recover %s at offset %d", id, source, offset)
		}
		return page
	}
	start, end := readPage("head-start", headID, 0), readPage("head-end", headID, headLimit)
	if start.NextOffset == nil || *start.NextOffset != headLimit || end.NextOffset != nil || start.Text+end.Text != string(stored[headID].Result.Value) {
		t.Fatal("read_tool_output did not reconstruct the original read_session page")
	}
	search := readPage("search-original", searchID, 0)
	if search.NextOffset != nil || search.Text != string(stored[searchID].Result.Value) {
		t.Fatal("read_tool_output did not reconstruct the original search_session page")
	}
}

type pressureHistoryReceipt struct {
	Processed string                             `json:"processed"`
	Output    protocol.ToolOutputOutputReference `json:"output"`
	Page      struct {
		Revision *uint64 `json:"revision"`
	} `json:"page"`
	Run struct {
		Turn protocol.TurnID `json:"turn"`
	} `json:"run"`
	Revision         *uint64           `json:"revision"`
	NextBefore       *uint64           `json:"next_before"`
	Missing          []protocol.TurnID `json:"missing"`
	Truncated        bool              `json:"truncated"`
	TruncatedEntries []string          `json:"truncated_entries"`
}

func pressureToolText(request fantasy.Call, id string) string {
	for _, message := range request.Prompt {
		for _, part := range message.Content {
			if result, ok := part.(fantasy.ToolResultPart); ok && result.ToolCallID == id {
				if body, ok := result.Output.(fantasy.ToolResultOutputContentText); ok {
					return body.Text
				}
			}
		}
	}
	return ""
}

func pressureStoredResults(records []canonical.Record) map[string]*canonical.ToolResponse {
	results := make(map[string]*canonical.ToolResponse)
	for _, record := range records {
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				if result, ok := block.Data.(*canonical.ToolResponse); ok {
					results[result.ID] = result
				}
			}
		}
	}
	return results
}

func pressureArchiveFixture(t *testing.T, f *engineFixture) (protocol.SessionID, protocol.TurnID, uint64) {
	t.Helper()
	active := f.session
	defer func() { f.session = active }()
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		request := protocol.NewRequest(f.node, protocol.Command{Kind: "create_session", Data: &protocol.CommandCreateSessionPayload{Config: &active.Config}})
		created, _, err := store.ExecuteCommand(f.ctx, db, f.node, f.node, f.r.profile, request, store.PreparedPaths{})
		if err == nil {
			f.session = created.Data.(protocol.Session)
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	session, turn := f.session.ID, f.submit("Archive the inspections.")
	var revision uint64
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		invocation, _, err := store.ClaimNext(f.ctx, db, f.r.clock.Now())
		if err != nil {
			return err
		}
		if invocation == nil || invocation.Run.Turn != turn {
			return fmt.Errorf("archived turn %s was not claimed", turn)
		}
		if _, _, err := store.AppendCanonical(f.ctx, db, session, inputRecord(turn, invocation.Message, f.r)); err != nil {
			return err
		}
		for i := range 80 {
			body := fmt.Sprintf("Inspection %02d. %s archive-marker %s", i, strings.Repeat("Earlier archive line. ", 6),
				strings.Repeat("The inspected station is stable; route checksum was retained for the night shift. ", 30))
			if i == 79 {
				body += strings.Repeat("Extra archive detail. ", 90)
			}
			record := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, body)), f.r.clock.Now())
			record.Provenance.Author = "assistant"
			if _, _, err := store.AppendCanonical(f.ctx, db, session, record); err != nil {
				return err
			}
		}
		if _, _, err := store.FinishTurn(f.ctx, db, turn, protocol.ConversationStatusCompleted, nil, f.r.clock.Now()); err != nil {
			return err
		}
		history, err := store.ReadConversation(f.ctx, db, session, nil, 1)
		revision = history.Page.Revision
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return session, turn, revision
}
