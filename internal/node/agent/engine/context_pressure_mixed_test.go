package engine

import (
	"context"
	"encoding/json"
	"fmt"
	"reflect"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/output"
	"github.com/sailry/sailry-harness/internal/node/agent/tokens"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type mixedPressureResult struct {
	id, name string
	value    json.RawMessage
}

func mixedPressureResults(target int) []mixedPressureResult {
	names := []string{"git_status", "git_diff", "code_context", "web_fetch", "run_command", "code_search", "web_search", "git_diff"}
	results := make([]mixedPressureResult, 0, len(names))
	for index, name := range names {
		var rows []map[string]any
		var body strings.Builder
		var value []byte
		for row := 0; len(value) < target; row++ {
			path := fmt.Sprintf("internal/replay/part_%02d/item_%03d.go", index, row)
			switch name {
			case "git_status":
				rows = append(rows, map[string]any{"path": path, "index_status": " ", "worktree_status": "M", "original_path": nil})
				value, _ = json.Marshal(map[string]any{"branch": "fixture-review", "staged": false, "entries": rows})
			case "git_diff":
				fmt.Fprintf(&body, "diff --git a/%s b/%s\n--- a/%s\n+++ b/%s\n@@ -10,3 +10,3 @@ func Resolve() string {\n-\treturn \"pending\"\n+\treturn \"ready-%03d\"\n }\n", path, path, path, path, row)
				value, _ = json.Marshal(map[string]any{"path": "internal/replay", "scope": "unstaged", "diff": body.String(), "truncated": false})
			case "code_context", "code_search":
				rows = append(rows, map[string]any{"path": path, "name": fmt.Sprintf("Resolve%03d", row), "signature": "func Resolve(ctx context.Context, key string) (Entry, error)", "start_line": 10, "end_line": 14, "source": "func Resolve(ctx context.Context, key string) (Entry, error) {\n\treturn repository.Lookup(ctx, key)\n}", "confidence": "local", "ambiguous": false})
				value, _ = json.Marshal(map[string]any{"symbols": rows, "partial": false, "next_offset": nil})
			case "web_fetch", "web_search":
				rows = append(rows, map[string]any{"url": fmt.Sprintf("https://docs.example.invalid/replay/section-%03d", row), "title": fmt.Sprintf("Recovery API section %03d", row), "text": "## Reading retained output\n\nUse the immutable call identifier and revision to read the original response. A continuation offset is measured in UTF-8 bytes; retain the revision between pages.\n", "published": "2026-01-01"})
				value, _ = json.Marshal(map[string]any{"results": rows, "truncated": false})
			case "run_command":
				fmt.Fprintf(&body, "ok  example.invalid/replay/package%03d  0.012s\n", row)
				value, _ = json.Marshal(map[string]any{"kind": "command_result", "data": map[string]any{"elapsed_ms": 12, "outcome": map[string]any{"kind": "exited", "data": 0}, "stdout": map[string]any{"text": body.String(), "truncated": false, "invalid_utf8": false}, "stderr": map[string]any{"text": "", "truncated": false, "invalid_utf8": false}}})
			}
		}
		results = append(results, mixedPressureResult{id: fmt.Sprintf("mixed-%d", index), name: name, value: value})
	}
	return results
}

func TestContextPressureMixedToolFollowup(t *testing.T) {
	results := mixedPressureResults(26 * 1024)
	m := &scriptedModel{}
	var tools []Tool
	seen := map[string]bool{}
	byID := map[string]json.RawMessage{}
	for _, result := range results {
		byID[result.id] = result.value
		if seen[result.name] {
			continue
		}
		seen[result.name] = true
		tools = append(tools, objectTool(result.name, true, true, func(_ context.Context, request canonical.ToolRequest) ToolResult {
			return ToolResult{Format: canonical.JSON, Value: byID[request.ID]}
		}))
	}
	for batch := range len(results) / 2 {
		a, b := results[batch*2], results[batch*2+1]
		m.scripts = append(m.scripts,
			response(call(a.id, a.name, `{}`), call(b.id, b.name, `{}`), workingSetUsage(1200)),
			response(text("Inspection complete; the evidence is retained."), workingSetUsage(1400)))
	}
	m.scripts = append(m.scripts, func(ctx context.Context, request fantasy.Call, yield func(fantasy.StreamPart) bool) {
		var receipt struct {
			Output protocol.ToolOutputOutputReference `json:"output"`
		}
		if err := json.Unmarshal([]byte(pressureToolText(request, results[0].id)), &receipt); err != nil || receipt.Output.Call == "" {
			t.Error("ordinary short follow-up did not retain a recoverable old result")
			response(text("Missing receipt."), workingSetUsage(1500))(ctx, request, yield)
			return
		}
		args, _ := json.Marshal(map[string]any{"output": receipt.Output, "offset": 0, "limit": 32768})
		response(call("mixed-readback", "read_tool_output", string(args)), workingSetUsage(1500))(ctx, request, yield)
	}, response(text("The earlier inspection was successful."), workingSetUsage(1600)))
	f := newFixtureWithTimeout(t, m, 20*time.Second, tools...)
	setWorkingSetWindow(t, f)
	f.start()
	var first protocol.TurnID
	for batch := range len(results) / 2 {
		turn := f.submit(fmt.Sprintf("Inspect section %d and retain the evidence.", batch))
		if batch == 0 {
			first = turn
		}
		f.waitRun(turn, protocol.ConversationStatusCompleted)
	}
	stored := pressureStoredResults(f.state().Records)
	f.waitRun(f.submit("What did you find?"), protocol.ConversationStatusCompleted)
	m.mu.Lock()
	calls := append([]fantasy.Call(nil), m.calls...)
	m.mu.Unlock()
	if len(calls) != len(m.scripts) {
		t.Fatalf("model calls = %d, want %d", len(calls), len(m.scripts))
	}
	before, followup := calls[7], calls[8]
	if len(followup.Prompt) < len(before.Prompt) || !reflect.DeepEqual(before.Prompt, followup.Prompt[:len(before.Prompt)]) {
		t.Fatal("ordinary follow-up rewrote the previous projected prefix")
	}
	var originalBytes, projectedBytes int
	var originalTokens, projectedTokens uint64
	for index, result := range results {
		source := stored[result.id]
		if source == nil || source.Status != canonical.ToolSuccess || !reflect.DeepEqual(source.Result.Value, result.value) {
			t.Fatalf("canonical result %s was not retained exactly", result.id)
		}
		body := pressureToolText(followup, result.id)
		originalBytes += len(source.Result.Value)
		projectedBytes += len(body)
		originalTokens += tokens.Text(string(source.Result.Value))
		projectedTokens += tokens.Text(body)
		if index >= len(results)-2 {
			if body != pressureToolText(before, result.id) || len(body) < len(result.value)/2 {
				t.Fatalf("latest batch lost its readable evidence: %s", result.id)
			}
			continue
		}
		var receipt struct {
			Output protocol.ToolOutputOutputReference `json:"output"`
		}
		if json.Unmarshal([]byte(body), &receipt) != nil || receipt.Output.Call != source.ID || receipt.Output.Revision != source.Revision || len(body)*2 >= len(source.Result.Value) {
			t.Fatalf("old %s result was not shortened with its canonical reference", result.name)
		}
	}
	t.Logf("mixed short follow-up: tool bytes %d -> %d; tool body tokens %d -> %d", originalBytes, projectedBytes, originalTokens, projectedTokens)
	if projectedBytes*2 >= originalBytes || projectedTokens*2 >= originalTokens {
		t.Fatal("older mixed results still dominate the short follow-up")
	}
	state := f.state()
	after := pressureStoredResults(state.Records)
	for _, result := range results {
		if !reflect.DeepEqual(stored[result.id], after[result.id]) {
			t.Fatalf("canonical result %s changed after recovery", result.id)
		}
	}
	var page protocol.ToolOutputOutputPage
	recovered := after["mixed-readback"]
	if recovered == nil || recovered.Status != canonical.ToolSuccess || json.Unmarshal(recovered.Result.Value, &page) != nil || page.Turn != first || page.Output.Call != results[0].id || page.Output.Revision != stored[results[0].id].Revision || page.Offset != 0 || page.NextOffset != nil || page.TotalBytes != uint64(len(results[0].value)) || page.Text != string(results[0].value) {
		t.Fatal("read_tool_output did not recover the complete immutable original")
	}
	for _, record := range state.Records {
		if message := record.Message(); record.Kind == "compaction" || message != nil && mainReplyOverHalfWindow(record, message) {
			t.Fatal("mixed replay unexpectedly crossed a compaction or model-window boundary")
		}
	}
}

func mixedPressureHistory(results []mixedPressureResult) []canonical.Record {
	var history []canonical.Record
	for batch := range len(results) / 2 {
		var requests, responses []canonical.Block
		for slot := range 2 {
			result := results[batch*2+slot]
			request := canonical.ToolRequest{ID: result.id, Name: result.name, Arguments: json.RawMessage(`{}`)}
			requests = append(requests, canonical.Block{Index: uint64(slot), Kind: "tool_request", Data: &request})
			responses = append(responses, responseBlock(uint64(slot), request, ToolResult{Format: canonical.JSON, Value: result.value}))
		}
		reply := canonical.NewMessage(canonical.Assistant, requests...)
		answer := canonical.NewMessage(canonical.User, responses...)
		history = append(history,
			canonical.Record{ID: fmt.Sprintf("mixed-request-%d", batch), Turn: protocol.TurnID{1}, Kind: "message", Data: &reply},
			canonical.Record{ID: fmt.Sprintf("mixed-response-%d", batch), Turn: protocol.TurnID{1}, Kind: "message", Data: &answer})
	}
	return history
}

func TestContextPressureMixedBelowThresholdStable(t *testing.T) {
	results := mixedPressureResults(6 * 1024)
	history := mixedPressureHistory(results)
	original, err := json.Marshal(history)
	if err != nil {
		t.Fatal(err)
	}
	var warm output.Cache
	for end := 2; end <= len(history); end += 2 {
		prefix := history[:end]
		baseline := new(output.Cache).Project(model.VisibleAnsweredRecords(prefix))
		projected := pressureProjection(prefix, &warm, true)
		if !reflect.DeepEqual(projected, baseline) || !reflect.DeepEqual(projected, pressureProjection(prefix, new(output.Cache), true)) {
			t.Fatalf("below-threshold batch %d changed the ordinary projection", end/2)
		}
		followup := canonical.NewMessage(canonical.User, canonical.Text(0, "Anything else?"))
		next := append(append([]canonical.Record(nil), prefix...), canonical.Record{ID: "short-followup", Turn: protocol.TurnID{2}, Kind: "message", Data: &followup})
		if got := pressureProjection(next, &warm, true); len(got) < len(projected) || !reflect.DeepEqual(projected, got[:len(projected)]) {
			t.Fatalf("single short follow-up changed below-threshold batch %d", end/2)
		}
	}
	updated, err := json.Marshal(history)
	if err != nil || string(original) != string(updated) {
		t.Fatal("projection mutated canonical history")
	}
}

func BenchmarkContextPressureMixedToolReplay(b *testing.B) {
	for _, size := range []struct {
		name  string
		bytes int
	}{{"medium", 6 * 1024}, {"large", 26 * 1024}} {
		history := mixedPressureHistory(mixedPressureResults(size.bytes))
		followup := canonical.NewMessage(canonical.User, canonical.Text(0, "What did you find?"))
		history = append(history, canonical.Record{ID: "short-followup", Turn: protocol.TurnID{2}, Kind: "message", Data: &followup})
		for _, mode := range []struct {
			name     string
			pressure bool
		}{{"unshortened", false}, {"pressure", true}} {
			b.Run(size.name+"/"+mode.name, func(b *testing.B) {
				var cache output.Cache
				var projected []canonical.Record
				b.ReportAllocs()
				for b.Loop() {
					projected = pressureProjection(history, &cache, mode.pressure)
				}
				var bodyBytes int
				var bodyTokens uint64
				for _, result := range pressureStoredResults(projected) {
					bodyBytes += len(result.Result.Value)
					bodyTokens += tokens.Text(string(result.Result.Value))
				}
				b.ReportMetric(float64(bodyBytes), "tool-bytes")
				b.ReportMetric(float64(bodyTokens), "tool-tokens")
				b.ReportMetric(float64(tokens.Chat("", projected)), "prompt-tokens")
			})
		}
	}
}
