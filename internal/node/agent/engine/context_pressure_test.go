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
