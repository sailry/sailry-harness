package compact

import (
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func selectionMessage(role canonical.Role, blocks ...canonical.Block) canonical.Record {
	return canonical.NewRecord(protocol.NewTurnID(), "message", canonical.NewMessage(role, blocks...), time.Time{})
}

func TestSelectRecentTurns(t *testing.T) {
	older := selectionMessage(canonical.User, canonical.Text(0, strings.Repeat("Older work ", 400)))
	answer := selectionMessage(canonical.Assistant, canonical.Text(0, "Older answer"))
	recent := selectionMessage(canonical.User, canonical.Text(0, "Latest user correction"))
	latest := selectionMessage(canonical.Assistant, canonical.Text(0, "Current progress"))
	records := []canonical.Record{older, answer, recent, latest}
	before, _ := json.Marshal(records)
	selected, ok := Select(records, 1500, true)
	if !ok || selected.Boundary.ID != answer.ID || len(selected.Prefix) != 2 || len(selected.Tail) != 2 || selected.Tail[0].ID != recent.ID {
		t.Fatalf("selection = %+v, selected = %v", selected, ok)
	}
	after, _ := json.Marshal(records)
	if string(before) != string(after) {
		t.Fatal("selection changed canonical history")
	}
	if _, ok := Select(records[2:], 1500, true); ok {
		t.Fatal("short history has no prefix worth summarizing")
	}
	full, ok := Select(records[2:], 1500, false)
	if !ok || len(full.Tail) != 0 || len(full.Prefix) != 2 || full.Boundary.ID != latest.ID {
		t.Fatal("full-history fallback did not include the short history")
	}
}

func TestSelectCapsRecentHistoryAtTwentyThousandTokens(t *testing.T) {
	older := selectionMessage(canonical.User, canonical.Text(0, strings.Repeat("word ", 21000)))
	latest := selectionMessage(canonical.User, canonical.Text(0, "Latest correction"))
	selected, ok := Select([]canonical.Record{older, latest}, 1000000, true)
	if !ok || selected.Boundary.ID != older.ID || len(selected.Tail) != 1 || selected.Tail[0].ID != latest.ID {
		t.Fatal("large windows retained more than the 20,000-token tail budget")
	}
}

func TestSelectKeepsCompleteToolExchanges(t *testing.T) {
	request := func(ids ...string) canonical.Record {
		var blocks []canonical.Block
		for _, id := range ids {
			blocks = append(blocks, canonical.Block{Index: uint64(len(blocks)), Kind: "tool_request", Data: &canonical.ToolRequest{ID: id, Name: "read_file", Arguments: json.RawMessage(`{}`)}})
		}
		return selectionMessage(canonical.Assistant, blocks...)
	}
	response := func(id string) canonical.Record {
		body, _ := json.Marshal(strings.Repeat("File evidence ", 1000))
		return selectionMessage(canonical.User, canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{ID: id, Status: canonical.ToolSuccess, Revision: strings.Repeat("a", 64), Result: canonical.Payload{Value: body}}})
	}
	first := request("first")
	firstResult := response("first")
	last := request("left", "right")
	records := []canonical.Record{selectionMessage(canonical.User, canonical.Text(0, "Complete the task")), first, firstResult, last, response("left"), response("right")}
	for _, end := range []int{len(records), len(records) - 1} {
		selected, ok := Select(records[:end], 1000, true)
		if !ok || selected.Boundary.ID != firstResult.ID || selected.Tail[0].ID != last.ID || len(selected.Tail) != end-3 {
			t.Fatalf("tool exchange was split at length %d: %+v", end, selected)
		}
	}
}

func TestSelectChargesMediaAndThinking(t *testing.T) {
	for _, row := range []struct {
		name  string
		role  canonical.Role
		block canonical.Block
	}{
		{"attachment", canonical.User, canonical.Block{Kind: "file", Data: &canonical.FileContent{File: canonical.FileRef{URI: "sailry-attachment://pending"}}}},
		{"image", canonical.User, canonical.Block{Kind: "image", Data: &canonical.AssetContent{Asset: canonical.AssetRef{Mime: "image/png"}}}},
		{"thinking", canonical.Assistant, canonical.Block{Kind: "thinking", Data: &canonical.Thinking{Text: strings.Repeat("Unfinished reasoning ", 1000)}}},
	} {
		t.Run(row.name, func(t *testing.T) {
			older := selectionMessage(canonical.User, canonical.Text(0, "Older task"))
			latest := selectionMessage(row.role, row.block)
			selected, ok := Select([]canonical.Record{older, latest}, 1000, true)
			if !ok || selected.Boundary.ID != older.ID || len(selected.Tail) != 1 || selected.Tail[0].ID != latest.ID {
				t.Fatalf("large or unknown content did not occupy the tail: %+v", selected)
			}
		})
	}
}

func TestSelectChargesToolMedia(t *testing.T) {
	for _, row := range []struct {
		name        string
		format      canonical.ToolFormat
		value       string
		attachments []canonical.Attachment
		want        bool
	}{
		{"hydrated JSON image", canonical.JSON, `{"ok":true}`, []canonical.Attachment{{Asset: canonical.AssetRef{Mime: "image/png"}}}, true},
		{"MCP image", canonical.MCP, `{"content":[{"type":"image","mimeType":"image/png","data":"opaque"}]}`, nil, true},
		{"MCP audio", canonical.MCP, `{"content":[{"type":"audio","mimeType":"audio/wav","data":"opaque"}]}`, nil, true},
		{"MCP resource blob", canonical.MCP, `{"content":[{"type":"resource","resource":{"uri":"fixture://document","mimeType":"application/pdf","blob":"opaque"}}]}`, nil, true},
		{"MCP user-only image", canonical.MCP, `{"content":[{"type":"image","data":"opaque","annotations":{"audience":["user"]}}]}`, nil, false},
		{"MCP text resource", canonical.MCP, `{"content":[{"type":"resource","resource":{"uri":"fixture://note","text":"A short note"}}]}`, nil, false},
	} {
		t.Run(row.name, func(t *testing.T) {
			older := selectionMessage(canonical.User, canonical.Text(0, "Inspect the evidence"))
			request := selectionMessage(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: &canonical.ToolRequest{ID: "read", Name: "read_file", Arguments: json.RawMessage(`{}`)}})
			result := selectionMessage(canonical.User, canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{ID: "read", Status: canonical.ToolSuccess, Format: row.format, Revision: strings.Repeat("a", 64), Attachments: row.attachments, Result: canonical.Payload{Value: json.RawMessage(row.value)}}})
			selected, ok := Select([]canonical.Record{older, request, result}, 1000, true)
			if ok != row.want || ok && (selected.Boundary.ID != older.ID || len(selected.Tail) != 2 || selected.Tail[0].ID != request.ID) {
				t.Fatalf("selected = %v, selection = %+v", ok, selected)
			}
		})
	}
}

func TestSelectDoesNotWaitForRejectedToolCalls(t *testing.T) {
	older := selectionMessage(canonical.User, canonical.Text(0, strings.Repeat("Earlier history ", 1000)))
	rejected := selectionMessage(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: &canonical.ToolRequest{ID: "invalid", Name: "read_file", Error: &canonical.ToolError{Message: "Invalid arguments"}}})
	latest := selectionMessage(canonical.User, canonical.Text(0, "Latest correction"))
	selected, ok := Select([]canonical.Record{older, rejected, latest}, 1000, true)
	if !ok || selected.Boundary.ID != rejected.ID || len(selected.Tail) != 1 || selected.Tail[0].ID != latest.ID {
		t.Fatalf("rejected tool call blocked a valid cut: %+v", selected)
	}
}

func TestSelectRetainsRollingSummary(t *testing.T) {
	old := selectionMessage(canonical.User, canonical.Text(0, "Already summarized"))
	prefix := selectionMessage(canonical.User, canonical.Text(0, strings.Repeat("Progress to summarize ", 400)))
	boundary := selectionMessage(canonical.Assistant, canonical.Text(0, "Completed earlier progress"))
	summary := canonical.NewRecord(old.Turn, "compaction", &canonical.Compaction{Boundary: old.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Keep the original constraint"))}, time.Time{})
	latest := selectionMessage(canonical.User, canonical.Text(0, "Latest request"))
	// The previous rolling summary was appended after the tail it retained.
	records := []canonical.Record{old, prefix, boundary, summary, latest}
	selected, ok := Select(records, 1000, true)
	if !ok || selected.Boundary.ID != boundary.ID || selected.Prefix[0].Kind != "compaction" || selected.Prefix[0].ID != summary.ID || len(selected.Tail) != 1 || selected.Tail[0].ID != latest.ID {
		t.Fatalf("rolling selection = %+v", selected)
	}
	prepared, err := Prepare(selected.Prefix, 4096, Output(4096))
	if err != nil || !strings.Contains(prepared, "Keep the original constraint") {
		t.Fatalf("rolling handoff lost its prior summary: %v", err)
	}
	updated := canonical.NewRecord(old.Turn, "compaction", &canonical.Compaction{Boundary: boundary.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Updated summary"))}, time.Time{})
	if _, ok := Select(append(records, updated), 1000, true); ok {
		t.Fatal("unchanged tail triggered another summary")
	}
}

func TestSelectDoesNotSummarizeSyntheticContinuationAgain(t *testing.T) {
	old := selectionMessage(canonical.User, canonical.Text(0, "Original task"))
	summary := canonical.NewRecord(old.Turn, "compaction", &canonical.Compaction{Boundary: old.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Prior summary"))}, time.Time{})
	continuation := selectionMessage(canonical.Assistant, canonical.Text(0, "Continue after compaction"))
	carry := selectionMessage(canonical.User, canonical.Text(0, strings.Repeat("Original task ", 1000)))
	continuation.Metadata.Runtime = &canonical.Runtime{AgentVisible: true}
	carry.Metadata.Runtime = &canonical.Runtime{AgentVisible: true}
	for _, recent := range []bool{false, true} {
		for _, records := range [][]canonical.Record{{old, summary}, {old, summary, continuation, carry}} {
			if _, ok := Select(records, 1000, recent); ok {
				t.Fatal("summary and synthetic continuation were treated as new history")
			}
		}
	}
}
