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

func BenchmarkWarmFileToolHistory(b *testing.B) {
	lines := make([]protocol.FileLine, 100)
	for i := range lines {
		lines[i] = protocol.FileLine{Number: uint64(i + 1), Text: strings.Repeat("abcdefgh", 12) + "\n"}
	}
	records := make([]canonical.Record, 32)
	for i := range records {
		path := fmt.Sprintf("file-%d.go", i)
		records[i] = resultRecord(path, map[string]any{"kind": "file_lines", "data": protocol.FileLines{Path: path, Revision: "test", Lines: lines}}, canonical.JSON)
	}
	var cache Cache
	cache.Project(records)
	b.ReportAllocs()
	for b.Loop() {
		cache.Project(records)
	}
}

func TestWarmProjectionKeepsCurrentResponseMetadata(t *testing.T) {
	record := resultRecord("tool", map[string]any{"body": strings.Repeat("payload", MaxBytes)}, canonical.JSON)
	original := onlyResponse(&record)
	var cache Cache
	first := cache.Project([]canonical.Record{record})
	firstResult := onlyResponse(&first[0])
	if firstResult.Format != canonical.MCP {
		t.Fatal("fixture did not require a projection")
	}
	firstResult.Result.Value[0] = '!'
	meta := map[string]json.RawMessage{"state": json.RawMessage(`"new"`)}
	original.Meta = &meta
	second := cache.Project([]canonical.Record{record})
	secondResult := onlyResponse(&second[0])
	if !json.Valid(secondResult.Result.Value) || secondResult.Meta == nil || string((*secondResult.Meta)["state"]) != `"new"` {
		t.Fatal("warm projection lost metadata or reused caller-owned bytes")
	}
	(*secondResult.Meta)["state"][1] = 'x'
	if string(meta["state"]) != `"new"` {
		t.Fatal("warm projection modified canonical metadata")
	}
	original.Result.Value = json.RawMessage(`{"changed":true}`)
	third := cache.Project([]canonical.Record{record})
	if got := onlyResponse(&third[0]); got.Format != canonical.JSON || string(got.Result.Value) != string(original.Result.Value) {
		t.Fatal("different payload reused a prior projection")
	}
}

func TestProjectionKeepsCurrentSlotsAligned(t *testing.T) {
	parts := []any{
		map[string]any{"type": "image", "data": nil, "annotations": map[string]any{"audience": []string{"user"}}},
		map[string]any{"type": "text", "text": strings.Repeat("x", MaxBytes*2)},
		map[string]any{"type": "text", "text": "later"},
		map[string]any{"type": "image", "data": nil},
	}
	record := resultRecord("slots", map[string]any{"content": parts, "structuredContent": map[string]any{"bytes": nil}}, canonical.MCP)
	result := onlyResponse(&record)
	for _, path := range []string{"/content/0/data", "/content/3/data", "/structuredContent/bytes"} {
		result.Result.Assets = append(result.Result.Assets, canonical.BinarySlot{Pointer: path, Encoding: canonical.Base64, Asset: canonical.AssetRef{Digest: "old"}})
	}
	raw := Audience(record)
	before := onlyResponse(&raw).Result.Assets
	if len(before) != 2 || before[0].Pointer != "/content/2/data" || before[1].Pointer != "/structuredContent/bytes" {
		t.Fatalf("filtered slots = %+v", before)
	}
	var cache Cache
	for _, digest := range []string{"cold", "warm"} {
		result.Result.Assets[1].Asset.Digest = digest
		projected := cache.Project([]canonical.Record{record})
		got := onlyResponse(&projected[0]).Result.Assets
		if len(got) != 1 || got[0].Pointer != "/content/1/data" || got[0].Asset.Digest != digest {
			t.Fatalf("%s slots = %+v", digest, got)
		}
		if result.Result.Assets[1].Pointer != "/content/3/data" {
			t.Fatal("source slots changed")
		}
	}
}

func TestUnchangedResultOwnsProjectedBytes(t *testing.T) {
	record := resultRecord("small", 42, canonical.JSON)
	var cache Cache
	for range 2 {
		projected := cache.Project([]canonical.Record{record})
		onlyResponse(&projected[0]).Result.Value[0] = '7'
		if string(onlyResponse(&record).Result.Value) != "42" {
			t.Fatal("projected result aliases canonical history")
		}
	}
}

func TestAudienceOwnsUnchangedToolOutput(t *testing.T) {
	record := resultRecord("plain", nil, canonical.JSON)
	original := onlyResponse(&record)
	original.Result.Value = json.RawMessage(`{ "value": "<file>" }`)
	metadata := map[string]json.RawMessage{"source": json.RawMessage(`"local"`)}
	original.Meta = &metadata

	projected := Audience(record)
	response := onlyResponse(&projected)
	if string(response.Result.Value) != `{"value":"\u003cfile\u003e"}` || response.Revision != original.Revision {
		t.Fatalf("unexpected projected response: %+v", response)
	}
	response.Result.Value[0] = '!'
	(*response.Meta)["source"][1] = '!'
	if string(original.Result.Value) != `{ "value": "<file>" }` || string(metadata["source"]) != `"local"` {
		t.Fatal("audience projection modified canonical output")
	}
}

func TestFilePreviewFitsUpTo48KiBAndPaginatesAbove(t *testing.T) {
	makeFile := func(count int) canonical.Record {
		lines := make([]protocol.FileLine, count)
		size := uint64(0)
		for i := range lines {
			text := fmt.Sprintf("const line%04d = %q\n", i+1, strings.Repeat("x", 64))
			lines[i] = protocol.FileLine{Number: uint64(i + 1), Text: text}
			size += uint64(len(text))
		}
		return resultRecord("file", map[string]any{
			"kind": "file_lines",
			"data": protocol.FileLines{Path: "tools.go", Lines: lines, Size: size, Revision: "rev-1", Offset: 0},
		}, canonical.JSON)
	}

	t.Run("fits_38kib_in_single_page", func(t *testing.T) {
		record := makeFile(420) // ~36 KiB raw text, > 32 KiB old limit, < 48 KiB new limit
		var cache Cache
		projected := cache.Project([]canonical.Record{record})
		value := onlyResponse(&projected[0])
		envelope := object(decode(value.Result.Value))
		bytes, lines := size(envelope)
		if bytes > MaxBytes || lines > MaxLines {
			t.Fatalf("projected file exceeded bounds: %d bytes, %d lines", bytes, lines)
		}
		text := projectedText(projected[0])
		if !strings.Contains(text, "Lines 1–420;") || !strings.Contains(text, "420\tconst line0420") {
			t.Fatalf("36 KiB file was paginated unexpectedly: tail=%q", text[max(0, len(text)-240):])
		}
		if strings.Contains(text, "Next file page:") || !strings.Contains(text, `"next_offset":null`) {
			t.Fatalf("unexpected pagination metadata: tail=%q", text[max(0, len(text)-240):])
		}
	})

	t.Run("paginates_60kib_cleanly_within_max_bytes", func(t *testing.T) {
		record := makeFile(720) // ~62 KiB raw text, > 48 KiB limit
		var cache Cache
		projected := cache.Project([]canonical.Record{record})
		value := onlyResponse(&projected[0])
		envelope := object(decode(value.Result.Value))
		bytes, lines := size(envelope)
		if bytes > MaxBytes || lines > MaxLines {
			t.Fatalf("paginated file exceeded bounds: %d bytes, %d lines", bytes, lines)
		}
		text := projectedText(projected[0])
		if !strings.Contains(text, "Next file page: read_file with offset=") || !strings.Contains(text, "do not use read_tool_output") {
			t.Fatalf("pagination guidance missing: tail=%q", text[max(0, len(text)-240):])
		}
		var receipt struct {
			Output     protocol.ToolOutputOutputReference `json:"output"`
			NextOffset *uint64                            `json:"next_offset"`
		}
		if err := json.Unmarshal([]byte(text[strings.LastIndexByte(text, '\n')+1:]), &receipt); err != nil || receipt.Output.Call != "file" || receipt.NextOffset == nil || *receipt.NextOffset == 0 {
			t.Fatalf("invalid pagination receipt: %q, err=%v", text[strings.LastIndexByte(text, '\n')+1:], err)
		}
	})
}
