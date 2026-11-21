package engine

import (
	"bytes"
	"encoding/json"
	"fmt"
	"reflect"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/output"
	"github.com/sailry/sailry-harness/internal/node/agent/tokens"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func pressureExchange(step int, input uint64, path string) []canonical.Record {
	call := canonical.ToolRequest{ID: fmt.Sprintf("call-%d", step), Name: "read_file", Arguments: json.RawMessage(fmt.Sprintf(`{"path":%q}`, path))}
	if step == 1 {
		call.ID = "call-0" // Stored histories can reuse a provider call ID.
	}
	page := protocol.FileLines{Path: path, Revision: strings.Repeat("f", 64)}
	for row := 1; row <= 160; row++ {
		line := fmt.Sprintf("func item%03d() string { return %q }\n", row, path)
		page.Lines = append(page.Lines, protocol.FileLine{Number: uint64(row), Text: line})
		page.Size += uint64(len(line))
	}
	reply := canonical.NewMessage(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: &call})
	finish, purpose, window := canonical.Completed, canonical.PurposeMain, uint32(64000)
	reply.Finish, reply.Usage = &finish, &canonical.Usage{Input: &input}
	observation := protocol.DefaultConversationObservationRequest()
	turn := protocol.TurnID{1}
	observation.ID, observation.Turn, observation.Model, observation.ContextLimit = fmt.Sprintf("request-%d", step), turn, "fixture", &window
	result := canonical.NewMessage(canonical.User, responseBlock(0, call, ToolResult{Value: map[string]any{"kind": "file_lines", "data": page}}))
	return []canonical.Record{
		{ID: fmt.Sprintf("assistant-%d", step), Turn: turn, Kind: "message", Data: &reply, Metadata: canonical.Metadata{Execution: &canonical.Execution{Purpose: &purpose, Observation: &observation}}},
		{ID: fmt.Sprintf("result-%d", step), Turn: turn, Kind: "message", Data: &result},
	}
}

func pressureProjection(history []canonical.Record, cache *output.Cache, canRead bool) []canonical.Record {
	projected := model.VisibleAnsweredRecords(history)
	var tools []fantasy.FunctionTool
	if canRead {
		tools = []fantasy.FunctionTool{{Name: "read_tool_output"}}
	}
	projectContextPressure(projected, history, tools)
	return cache.Project(projected)
}

func TestContextPressureToolHeavyHistory(t *testing.T) {
	var history []canonical.Record
	for i := range 7 {
		input := uint64(31000)
		if i == 6 {
			input = 32000
		}
		history = append(history, pressureExchange(i, input, fmt.Sprintf("pkg/part%d.go", i))...)
	}
	original, err := json.Marshal(history)
	if err != nil {
		t.Fatal(err)
	}
	var cache, ordinary output.Cache
	below := history[:12]
	if !reflect.DeepEqual(pressureProjection(below, &cache, true), ordinary.Project(model.VisibleAnsweredRecords(below))) {
		t.Fatal("lower usage changed a completed tool exchange")
	}
	before := ordinary.Project(model.VisibleAnsweredRecords(history))
	if !reflect.DeepEqual(before, pressureProjection(history, &cache, false)) {
		t.Fatal("results shortened without a declared recovery tool")
	}
	after := pressureProjection(history, &cache, true)
	result := func(r canonical.Record) *canonical.ToolResponse {
		return r.Message().Content[0].Data.(*canonical.ToolResponse)
	}
	var oldBytes, newBytes int
	for i := 0; i < len(history); i += 2 {
		if !reflect.DeepEqual(before[i], after[i]) || result(after[i+1]).ID != result(before[i+1]).ID {
			t.Fatal("pressure changed tool call/result pairing")
		}
		oldBytes += len(result(before[i+1]).Result.Value)
		newBytes += len(result(after[i+1]).Result.Value)
		if i >= 12 {
			if !reflect.DeepEqual(before[i+1], after[i+1]) {
				t.Fatal("pressure changed the latest tool batch")
			}
			continue
		}
		var receipt struct {
			Processed string                             `json:"processed"`
			Output    protocol.ToolOutputOutputReference `json:"output"`
		}
		source := result(history[i+1])
		if err := json.Unmarshal(result(after[i+1]).Result.Value, &receipt); err != nil || receipt.Processed != "file_lines" || receipt.Output.Call != source.ID || receipt.Output.Revision != source.Revision {
			t.Fatalf("missing immutable source reference for %s", history[i+1].ID)
		}
	}
	oldTokens, newTokens := tokens.Chat("", before), tokens.Chat("", after)
	t.Logf("synthetic 7-batch history: projected tool result JSON %d -> %d bytes; local o200k chat estimate %d -> %d tokens", oldBytes, newBytes, oldTokens, newTokens)
	if newTokens*2 >= oldTokens || newBytes*2 >= oldBytes {
		t.Fatal("old tool pages still dominate the projected request")
	}
	updated, err := json.Marshal(history)
	if err != nil || !bytes.Equal(updated, original) {
		t.Fatal("pressure changed the stored history")
	}
	var reloaded []canonical.Record
	if err := json.Unmarshal(original, &reloaded); err != nil {
		t.Fatal(err)
	}
	for _, next := range []struct{ step, input int }{{7, 1000}, {8, 60000}} {
		reloaded = append(reloaded, pressureExchange(next.step, uint64(next.input), "pkg/part0.go")...)
		warm := pressureProjection(reloaded, &cache, true)
		cold := pressureProjection(reloaded, new(output.Cache), true)
		if !reflect.DeepEqual(warm, cold) || !reflect.DeepEqual(after, cold[:len(after)]) {
			t.Fatal("record replay or a later usage report moved the original boundary")
		}
		if next.step == 7 && !bytes.Contains(result(cold[len(cold)-1]).Result.Value, []byte("func item001")) {
			t.Fatal("a shortened page incorrectly suppressed an explicitly repeated read")
		}
	}
}

func TestContextPressureIgnoresObservationsBeforeCheckpoint(t *testing.T) {
	first := canonical.NewMessage(canonical.User, canonical.Text(0, "Task"))
	history := []canonical.Record{{ID: "first", Turn: protocol.TurnID{1}, Kind: "message", Data: &first}}
	history = append(history, pressureExchange(0, 1000, "one.go")...)
	history = append(history, pressureExchange(1, 40000, "two.go")...)
	history = append(history, canonical.Record{ID: "summary", Turn: protocol.TurnID{1}, Kind: "compaction", Data: &canonical.Compaction{
		Boundary: "first", Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Retained evidence")),
	}})
	baseline := new(output.Cache).Project(model.VisibleAnsweredRecords(history))
	if !reflect.DeepEqual(pressureProjection(history, new(output.Cache), true), baseline) {
		t.Fatal("old pressure shortened the tail retained by a new checkpoint")
	}
}

func TestContextPressureRetainedTailAfterCheckpoint(t *testing.T) {
	for _, test := range []struct {
		name     string
		retained int
		input    uint64
		shorten  bool
	}{
		{"new window pressure", 2, 32000, true},
		{"new byte pressure", 12, 1000, true},
		{"below threshold with hidden pressure", 2, 1000, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			history := pressureExchange(0, 60000, "covered.go")
			boundary := history[len(history)-1].ID
			var retainedBytes int
			for step := 1; step <= test.retained; step++ {
				exchange := pressureExchange(step, 40000, fmt.Sprintf("retained-%d.go", step))
				result := exchange[1].Message().Content[0].Data.(*canonical.ToolResponse)
				retainedBytes += min(len(result.Result.Value), output.MaxBytes)
				history = append(history, exchange...)
			}
			history = append(history, canonical.Record{ID: "summary", Turn: protocol.TurnID{1}, Kind: "compaction", Data: &canonical.Compaction{
				Boundary: boundary, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Retained evidence")),
			}})
			var hiddenBytes int
			for step := test.retained + 1; step <= test.retained+12; step++ {
				exchange := pressureExchange(step, 60000, fmt.Sprintf("hidden-%d.go", step))
				result := exchange[1].Message().Content[0].Data.(*canonical.ToolResponse)
				hiddenBytes += min(len(result.Result.Value), output.MaxBytes)
				for i := range exchange {
					exchange[i].Metadata.Runtime = &canonical.Runtime{AgentVisible: false}
				}
				history = append(history, exchange...)
			}
			if hiddenBytes < toolOutputPressureBytes {
				t.Fatal("hidden fixture does not cross the byte threshold")
			}
			var cache output.Cache
			baseline := new(output.Cache).Project(model.VisibleAnsweredRecords(history))
			if !reflect.DeepEqual(pressureProjection(history, &cache, true), baseline) {
				t.Fatal("checkpoint replay used old or hidden pressure")
			}

			newest := pressureExchange(test.retained+13, test.input, "newest.go")
			newestResult := newest[1].Message().Content[0].Data.(*canonical.ToolResponse)
			bytePressure := retainedBytes+min(len(newestResult.Result.Value), output.MaxBytes) >= toolOutputPressureBytes
			if bytePressure != (test.retained == 12) {
				t.Fatal("fixture does not isolate window and byte pressure")
			}
			history = append(history, newest...)
			original, err := json.Marshal(history)
			if err != nil {
				t.Fatal(err)
			}
			before := new(output.Cache).Project(model.VisibleAnsweredRecords(history))
			after := pressureProjection(history, &cache, true)
			if len(after) != 1+2*(test.retained+1) || len(after) != len(before) {
				t.Fatal("pressure changed the visible retained tail")
			}
			if !reflect.DeepEqual(after, pressureProjection(history, new(output.Cache), true)) {
				t.Fatal("checkpoint replay differs between warm and cold projections")
			}
			shortened := 0
			for i, record := range before {
				message := record.Message()
				if !test.shorten || message == nil || message.Role != canonical.User || record.ID == newest[1].ID {
					if !reflect.DeepEqual(record, after[i]) {
						t.Fatal("pressure changed the summary, a call, the newest batch, or below-threshold evidence")
					}
					continue
				}
				source := message.Content[0].Data.(*canonical.ToolResponse)
				result := after[i].Message().Content[0].Data.(*canonical.ToolResponse)
				var receipt struct {
					Processed string                             `json:"processed"`
					Output    protocol.ToolOutputOutputReference `json:"output"`
				}
				if err := json.Unmarshal(result.Result.Value, &receipt); err != nil || receipt.Processed != "file_lines" ||
					receipt.Output.Call != source.ID || receipt.Output.Revision != source.Revision || result.ID != source.ID {
					t.Fatal("new pressure did not retain a recoverable receipt for the old tail")
				}
				shortened++
			}
			if test.shorten && shortened != test.retained {
				t.Fatal("new pressure did not recover the entire visible retained tail")
			}
			if updated, err := json.Marshal(history); err != nil || !bytes.Equal(updated, original) {
				t.Fatal("pressure changed canonical or hidden history")
			}
		})
	}
}

func BenchmarkContextPressureHistoryReplay(b *testing.B) {
	session, turn := protocol.SessionID{1}, protocol.TurnID{1}
	page := protocol.DefaultConversationSearchPage()
	page.Session, page.Revision, page.NextBefore = session, 3, new(uint64(140))
	for i := range 72 {
		page.Matches = append(page.Matches, protocol.ConversationSearchMatch{
			Turn: turn, TurnSequence: 1, Entry: fmt.Sprintf("entry-%03d", i), Sequence: 140 + uint64(i), Author: "assistant",
			Snippet: strings.Repeat("Archived inspection at the south gate. ", 5),
		})
	}
	var history []canonical.Record
	for i := range 7 {
		id := fmt.Sprintf("search-%d", i)
		request := canonical.ToolRequest{ID: id, Name: "search_session", Arguments: json.RawMessage(fmt.Sprintf(`{"session":%q,"text":"inspection"}`, session.String()))}
		reply := canonical.NewMessage(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: &request})
		result := canonical.NewMessage(canonical.User, responseBlock(0, request, ToolResult{Value: page}))
		history = append(history, canonical.Record{ID: id, Turn: turn, Kind: "message", Data: &reply},
			canonical.Record{ID: id + "-output", Turn: turn, Kind: "message", Data: &result})
	}
	followup := canonical.NewMessage(canonical.User, canonical.Text(0, "What did the archive say?"))
	history = append(history, canonical.Record{ID: "follow-up", Turn: turn, Kind: "message", Data: &followup})
	for _, run := range []struct {
		name     string
		pressure bool
	}{{"baseline", false}, {"pressure", true}} {
		b.Run(run.name, func(b *testing.B) {
			var cache output.Cache
			var projected []canonical.Record
			b.ReportAllocs()
			for b.Loop() {
				projected = pressureProjection(history, &cache, run.pressure)
			}
			var size int
			for _, result := range pressureStoredResults(projected) {
				size += len(result.Result.Value)
			}
			b.ReportMetric(float64(size), "tool-bytes")
			b.ReportMetric(float64(tokens.Chat("", projected)), "prompt-tokens")
		})
	}
}
