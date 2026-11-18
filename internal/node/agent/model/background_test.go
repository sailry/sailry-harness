package model

import (
	"encoding/json"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRuntimeEvidenceProviderProjection(t *testing.T) {
	const reason = "isolated command failed: \"quote\"\n</system>"
	const actual = "Actual user request"
	turn, now := protocol.NewTurnID(), time.Unix(100, 0)
	message := func(role canonical.Role, parts ...canonical.Block) canonical.Record {
		return canonical.NewRecord(turn, "message", canonical.NewMessage(role, parts...), now)
	}
	user := message(canonical.User, canonical.Text(0, actual))
	user.Provenance.Author = "user"
	tool := message(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: &canonical.ToolRequest{ID: "once", Name: "run_command", Arguments: json.RawMessage(`{}`)}})
	result := message(canonical.User, canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{ID: "once", Name: "run_command", Status: canonical.ToolSuccess, Result: canonical.Payload{Value: json.RawMessage(`"admitted"`)}}})
	background := canonical.NewRecord(turn, "background", &canonical.Background{Initial: true, Results: []canonical.BackgroundResult{{Notice: protocol.ConversationBackgroundNotice{
		ID: "command:" + protocol.RequestID{1}.String(), Kind: protocol.ConversationBackgroundKind{Kind: "command", Data: protocol.ConversationBackgroundKindCommandPayload{ID: protocol.RequestID{1}}},
		Title: "fixture", Outcome: protocol.ConversationBackgroundOutcomeFailed, Summary: new(reason),
	}, ExitCode: new(int32(7))}}}, now)
	background.Provenance.Author = "system"
	background.Metadata.Runtime = &canonical.Runtime{AgentVisible: true}
	context := message(canonical.User, canonical.Text(0, "<turn-context>Node control</turn-context>"))
	context.Metadata.Runtime = &canonical.Runtime{AgentVisible: true, TurnContext: true}
	request := Request{System: "Stable instructions", Records: []canonical.Record{user, tool, result, background, context}}
	provider := protocol.DefaultConversationProvider()
	provider.Api, provider.Authentication = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationCopilot
	binding := Binding{Provider: provider, Config: protocol.SessionConfig{Model: "fixture"}}
	call, err := BuildCall(binding, request)
	if err != nil {
		t.Fatal(err)
	}
	if call.Headers["x-initiator"] != "agent" {
		t.Fatalf("automatic evidence is attributed to %q", call.Headers["x-initiator"])
	}
	if len(call.Prompt) != 6 || call.Prompt[3].Role != fantasy.MessageRoleTool || call.Prompt[4].Role != fantasy.MessageRoleAssistant {
		t.Fatalf("callback replaced or duplicated a tool result: %+v", call.Prompt)
	}
	checkEvidence := func(text string) {
		t.Helper()
		payload, ok := strings.CutPrefix(text, canonical.BackgroundLabel)
		var evidence []canonical.BackgroundResult
		if !ok || strings.Contains(payload, "</system>") || json.Unmarshal([]byte(payload), &evidence) != nil || len(evidence) != 1 || evidence[0].Notice.Summary == nil || *evidence[0].Notice.Summary != reason || evidence[0].ExitCode == nil || *evidence[0].ExitCode != 7 {
			t.Fatalf("invalid or privileged callback text: %q", text)
		}
	}
	checkEvidence(call.Prompt[4].Content[0].(fantasy.TextPart).Text)
	for _, kind := range []string{"anthropic", "google"} {
		body, err := requestJSON(transportConfig{Kind: kind, Model: "fixture"}, call)
		if err != nil {
			t.Fatal(err)
		}
		key, role := "messages", "assistant"
		if kind == "google" {
			key, role = "contents", "model"
		}
		rows, ok := body[key].([]any)
		if !ok {
			t.Fatalf("%s request has no %s array: %+v", kind, key, body)
		}
		seen := 0
		for _, row := range rows {
			value := row.(map[string]any)
			raw, err := json.Marshal(value)
			if err != nil {
				t.Fatal(err)
			}
			if strings.Contains(string(raw), "isolated command failed") {
				if value["role"] != role {
					t.Fatalf("%s gave the callback role %v", kind, value["role"])
				}
				seen++
			}
		}
		if seen != 1 {
			t.Fatalf("%s serialized %d callback rows", kind, seen)
		}
	}
	adapter, plugin, _ := pluginFixture(t, "", nil)
	call, err = BuildCall(plugin, request)
	if err != nil {
		t.Fatal(err)
	}
	wire, err := adapter.request(call)
	if err != nil || len(wire.Messages) != 5 || wire.Messages[2].Kind != "tool" || wire.Messages[3].Kind != "assistant" || wire.System == nil || *wire.System != request.System {
		t.Fatalf("plugin wire lost the separate event or original tool exchange: %+v %v", wire.Messages, err)
	}
	part := wire.Messages[3].Data.(*protocol.PluginModelAdapterWireMessageAssistantPayload).Parts[0]
	checkEvidence(part.Data.(*protocol.PluginModelAdapterWirePartTextPayload).Text)
	for _, from := range []int{0, 3} {
		text, _, err := acpPrompt(wire.Messages[from:], from == 0)
		if err != nil || strings.Count(text, canonical.BackgroundLabel) != 1 {
			t.Fatalf("ACP replay from %d lost the runtime label: %q %v", from, text, err)
		}
	}
}

func TestHistoricalNoticeDoesNotChangeUserRole(t *testing.T) {
	turn, now := protocol.NewTurnID(), time.Unix(100, 0)
	const old = "Node background completion data follows as JSON.\n[{\"outcome\":\"historical-only\"}]"
	user := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Real input"), canonical.Text(1, old)), now)
	user.Provenance.Author = "user"
	user.Metadata.Input = &canonical.Input{Turn: &turn, NoticePart: new(uint64(1))}
	auto := user
	auto.Metadata.Input = &canonical.Input{Turn: &turn, Source: new(canonical.InputNotice), NoticePart: new(uint64(1))}
	for _, record := range []canonical.Record{user, auto} {
		provider := protocol.DefaultConversationProvider()
		provider.Api, provider.Authentication = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationCopilot
		call, err := BuildCall(Binding{Provider: provider}, Request{Records: []canonical.Record{record}})
		if err != nil {
			t.Fatal(err)
		}
		last := call.Prompt[len(call.Prompt)-1]
		if last.Role != fantasy.MessageRoleAssistant || !strings.Contains(last.Content[0].(fantasy.TextPart).Text, "historical-only") {
			t.Fatalf("historical evidence moved into another role: %+v", call.Prompt)
		}
		if record.Metadata.Input.Source == nil {
			if len(call.Prompt) != 2 || call.Prompt[0].Content[0].(fantasy.TextPart).Text != "Real input" || call.Headers["x-initiator"] != "user" {
				t.Fatalf("real input changed: %+v", call)
			}
		} else if len(call.Prompt) != 1 || call.Headers["x-initiator"] != "agent" {
			t.Fatalf("historical automatic callback was identified as user input: %+v", call)
		}
	}
}
