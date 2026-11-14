package model

import (
	"encoding/json"
	"reflect"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/output"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestVisibleRecordsCanBeProjectedAgain(t *testing.T) {
	turn := protocol.NewTurnID()
	message := func(text string) canonical.Record {
		return canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, text)), time.Time{})
	}
	old, revealed, hidden := message("Old history"), message("Revealed input"), message("Hidden input")
	revealed.Metadata.Runtime = &canonical.Runtime{AgentVisible: false}
	summary := canonical.NewRecord(turn, "compaction", &canonical.Compaction{Boundary: old.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Summary"))}, time.Time{})
	visibility := func(record canonical.Record, visible bool) canonical.Record {
		operation := canonical.NewRecord(turn, "operation", &canonical.Operation{Kind: "visibility", Data: &canonical.Visibility{Message: record.ID, AgentVisible: visible}}, time.Time{})
		operation.Metadata.Runtime = &canonical.Runtime{AgentVisible: false}
		return operation
	}
	records := []canonical.Record{old, revealed, hidden, summary, visibility(revealed, true), visibility(hidden, false)}
	once := VisibleRecords(records)
	if len(once) != 2 || once[0].ID != summary.ID || once[1].ID != revealed.ID {
		t.Fatalf("projection = %+v", once)
	}
	if twice := VisibleRecords(once); !reflect.DeepEqual(once, twice) {
		t.Fatal("reprojection lost the handoff or a visibility override")
	}
	if revealed.Metadata.Runtime.AgentVisible || !reflect.DeepEqual(once[1].Message(), revealed.Message()) {
		t.Fatal("projection mutated the original audience or replaced message content")
	}
}

func TestRollingSummaryRetainsTailAndResetsReadProjection(t *testing.T) {
	turn := protocol.NewTurnID()
	message := func(role canonical.Role, blocks ...canonical.Block) canonical.Record {
		return canonical.NewRecord(turn, "message", canonical.NewMessage(role, blocks...), time.Time{})
	}
	pair := func(id string) (canonical.Record, canonical.Record) {
		call := message(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: &canonical.ToolRequest{ID: id, Name: "read_file", Arguments: json.RawMessage(`{"path":"fixture.go"}`)}})
		result := message(canonical.User, canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{ID: id, Name: "read_file", Revision: strings.Repeat("a", 64), Status: canonical.ToolSuccess, Format: canonical.JSON,
			Result: canonical.Payload{Value: json.RawMessage(`{"kind":"file_lines","data":{"path":"fixture.go","lines":[{"number":1,"text":"retained line\n"}],"size":14,"revision":"file-revision","offset":0,"truncated":false}}`)}}})
		return call, result
	}
	oldCall, oldResult := pair("old")
	tailCall, tailResult := pair("tail")
	tail := message(canonical.User, canonical.Text(0, "Read the file again"))
	newest := message(canonical.User, canonical.Text(0, "Continue with this evidence"))
	summary := canonical.NewRecord(turn, "compaction", &canonical.Compaction{Boundary: oldResult.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Rolling handoff"))}, time.Time{})
	records := []canonical.Record{oldCall, oldResult, tail, tailCall, tailResult, summary, newest}
	before, err := json.Marshal(records)
	if err != nil {
		t.Fatal(err)
	}
	visible := VisibleRecords(records)
	want := []string{summary.ID, tail.ID, tailCall.ID, tailResult.ID, newest.ID}
	ids := func(records []canonical.Record) []string {
		var ids []string
		for _, record := range records {
			ids = append(ids, record.ID)
		}
		return ids
	}
	if !reflect.DeepEqual(ids(visible), want) || !reflect.DeepEqual(VisibleRecords(visible), visible) || !reflect.DeepEqual(Answered(visible), visible) {
		t.Fatal("rolling summary changed tail order, visibility or complete tool pairs")
	}
	var cache output.Cache
	warmed := cache.Project(Answered(records[:5]))
	if response := warmed[4].Message().Content[0].Data.(*canonical.ToolResponse); !strings.Contains(string(response.Result.Value), "are unchanged") {
		t.Fatal("fixture did not establish an earlier read")
	}
	projected := cache.Project(Answered(visible))
	if response := projected[3].Message().Content[0].Data.(*canonical.ToolResponse); strings.Contains(string(response.Result.Value), "are unchanged") || !strings.Contains(string(response.Result.Value), "retained line") {
		t.Fatal("a compacted read suppressed retained tail evidence")
	}
	provider := protocol.DefaultConversationProvider()
	provider.Api = protocol.ConversationModelApiChatCompletions
	binding := Binding{Provider: provider, Config: protocol.DefaultSessionConfig()}
	fullCall, err := BuildCall(binding, Request{Records: records})
	if err != nil {
		t.Fatal(err)
	}
	tailCallProjection, err := BuildCall(binding, Request{Records: visible})
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(fullCall.Prompt, tailCallProjection.Prompt) || len(fullCall.Prompt) != 5 || fullCall.Prompt[0].Content[0].(fantasy.TextPart).Text != "Rolling handoff" {
		t.Fatal("SDK request changed the summary and retained tail")
	}
	selection := protocol.DefaultConversationPresetsBinding()
	selection.Adapter = &protocol.PluginModelAdapterBinding{Capabilities: protocol.PluginModelAdapterCapabilities{Tools: true}}
	captured := &selection
	binding.Provider.Api, binding.Provider.Binding = protocol.ConversationModelApiPlugin, &captured
	fullPlugin, err := pluginRequest(binding, Request{Records: records})
	if err != nil {
		t.Fatal(err)
	}
	tailPlugin, err := pluginRequest(binding, Request{Records: visible})
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(fullPlugin.Messages, tailPlugin.Messages) || len(fullPlugin.Messages) != 5 || fullPlugin.Messages[0].Data.(*protocol.PluginModelAdapterWireMessageUserPayload).Parts[0].Data.(*protocol.PluginModelAdapterWirePartTextPayload).Text != "Rolling handoff" {
		t.Fatal("plugin request changed the summary and retained tail")
	}
	next := canonical.NewRecord(turn, "compaction", &canonical.Compaction{Boundary: tailResult.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Updated handoff"))}, time.Time{})
	if got := VisibleRecords(append(append([]canonical.Record(nil), records...), next)); !reflect.DeepEqual(ids(got), []string{next.ID, newest.ID}) {
		t.Fatal("a later rolling summary retained superseded history")
	}
	after, err := json.Marshal(records)
	if err != nil || string(before) != string(after) {
		t.Fatal("request projection mutated canonical records")
	}
}
