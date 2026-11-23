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
	entries := []any{map[string]any{"id": "entry", "parts": []any{
		map[string]any{"kind": "text", "data": body},
		map[string]any{"kind": "resource", "data": map[string]any{"type": "tool_result", "result": map[string]any{"error": "archived data"}}},
	}}}
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

func TestProcessedRecoverableResults(t *testing.T) {
	const originalCall = "stored-call"
	body := strings.Repeat("bulk evidence from an old result\n", 52)
	web := protocol.WebPage{
		Url: "https://example.invalid/docs", Status: 200, ContentType: "text/plain", Content: body,
		Untrusted: true, Truncated: true, NextOffset: new(uint64(20000)),
	}
	diff := protocol.GitDiff{Path: "file.go", Scope: protocol.GitDiffScopeAll, Text: body, Additions: 32}
	textResult := map[string]any{"content": []any{map[string]any{
		"type": "text", "text": encoded(map[string]any{"snapshot": body, "error": "text from a rendered window"}),
	}}, "isError": false}
	for _, fixture := range []struct {
		name   string
		value  any
		format canonical.ToolFormat
		want   string
		facts  map[string]any
	}{
		{"web page", protocol.Output{Kind: "web_page", Data: web}, canonical.JSON, "web_page", map[string]any{"url": web.Url, "untrusted": true, "next_offset": 20000}},
		{"web search", map[string]any{"results": []any{map[string]string{"snippet": body}}, "untrusted": true}, canonical.JSON, "json", map[string]any{"untrusted": true}},
		{"git status", map[string]any{"branch": "main", "entries": []any{map[string]string{"path": body}}}, canonical.JSON, "json", map[string]any{"branch": "main"}},
		{"git diff", protocol.Output{Kind: "git_diff", Data: diff}, canonical.JSON, "git_diff", map[string]any{"path": diff.Path, "additions": 32}},
		{"plain text", body, canonical.JSON, "text", nil},
		{"JSON array", []string{body, body}, canonical.JSON, "json", map[string]any{"items": 2}},
		{"desktop MCP", textResult, canonical.MCP, "mcp", nil},
		{"desktop JSON wrapper", textResult, canonical.JSON, "mcp", nil},
		{"structured only MCP", map[string]any{"content": []any{}, "structuredContent": map[string]string{"path": "file.go", "body": body}}, canonical.MCP, "mcp", map[string]any{"path": "file.go"}},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			source := resultRecord(originalCall, fixture.value, fixture.format)
			original := string(onlyResponse(&source).Result.Value)
			visible := clone(source)
			tool := onlyResponse(&visible)
			tool.ID, tool.OriginalID = "repaired-call", originalCall
			short, ok := Processed(visible, source)
			if !ok {
				t.Fatal("recoverable result was not shortened")
			}
			result := onlyResponse(&short)
			summary := object(decode(result.Result.Value))
			ref := object(summary["output"])
			if result.Format != canonical.JSON || result.ID != tool.ID || result.OriginalID != originalCall || result.Revision != tool.Revision ||
				len(result.Result.Value) > processedMaxBytes || len(result.Result.Value) >= len(original) || strings.Contains(string(result.Result.Value), "bulk evidence") ||
				summary["processed"] != fixture.want || ref["call"] != originalCall || ref["revision"] != tool.Revision {
				t.Fatalf("invalid projection: %s", result.Result.Value)
			}
			for key, want := range fixture.facts {
				if encoded(summary[key]) != encoded(want) {
					t.Errorf("%s = %v, want %v", key, summary[key], want)
				}
			}
			projected := new(Cache).Project([]canonical.Record{short})
			if again, changed := Processed(visible, source); !changed || encoded(again) != encoded(short) ||
				!strings.Contains(string(onlyResponse(&projected[0]).Result.Value), "read_tool_output") ||
				string(onlyResponse(&source).Result.Value) != original || string(tool.Result.Value) != original {
				t.Fatal("projection was unstable, lost its reference, or mutated its source")
			}
		})
	}
}

func TestProcessedKeepsProtectedResults(t *testing.T) {
	body := strings.Repeat("important evidence\n", 90)
	mcp := func(part map[string]any, failed bool) map[string]any {
		return map[string]any{"content": []any{map[string]any{"type": "text", "text": body}, part}, "isError": failed}
	}
	for _, fixture := range []struct {
		name   string
		value  any
		format canonical.ToolFormat
	}{
		{"diagnostics", map[string]any{"result": body, "diagnostics": []string{"permission denied"}}, canonical.JSON},
		{"unknown tagged result", map[string]any{"kind": "unrecognized", "data": body}, canonical.JSON},
		{"unknown format", body, canonical.ToolFormat("unknown")},
		{"MCP error", mcp(map[string]any{"type": "text", "text": "failure"}, true), canonical.MCP},
		{"JSON-wrapped MCP error", mcp(map[string]any{"type": "text", "text": "failure"}, true), canonical.JSON},
		{"MCP media", mcp(map[string]any{"type": "image", "data": "aW1hZ2U=", "mimeType": "image/png"}, false), canonical.MCP},
		{"MCP unknown content", mcp(map[string]any{"type": "extension"}, false), canonical.MCP},
		{"MCP hidden content", mcp(map[string]any{"type": "text", "text": "user only", "annotations": map[string]any{"audience": []string{"user"}}}, false), canonical.MCP},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			source := resultRecord("stored-call", fixture.value, fixture.format)
			if _, ok := Processed(source, source); ok {
				t.Fatal("protected result acquired a replacement reference")
			}
		})
	}
	source := resultRecord("stored-call", map[string]string{"result": body}, canonical.JSON)
	filtered := clone(source)
	onlyResponse(&filtered).Result.Value = json.RawMessage(encoded(map[string]string{"result": body + " filtered"}))
	if _, ok := Processed(filtered, source); ok {
		t.Fatal("a modified projection acquired a reference to unfiltered evidence")
	}
	for _, toolName := range []string{"load_skill", "read_skill_resource", "read_skill_document", "tool_search", "ask_user", "update_plan"} {
		t.Run("protected tool "+toolName, func(t *testing.T) {
			record := resultRecord("stored-"+toolName, map[string]any{"body": body}, canonical.JSON)
			onlyResponse(&record).Name = toolName
			if _, ok := Processed(record, record); ok {
				t.Fatalf("protected tool %s was folded under pressure", toolName)
			}
		})
	}
}

func TestProcessedCommandRecovery(t *testing.T) {
	const call, storedCall = "repaired-command", "stored-command"
	id := protocol.NewRequestID().String()
	body := strings.Repeat("stdout content\n", 130)
	retained := map[string]any{"id": id, "stdout": map[string]any{
		"tail": "retained-only tail content", "tail_offset": 65536, "total_bytes": 66000, "retained_end": 66000,
	}}
	success := map[string]any{"kind": "exited", "data": 0}
	for _, fixture := range []struct {
		name      string
		tool      string
		kind      string
		outcome   map[string]any
		body      string
		truncated bool
		retained  map[string]any
		want      bool
	}{
		{"retained success", "run_command", "command_result", success, body, true, retained, true},
		{"plain capture", "run_command", "command_result", map[string]any{"exit_code": 0}, body, false, nil, true},
		{"nonzero exit", "run_command", "command_result", map[string]any{"kind": "exited", "data": 1}, body, false, nil, false},
		{"command timeout", "run_command", "command_result", map[string]any{"kind": "timed_out"}, body, false, retained, false},
		{"async wait timeout", "read_command", "command_output", success, body, false, retained, false},
		{"truncated capture without stream ID", "run_command", "command_result", success, body, true, nil, false},
		{"only retained reference", "run_command", "command_result", success, "", true, map[string]any{"id": id, "stdout": map[string]any{"tail": body}}, false},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			capture := map[string]any{
				"outcome": fixture.outcome,
				"stdout":  map[string]any{"text": fixture.body, "truncated": fixture.truncated},
				"stderr":  map[string]any{"text": "", "truncated": false},
			}
			const diagnostic = "Warning: one package was skipped."
			if fixture.want {
				object(capture["stderr"])["text"] = diagnostic
			}
			if fixture.kind == "command_output" {
				delete(capture, "outcome")
				capture["wait"] = protocol.ProcessWaitDeadline
			}
			value := map[string]any{"kind": fixture.kind, "data": capture}
			if fixture.retained != nil {
				value["sailry_command"] = fixture.retained
			}
			original := resultRecord(storedCall, value, canonical.JSON)
			source := onlyResponse(&original)
			source.Name = fixture.tool
			repaired := clone(original)
			tool := onlyResponse(&repaired)
			tool.ID, tool.OriginalID = call, storedCall
			short, ok := Processed(repaired, original)
			if ok != fixture.want {
				t.Fatalf("changed = %t, want %t", ok, fixture.want)
			}
			if !ok {
				return
			}
			result := onlyResponse(&short)
			got := object(decode(result.Result.Value))
			data, ref := object(got["data"]), object(got["output"])
			stdout := object(data["stdout"])
			if got["processed"] != "command_result" || encoded(data["outcome"]) != encoded(fixture.outcome) ||
				ref["call"] != storedCall || ref["revision"] != source.Revision || stdout["truncated"] != fixture.truncated ||
				encoded(stdout["bytes"]) != encoded(len(body)) || object(data["stderr"])["text"] != diagnostic ||
				strings.Contains(string(result.Result.Value), "stdout content") {
				t.Fatalf("projection lost outcome, reference or capture metadata, or kept duplicate text: %s", result.Result.Value)
			}
			if fixture.retained != nil {
				streams := object(got["sailry_command"])
				retainedStdout := object(streams["stdout"])
				if streams["id"] != id || encoded(retainedStdout["tail_offset"]) != "65536" ||
					encoded(retainedStdout["retained_end"]) != "66000" || retainedStdout["tail"] != nil {
					t.Fatalf("projection lost the retained stream reference or its bounds: %s", result.Result.Value)
				}
			}
			projected := new(Cache).Project([]canonical.Record{short})
			if string(onlyResponse(&projected[0]).Result.Value) != string(result.Result.Value) {
				t.Fatal("command projection changed the recovery summary")
			}
		})
	}
}
