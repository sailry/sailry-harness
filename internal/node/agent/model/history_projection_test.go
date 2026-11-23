package model

import (
	"encoding/json"
	"reflect"
	"strconv"
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

func TestToolFailureRequestIncludesReadableDiagnosticsAndKeepsRawFallback(t *testing.T) {
	provider := protocol.DefaultConversationProvider()
	provider.Api = protocol.ConversationModelApiChatCompletions
	binding := Binding{Provider: provider, Config: protocol.DefaultSessionConfig()}
	const reason = "Review is unavailable"
	for _, row := range []struct {
		name, body, want, omitted string
		format                    canonical.ToolFormat
		status                    canonical.ToolStatus
	}{
		{"runtime", `{"code":-32000,"message":"Review is unavailable","data":{"code":"unavailable","details":{"kind":"configuration","reason":"provider_disabled","credential":"private diagnostic"}}}`, "Tool error: " + reason + "\nCode: unavailable\nDetails: configuration, reason=provider_disabled", "private diagnostic", canonical.MCPError, canonical.ToolErrorStatus},
		{"invalid parameter", `{"code":"invalid_request","message":"Parameter is invalid","details":{"parameter":"limit","expected":"positive integer","message":"Parameter is invalid"}}`, "Tool error: Parameter is invalid\nCode: invalid_request\nDetails: parameter=limit, expected=positive integer", "", canonical.JSON, canonical.ToolErrorStatus},
		{"mcp", `{"isError":true,"content":[{"type":"text","text":"Private note","annotations":{"audience":["user"]}},{"type":"text","text":"{\"error\":{\"code\":\"conflict\",\"message\":\"Review is unavailable\"},\"details\":\"Run the review again\"}"}]}`, "Tool error: " + reason + "\nCode: conflict\nDetails: Run the review again", "Private note", canonical.MCP, canonical.ToolErrorStatus},
		{"unknown", `{"code":-32000,"data":{"status":"busy"}}`, `{"code":-32000,"data":{"status":"busy"}}`, "", canonical.MCPError, canonical.ToolErrorStatus},
		{"success", `{"message":"A business field","data":{"value":7}}`, `{"message":"A business field","data":{"value":7}}`, "", canonical.JSON, canonical.ToolSuccess},
	} {
		t.Run(row.name, func(t *testing.T) {
			result := canonical.ToolResponse{ID: "call", Name: "review_scope", Format: row.format, Status: row.status,
				Revision: strings.Repeat("a", 64), Result: canonical.Payload{Value: json.RawMessage(row.body)}}
			record := canonical.NewRecord(protocol.NewTurnID(), "message", canonical.NewMessage(canonical.User,
				canonical.Block{Kind: "tool_response", Data: &result}), time.Time{})
			records := []canonical.Record{record}
			cache := output.Cache{}
			calls := []struct {
				build   func(Binding, Request) (fantasy.Call, error)
				records []canonical.Record
			}{{BuildCall, records}, {BuildProjectedCall, cache.Project(records)}, {BuildProjectedCall, cache.Project(records)}}
			for _, check := range calls {
				call, err := check.build(binding, Request{Records: check.records})
				if err != nil {
					t.Fatal(err)
				}
				tool := call.Prompt[0].Content[0].(fantasy.ToolResultPart)
				if got := toolText(tool.Output); got != row.want {
					t.Fatalf("tool result = %q, want %q", got, row.want)
				}
			}
			projected, err := pluginRequest(binding, Request{Records: records})
			if err != nil {
				t.Fatal(err)
			}
			tool := projected.Messages[0].Data.(*protocol.PluginModelAdapterWireMessageToolPayload)
			wire := tool.Parts[0].Data.(*protocol.PluginModelAdapterWirePartJsonPayload).Value
			if tool.IsError != (row.status == canonical.ToolErrorStatus) || row.omitted != "" && strings.Contains(string(wire), row.omitted) {
				t.Fatalf("unexpected plugin adapter result: %s", wire)
			}
			if want, readable := strings.CutPrefix(row.want, "Tool error: "); readable && protocol.ToolErrorMessage(wire) != want {
				t.Fatalf("plugin adapter lost the readable diagnostic: %s", wire)
			}
			if string(result.Result.Value) != row.body {
				t.Fatal("model projection changed the canonical payload")
			}
		})
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

func TestAnsweredPairsReusedIDsByOccurrence(t *testing.T) {
	turn := protocol.NewTurnID()
	request := func() canonical.Record {
		return canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Block{
			Kind: "tool_request", Data: &canonical.ToolRequest{ID: "reused", Name: "read", Arguments: json.RawMessage(`{}`)},
		}), time.Time{})
	}
	response := func(value string) canonical.Block {
		return canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{ID: "reused", Name: "read", Result: canonical.Payload{Value: json.RawMessage(value)}}}
	}
	result := func(blocks ...canonical.Block) canonical.Record {
		return canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, blocks...), time.Time{})
	}
	records := []canonical.Record{request(), result(response(`1`), response(`2`)), request(), result(response(`3`)), request()}
	projected := Answered(records)
	first := projected[0].Message().Content[0].Data.(*canonical.ToolRequest).ID
	firstResult := projected[1].Message().Content
	second := projected[2].Message().Content[0].Data.(*canonical.ToolRequest).ID
	secondResult := projected[3].Message().Content[0].Data.(*canonical.ToolResponse)
	if first != "reused" || len(firstResult) != 1 || firstResult[0].Data.(*canonical.ToolResponse).ID != first || second == first || secondResult.ID != second {
		t.Fatalf("history pairs lost their occurrences: %q, %v, %q, %+v", first, firstResult, second, secondResult)
	}
	if len(projected[4].Message().Content) != 0 || len(records[1].Message().Content) != 2 || records[2].Message().Content[0].Data.(*canonical.ToolRequest).ID != "reused" {
		t.Fatal("repair did not remove exactly the unpaired blocks or mutated stored history")
	}
	if again := Answered(projected); !reflect.DeepEqual(again, projected) {
		t.Fatal("a second projection changed established tool pairs")
	}
}

func TestBuildProjectedCallMatchesRawAudience(t *testing.T) {
	turn := protocol.NewTurnID()
	call := canonical.Block{Kind: "tool_request", Data: &canonical.ToolRequest{ID: "c", Name: "sample", Arguments: json.RawMessage(`{}`)}}
	result := canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{ID: "c", Name: "sample", Format: canonical.MCP, Status: canonical.ToolSuccess, Result: canonical.Payload{Value: json.RawMessage(`{"content":[{"type":"text","text":"secret-fixture","annotations":{"audience":["user"]}},{"type":"text","text":"allowed-fixture"}]}`)}}}
	hidden := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "hidden-message-fixture")), time.Time{})
	hidden.Metadata.Runtime = &canonical.Runtime{UserVisible: true, AgentVisible: false}
	records := []canonical.Record{
		canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, call), time.Time{}),
		canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, result), time.Time{}),
		hidden,
	}
	var cache output.Cache
	filtered := cache.Project(records)
	if len(filtered) != len(records) || filtered[2].ID != hidden.ID {
		t.Fatal("fixture must retain the hidden message after audience filtering")
	}
	binding := Binding{Provider: protocol.DefaultConversationProvider(), Config: protocol.DefaultSessionConfig()}
	for _, api := range []protocol.ConversationModelApi{protocol.ConversationModelApiChatCompletions, protocol.ConversationModelApiPlugin} {
		binding.Provider.Api = api
		if api == protocol.ConversationModelApiPlugin {
			selection := protocol.DefaultConversationPresetsBinding()
			selection.Adapter = &protocol.PluginModelAdapterBinding{Capabilities: protocol.PluginModelAdapterCapabilities{Tools: true}}
			capture := &selection
			binding.Provider.Binding = &capture
		}
		raw, err := BuildCall(binding, Request{Records: records})
		if err != nil {
			t.Fatal(err)
		}
		projected, err := BuildProjectedCall(binding, Request{Records: filtered})
		if err != nil {
			t.Fatal(err)
		}
		var source, actual any = raw.Prompt, projected.Prompt
		if api == protocol.ConversationModelApiPlugin {
			source = raw.ProviderOptions[pluginRequestKey]
			actual = projected.ProviderOptions[pluginRequestKey]
		}
		body, err := json.Marshal(actual)
		if err != nil {
			t.Fatal(err)
		}
		if !reflect.DeepEqual(source, actual) || strings.Contains(string(body), "secret-fixture") || strings.Contains(string(body), "hidden-message-fixture") || !strings.Contains(string(body), "allowed-fixture") {
			t.Fatalf("%s raw/projected output diverged or contained wrong audience: %s", api, body)
		}
	}
}

func TestVisibleAnsweredRecordsMatchesLegacyRequests(t *testing.T) {
	turn := protocol.NewTurnID()
	message := func(role canonical.Role, blocks ...canonical.Block) canonical.Record {
		return canonical.NewRecord(turn, "message", canonical.NewMessage(role, blocks...), time.Time{})
	}
	compaction := func(boundary canonical.Record, text string) canonical.Record {
		return canonical.NewRecord(turn, "compaction", &canonical.Compaction{Boundary: boundary.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, text))}, time.Time{})
	}
	visibility := func(record canonical.Record, visible bool) canonical.Record {
		change := canonical.NewRecord(turn, "operation", &canonical.Operation{Kind: "visibility", Data: &canonical.Visibility{Message: record.ID, AgentVisible: visible}}, time.Time{})
		change.Metadata.Runtime = &canonical.Runtime{AgentVisible: false}
		return change
	}
	request := func(id string) canonical.Record {
		return message(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: &canonical.ToolRequest{ID: id, Name: "sample", Arguments: json.RawMessage(`{}`)}})
	}
	result := func(id, text string) canonical.Record {
		return message(canonical.User, canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{
			ID: id, Name: "sample", Format: canonical.MCP, Status: canonical.ToolSuccess, Revision: strings.Repeat("a", 64),
			Result: canonical.Payload{Value: json.RawMessage(`{"content":[{"type":"text","text":"` + text + `"},{"type":"text","text":"user-only content","annotations":{"audience":["user"]}}]}`)},
		}})
	}
	covered := request("covered-call")
	previous := compaction(covered, "obsolete handoff")
	revealed := message(canonical.User, canonical.Text(0, "revealed tail"))
	revealed.Metadata.Runtime = &canonical.Runtime{AgentVisible: false}
	orphan := result("covered-call", "covered result")
	first, second := request("reused-call"), request("reused-call")
	firstResult, secondResult := result("reused-call", "first result"), result("reused-call", "second result")
	unanswered, hidden := request("hidden-pair"), result("hidden-pair", "hidden result")
	latest := compaction(previous, "current handoff")
	records := []canonical.Record{covered, previous, revealed, orphan, first, firstResult, second, secondResult, unanswered, hidden, latest, visibility(revealed, true), visibility(hidden, false)}
	before, err := json.Marshal(records)
	if err != nil {
		t.Fatal(err)
	}
	prepared := VisibleAnsweredRecords(records)
	legacy := Answered(VisibleRecords(records))
	if !reflect.DeepEqual(prepared, legacy) || !reflect.DeepEqual(VisibleAnsweredRecords(prepared), prepared) {
		t.Fatal("combined visibility and repair changed the projected records")
	}
	want := []canonical.Record{latest, revealed, orphan, first, firstResult, second, secondResult, unanswered}
	if len(prepared) != len(want) {
		t.Fatalf("prepared history contains %d records, want %d", len(prepared), len(want))
	}
	for i, record := range prepared {
		if record.ID != want[i].ID {
			t.Fatalf("record %d = %s, want %s", i, record.ID, want[i].ID)
		}
	}
	raw := Answered(records)
	if len(raw) != len(records) || !reflect.DeepEqual(raw[0], covered) || !reflect.DeepEqual(raw[9], hidden) {
		t.Fatal("Answered changed records outside the visible history")
	}
	var legacyCache, preparedCache output.Cache
	legacy = legacyCache.Project(legacy)
	prepared = preparedCache.Project(prepared)
	for _, api := range []protocol.ConversationModelApi{protocol.ConversationModelApiChatCompletions, protocol.ConversationModelApiPlugin} {
		t.Run(string(api), func(t *testing.T) {
			binding := Binding{Provider: protocol.DefaultConversationProvider(), Config: protocol.DefaultSessionConfig()}
			binding.Provider.Api = api
			buildRaw := BuildCall
			if api == protocol.ConversationModelApiPlugin {
				selection := protocol.DefaultConversationPresetsBinding()
				selection.Adapter = &protocol.PluginModelAdapterBinding{Capabilities: protocol.PluginModelAdapterCapabilities{Tools: true}}
				capture := &selection
				binding.Provider.Binding = &capture
				buildRaw = BuildPluginCall
			}
			actual, err := BuildVisibleProjectedCall(binding, Request{Records: prepared})
			if err != nil {
				t.Fatal(err)
			}
			for _, baseline := range []struct {
				build   func(Binding, Request) (fantasy.Call, error)
				records []canonical.Record
			}{{BuildProjectedCall, legacy}, {buildRaw, raw}} {
				want, err := baseline.build(binding, Request{Records: baseline.records})
				if err != nil || !reflect.DeepEqual(actual, want) {
					t.Fatalf("prepared and baseline requests diverged: %v", err)
				}
			}
			var payload any = actual.Prompt
			if api == protocol.ConversationModelApiPlugin {
				payload = actual.ProviderOptions[pluginRequestKey]
			}
			encoded, err := json.Marshal(payload)
			if err != nil {
				t.Fatal(err)
			}
			body := string(encoded)
			for _, text := range []string{"current handoff", "revealed tail", "first result", "second result"} {
				if !strings.Contains(body, text) {
					t.Fatalf("request omitted %q: %s", text, body)
				}
			}
			for _, text := range []string{"obsolete handoff", "covered-call", "hidden-pair", "user-only content"} {
				if strings.Contains(body, text) {
					t.Fatalf("request included %q: %s", text, body)
				}
			}
			if strings.Count(body, "reused-call") != 2 || strings.Count(body, canonical.ToolCallOccurrenceID(second.ID, 0)) != 2 {
				t.Fatalf("request did not retain both tool pairs with distinct IDs: %s", body)
			}
		})
	}
	after, err := json.Marshal(records)
	if err != nil || string(before) != string(after) {
		t.Fatal("request assembly mutated canonical history")
	}
}

func TestAnthropicParallelToolCoalescingAndBreakpoints(t *testing.T) {
	turn := protocol.NewTurnID()
	makeMsg := func(role canonical.Role, blocks ...canonical.Block) canonical.Record {
		msg := canonical.NewMessage(role, blocks...)
		return canonical.NewRecord(turn, "message", &msg, time.Time{})
	}
	user := makeMsg(canonical.User, canonical.Text(0, "Inspect 12 files"))
	var reqBlocks, resBlocks []canonical.Block
	for i := range 12 {
		id := "call-" + strconv.Itoa(i)
		reqBlocks = append(reqBlocks, canonical.Block{Index: uint64(i), Kind: "tool_request", Data: &canonical.ToolRequest{ID: id, Name: "read_file", Arguments: json.RawMessage(`{"path":"a.go"}`)}})
		resBlocks = append(resBlocks, canonical.Block{Index: uint64(i), Kind: "tool_response", Data: &canonical.ToolResponse{
			ID: id, Name: "read_file", Revision: strings.Repeat("b", 64), Status: canonical.ToolSuccess, Format: canonical.JSON,
			Result: canonical.Payload{Value: json.RawMessage(`{"ok":true}`)},
		}})
	}
	assistant := makeMsg(canonical.Assistant, reqBlocks...)
	toolMsg := makeMsg(canonical.User, resBlocks...)

	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Api = protocol.NewProviderID(), protocol.ConversationModelApiAnthropic
	provider.Authentication = protocol.AuthenticationApiKey
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = provider.ID, "claude-opus-5-5"
	binding := Binding{Provider: provider, Config: config, Session: protocol.NewSessionID()}

	call, err := BuildCall(binding, Request{System: "Stable system", Records: []canonical.Record{user, assistant, toolMsg}})
	if err != nil {
		t.Fatal(err)
	}
	body, err := requestJSON(transportConfig{Kind: "anthropic", Model: config.Model}, call)
	if err != nil {
		t.Fatal(err)
	}
	messages, _ := body["messages"].([]any)
	if len(messages) != 3 {
		t.Fatalf("anthropic messages = %d, want 3 (user + assistant + single coalesced tool_result user message)", len(messages))
	}
	// Prior user message MUST hold the earlier conversation cache breakpoint.
	userBlocks := messages[0].(map[string]any)["content"].([]any)
	if userBlocks[len(userBlocks)-1].(map[string]any)["cache_control"] == nil {
		t.Fatal("prior user message lost its cache breakpoint across a parallel tool batch")
	}
	// All 12 tool results must be coalesced into messages[2] with cache_control only on the final block.
	toolBlocks := messages[2].(map[string]any)["content"].([]any)
	if len(toolBlocks) != 12 {
		t.Fatalf("coalesced tool_result count = %d, want 12", len(toolBlocks))
	}
	for i, raw := range toolBlocks {
		block := raw.(map[string]any)
		hasCache := block["cache_control"] != nil
		if hasCache != (i == len(toolBlocks)-1) {
			t.Fatalf("tool_result[%d] cache_control=%v, want %v", i, hasCache, i == len(toolBlocks)-1)
		}
	}
}

func TestGeminiPreservesThoughtSignatureAcrossTurns(t *testing.T) {
	turn1, turn2 := protocol.NewTurnID(), protocol.NewTurnID()
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Api = protocol.NewProviderID(), protocol.ConversationModelApiGemini
	provider.Authentication = protocol.AuthenticationApiKey
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = provider.ID, "gemini-3-pro"
	binding := Binding{Provider: provider, Config: config, Session: protocol.NewSessionID()}
	scope := Scope(binding)

	sig := "c2lnbmVkLXRob3VnaHQ="
	u1Msg := canonical.NewMessage(canonical.User, canonical.Text(0, "First question"))
	u1 := canonical.NewRecord(turn1, "message", &u1Msg, time.Time{})
	a1Msg := canonical.NewMessage(canonical.Assistant,
		canonical.Block{Index: 0, Kind: "thinking", Data: &canonical.Thinking{Text: "Plan", Signature: &sig}},
		canonical.Text(1, "First answer"),
	)
	a1 := canonical.NewRecord(turn1, "message", &a1Msg, time.Time{})
	a1.Metadata.Execution = &canonical.Execution{Source: &scope}
	u2Msg := canonical.NewMessage(canonical.User, canonical.Text(0, "Second question"))
	u2 := canonical.NewRecord(turn2, "message", &u2Msg, time.Time{})

	call, err := BuildCall(binding, Request{System: "Stable", Records: []canonical.Record{u1, a1, u2}})
	if err != nil {
		t.Fatal(err)
	}
	body, err := requestJSON(transportConfig{Kind: "google", Model: config.Model}, call)
	if err != nil {
		t.Fatal(err)
	}
	raw, _ := json.Marshal(body)
	if !strings.Contains(string(raw), sig) {
		t.Fatalf("prior turn Gemini thoughtSignature was stripped on turn 2: %s", raw)
	}
}

func BenchmarkModelHistoryProjection(b *testing.B) {
	const count, tail = 4096, 256
	message := canonical.NewMessage(canonical.User, canonical.Text(0, "Recorded progress"))
	records := make([]canonical.Record, count+1)
	for i := range count {
		records[i] = canonical.Record{ID: strconv.Itoa(i), Kind: "message", Data: &message}
	}
	records[count] = canonical.Record{ID: "summary", Kind: "compaction", Data: &canonical.Compaction{
		Boundary: records[count-tail-1].ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Rolling handoff")),
	}}
	for _, entry := range []struct {
		name    string
		project func([]canonical.Record) []canonical.Record
	}{
		{"legacy", func(records []canonical.Record) []canonical.Record {
			return VisibleRecords(Answered(VisibleRecords(records)))
		}},
		{"prepared", VisibleAnsweredRecords},
	} {
		b.Run(entry.name, func(b *testing.B) {
			b.ReportAllocs()
			for b.Loop() {
				if len(entry.project(records)) != tail+1 {
					b.Fatal("projection lost the summary or retained tail")
				}
			}
		})
	}
}
