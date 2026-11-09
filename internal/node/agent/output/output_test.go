package output

import (
	"encoding/json"
	"fmt"
	"reflect"
	"strings"
	"testing"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func resultRecord(call string, value any, format canonical.ToolFormat) canonical.Record {
	raw := json.RawMessage(encoded(value))
	hash := blake3.Sum256(raw)
	response := canonical.ToolResponse{ID: call, Name: "fixture", Status: canonical.ToolSuccess, Result: canonical.Payload{Value: raw}, Revision: fmt.Sprintf("%x", hash), Format: format}
	return canonical.NewRecord(protocol.NewTurnID(), "message", canonical.NewMessage(canonical.User, canonical.Block{Index: 0, Kind: "tool_response", Data: &response}), time.Time{})
}
func onlyResponse(record *canonical.Record) *canonical.ToolResponse {
	return response(&record.MutableMessage().Content[0])
}
func projectedText(record canonical.Record) string {
	response := onlyResponse(&record)
	if response.Format == canonical.JSON {
		return string(response.Result.Value)
	}
	envelope := object(decode(response.Result.Value))
	var texts []string
	for _, part := range array(envelope["content"]) {
		if field := textField(object(part)); field != nil {
			texts = append(texts, text(field["text"]))
		}
	}
	return strings.Join(texts, " ")
}
func TestOutgoingProjection(t *testing.T) {
	for _, row := range []struct{ name, body string }{{"line bound", strings.Repeat("row 中文🙂\n", 4000)}, {"byte bound", strings.Repeat("中文🙂", 10000)}} {
		t.Run(row.name, func(t *testing.T) {
			original := []canonical.Record{resultRecord("call", map[string]any{"body": row.body}, canonical.JSON)}
			before := encoded(original)
			cache := Cache{}
			projected := cache.Project(original)
			value := onlyResponse(&projected[0])
			if value.Format != canonical.MCP {
				t.Fatal("large result did not become a bounded text projection")
			}
			envelope := object(decode(value.Result.Value))
			bytes, lines := size(envelope)
			if bytes > MaxBytes || lines > MaxLines || !utf8.ValidString(projectedText(projected[0])) {
				t.Fatalf("bounds=%dbytes,%dlines", bytes, lines)
			}
			text := projectedText(projected[0])
			if !strings.Contains(text, "read_tool_output using output=") || !strings.Contains(text, onlyResponse(&original[0]).Revision) {
				t.Fatal("bounded result lost its immutable source reference")
			}
			if encoded(original) != before {
				t.Fatal("projection changed canonical original")
			}
			if !reflect.DeepEqual(cache.Project(original), projected) {
				t.Fatal("unchanged source projection is unstable")
			}
			again := Cache{}
			if !reflect.DeepEqual(again.Project(projected), projected) {
				t.Fatal("projecting a projection changed its text")
			}
		})
	}
	t.Run("mcp_text_lines_and_error", func(t *testing.T) {
		original := []canonical.Record{resultRecord("text", map[string]any{"content": []any{map[string]any{"type": "text", "text": strings.Repeat("line\n", 3000)}}}, canonical.MCP), resultRecord("error", map[string]any{"code": -32602, "message": strings.Repeat("error\n", 3000), "data": map[string]any{"detail": strings.Repeat("x", 60000)}}, canonical.MCPError)}
		before := encoded(original)
		cache := Cache{}
		projected := cache.Project(original)
		if lineCount(projectedText(projected[0])) > MaxLines {
			t.Fatal("MCP text exceeded line ceiling")
		}
		failure := object(decode(onlyResponse(&projected[1]).Result.Value))
		message := text(failure["message"])
		if len(message) > MaxBytes || lineCount(message) > MaxLines || failure["data"] != nil {
			t.Fatal("MCP error was not bounded independently")
		}
		if encoded(original) != before {
			t.Fatal("error projection changed canonical payload")
		}
	})
	t.Run("small_results_are_preserved", func(t *testing.T) {
		original := []canonical.Record{resultRecord("small", map[string]any{"kind": "business_record", "data": "unchanged"}, canonical.JSON)}
		cache := Cache{}
		if !reflect.DeepEqual(cache.Project(original), clone(original)) {
			t.Fatal("small unrecognized business result changed")
		}
	})
	t.Run("file_reads_visibility_and_prefix_cache", func(t *testing.T) {
		page := map[string]any{"kind": "file_lines", "data": map[string]any{"path": "fixture.go", "lines": []any{map[string]any{"number": 1, "text": "one\n"}, map[string]any{"number": 2, "text": "two\n"}}, "size": 8, "revision": "file-revision", "offset": 0, "truncated": false}}
		first, second := resultRecord("first", page, canonical.JSON), resultRecord("second", page, canonical.JSON)
		cache := Cache{}
		projected := cache.Project([]canonical.Record{first, second})
		if !strings.Contains(projectedText(projected[0]), "1\tone\n2\ttwo\n") || !strings.Contains(projectedText(projected[1]), "Lines 1–2 are unchanged") {
			t.Fatal("numbered read projection or suppression changed")
		}
		hidden := clone(first)
		runtime := canonical.DefaultRuntime()
		runtime.AgentVisible = false
		hidden.Metadata.Runtime = &runtime
		projected = cache.Project([]canonical.Record{hidden, second})
		if strings.Contains(projectedText(projected[1]), "are unchanged") {
			t.Fatal("hidden prior read suppressed visible content")
		}
		summary := canonical.NewRecord(first.Turn, "compaction", &canonical.Compaction{Boundary: first.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Summary")), Start: first.Created, End: first.Created}, first.Created)
		projected = cache.Project([]canonical.Record{first, summary, second})
		if strings.Contains(projectedText(projected[2]), "are unchanged") {
			t.Fatal("a compacted read suppressed current evidence")
		}
	})
	t.Run("audience_does_not_mutate_original", func(t *testing.T) {
		record := resultRecord("audience", map[string]any{"content": []any{map[string]any{"type": "text", "text": "Private note", "annotations": map[string]any{"audience": []string{"user"}}}, map[string]any{"type": "text", "text": "Model note", "annotations": map[string]any{"audience": []string{"assistant"}}}}}, canonical.MCP)
		message := record.MutableMessage()
		message.Content = append(message.Content, canonical.Block{Index: 1, Kind: "image", Data: &canonical.AssetContent{Asset: canonical.AssetRef{Digest: "fixture", Mime: "image/png", Annotations: json.RawMessage(`{"audience":["user"]}`)}}})
		message.PartCount = 2
		before := encoded(record)
		visible := Audience(record)
		if len(visible.MutableMessage().Content) != 1 || strings.Contains(projectedText(visible), "Private note") || !strings.Contains(projectedText(visible), "Model note") {
			t.Fatal("user-only content reached the model")
		}
		if encoded(record) != before {
			t.Fatal("audience filtering changed canonical original")
		}
	})
	t.Run("read_output_pages_do_not_escape_json_twice", func(t *testing.T) {
		original := resultRecord("page", map[string]any{"turn": protocol.NewTurnID(), "output": map[string]any{"call": "source", "revision": strings.Repeat("a", 64)}, "offset": 1, "total_bytes": 30, "next_offset": 20, "text": "{\"raw\":\"page\"}"}, canonical.JSON)
		cache := Cache{}
		projected := cache.Project([]canonical.Record{original})
		text := projectedText(projected[0])
		if !strings.Contains(text, "Result bytes 1–15 of 30\n{\"raw\":\"page\"}") || !strings.Contains(text, "read_tool_output with the same output and next_offset") {
			t.Fatalf("page preview=%q", text)
		}
	})
}
