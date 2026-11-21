package output

import (
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProcessedSearchPage(t *testing.T) {
	page := protocol.DefaultSearchResults()
	page.Offset, page.Skipped, page.ScannedFiles, page.Partial = 1, 3, 217, true
	for i := range 100 {
		page.Matches = append(page.Matches, protocol.SearchMatch{Path: "pkg/example.go", LineNumber: uint64(i + 1), Line: fmt.Sprintf("func example%03d() { return }", i), Matched: true})
	}
	search := func() canonical.Record {
		return resultRecord("call", map[string]any{"kind": "search_results", "data": page}, canonical.JSON)
	}
	original := search()
	before := encoded(original)
	short, ok := Processed(original, original)
	if !ok || before != encoded(original) {
		t.Fatal("search page was not shortened independently of its source")
	}
	value := object(decode(onlyResponse(&short).Result.Value))
	if value["mode"] != "content" || value["partial"] != true || encoded(value["rows"]) != "100" || encoded(value["scanned_files"]) != "217" || encoded(value["skipped"]) != "3" || value["first_path"] != "pkg/example.go" || value["next_offset"] != nil {
		t.Fatalf("search summary lost scan facts: %s", encoded(value))
	}
	checkSkipped := func(record canonical.Record) {
		t.Helper()
		if _, ok := Processed(record, record); ok {
			t.Fatal("unsupported evidence was replaced")
		}
	}
	for _, mutate := range []func(*canonical.ToolResponse){
		func(tool *canonical.ToolResponse) { tool.Status = canonical.ToolErrorStatus },
		func(tool *canonical.ToolResponse) { tool.Format = canonical.MCP },
		func(tool *canonical.ToolResponse) { tool.Attachments = []canonical.Attachment{{}} },
		func(tool *canonical.ToolResponse) { tool.Result.Assets = []canonical.BinarySlot{{}} },
		func(tool *canonical.ToolResponse) { tool.Revision = strings.Repeat("x", 64) },
	} {
		r := search()
		mutate(onlyResponse(&r))
		checkSkipped(r)
	}
	page.Matches[0].Truncated = true
	checkSkipped(search())
	page.Matches[0].Truncated = false
	page.Diagnostics = []protocol.SearchDiagnostic{{Path: "secret", Message: "access denied"}}
	checkSkipped(search())
	checkSkipped(resultRecord("other", map[string]any{"kind": "other", "data": page}, canonical.JSON))
}

func TestProcessedSessionPages(t *testing.T) {
	session, turn, older := protocol.NewSessionID(), protocol.NewTurnID(), protocol.NewTurnID()
	const revision uint64 = 9007199254740991
	const nextEntry uint64 = 604
	body := strings.Repeat("Archived result text. ", 300)
	entries := []any{map[string]any{"id": "entry", "parts": []any{map[string]any{"kind": "text", "data": body}}}}
	run := map[string]any{"turn": turn, "error": nil}
	runs := []any{run}
	truncatedEntries, truncatedInputs := []string{"entry"}, []protocol.TurnID{turn}
	search := protocol.DefaultConversationSearchPage()
	search.Session, search.Revision, search.NextBefore = session, revision, new(nextEntry)
	for i := range 24 {
		search.Matches = append(search.Matches, protocol.ConversationSearchMatch{
			Turn: turn, Entry: fmt.Sprintf("hit-%d", i), Sequence: nextEntry + uint64(i), Author: "assistant", Snippet: body[:256],
		})
	}
	for _, fixture := range []struct {
		name, tool string
		body       any
		want       map[string]any
	}{
		{"recent turns", "read_session", map[string]any{
			"page":    map[string]any{"session": session, "revision": revision, "next_before": older, "entries": entries, "runs": runs},
			"missing": []protocol.TurnID{turn}, "truncated": true, "truncated_entries": truncatedEntries, "truncated_inputs": truncatedInputs,
		}, map[string]any{
			"page":    map[string]any{"session": session, "revision": revision, "next_before": older, "entry_count": 1, "run_count": 1},
			"missing": []protocol.TurnID{turn}, "truncated": true, "truncated_entries": truncatedEntries, "truncated_inputs": truncatedInputs,
		}},
		{"one turn", "read_session", map[string]any{
			"run": run, "revision": revision, "next_before": nextEntry, "entries": entries,
			"truncated": true, "truncated_entries": truncatedEntries, "truncated_inputs": truncatedInputs,
		}, map[string]any{
			"run": map[string]any{"turn": turn}, "revision": revision, "next_before": nextEntry, "entry_count": 1,
			"truncated": true, "truncated_entries": truncatedEntries, "truncated_inputs": truncatedInputs,
		}},
		{"search matches", "search_session", search, map[string]any{
			"session": session, "revision": revision, "next_before": nextEntry, "match_count": 24, "first_turn": turn, "first_entry": "hit-0", "first_sequence": nextEntry,
		}},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			original := resultRecord("call", fixture.body, canonical.JSON)
			source := onlyResponse(&original)
			source.Name = fixture.tool
			before := encoded(original)
			repaired := clone(original)
			tool := onlyResponse(&repaired)
			tool.OriginalID, tool.ID = tool.ID, "repaired-call"
			short, ok := Processed(repaired, original)
			if !ok || before != encoded(original) || len(onlyResponse(&short).Result.Value) > 1024 {
				t.Fatal("expected a short projection without mutating the original")
			}
			response := onlyResponse(&short)
			if response.ID != tool.ID || response.OriginalID != source.ID || response.Revision != source.Revision || response.Name != source.Name {
				t.Fatal("tool identity changed")
			}
			fixture.want["processed"] = fixture.tool
			fixture.want["output"] = protocol.ToolOutputOutputReference{Call: source.ID, Revision: source.Revision}
			var fields map[string]json.RawMessage
			if err := json.Unmarshal(response.Result.Value, &fields); err != nil {
				t.Fatal(err)
			}
			if !strings.Contains(string(fields["read"]), "read_tool_output") {
				t.Fatal("missing recovery instructions")
			}
			delete(fields, "read")
			if got, want := encoded(fields), encoded(fixture.want); got != want {
				t.Fatalf("summary lost pagination metadata\n got %s\nwant %s", got, want)
			}
			if _, ok := Processed(short, original); ok {
				t.Fatal("short result was processed twice")
			}
		})
	}
}

func TestProcessedSessionKeepsDiagnostics(t *testing.T) {
	for _, diagnostic := range []string{"run error", "nested tool error", "oversize truncation metadata"} {
		t.Run(diagnostic, func(t *testing.T) {
			turn := protocol.NewTurnID()
			run := map[string]any{"turn": turn, "error": nil}
			parts := []any{map[string]any{"kind": "text", "data": strings.Repeat("Session text. ", 400)}}
			page := map[string]any{"run": run, "revision": 12, "next_before": nil,
				"truncated": false, "truncated_entries": []string{}, "truncated_inputs": []protocol.TurnID{}}
			switch diagnostic {
			case "run error":
				run["error"] = map[string]string{"message": "read failed"}
			case "nested tool error":
				parts = append(parts, map[string]any{"kind": "resource", "data": map[string]any{"type": "tool_failed"}})
			case "oversize truncation metadata":
				page["truncated"], page["truncated_entries"] = true, []string{strings.Repeat("x", 1024)}
			}
			page["entries"] = []any{map[string]any{"parts": parts}}
			record := resultRecord("call", page, canonical.JSON)
			onlyResponse(&record).Name = "read_session"
			if _, ok := Processed(record, record); ok {
				t.Fatal("history containing critical metadata was shortened")
			}
		})
	}
}
