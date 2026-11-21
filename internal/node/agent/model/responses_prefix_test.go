package model

import (
	"bytes"
	"encoding/json"
	"fmt"
	"maps"
	"reflect"
	"slices"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestResponsesSerializedPrefixContinuations(t *testing.T) {
	for _, modelID := range []string{"gpt-6.2-sol", "gpt-6-astra"} {
		t.Run(modelID, func(t *testing.T) {
			responses := make([]string, 10)
			for i := range responses {
				responses[i] = strings.ReplaceAll(string(fixture(t, "responses-encrypted.sse")), "stable-call", fmt.Sprintf("call_%d", i))
			}
			server := serverFixture(t, responses...)
			provider := protocol.DefaultConversationProvider()
			provider.ID, provider.Enabled = protocol.NewProviderID(), true
			provider.Api, provider.Authentication, provider.Endpoint = protocol.ConversationModelApiResponses, protocol.AuthenticationApiKey, server.URL+"/v1"
			entry := protocol.DefaultConversationModel()
			entry.ID, entry.Context, entry.Output, entry.Tools, entry.Reasoning = modelID, 32000, 512, true, true
			provider.Models = []protocol.ConversationModel{entry}
			selection := protocol.DefaultSessionConfig()
			selection.Provider, selection.Model = provider.ID, modelID
			selection.Mode, selection.Permission = protocol.WorkModeCode, protocol.PermissionAsk
			session, err := protocol.ParseSessionID(sessionID)
			if err != nil {
				t.Fatal(err)
			}
			secret := protocol.NewSecret("fixture-key")
			binding, err := Capture(provider, selection, &secret, session)
			if err != nil {
				t.Fatal(err)
			}
			language, err := New(t.Context(), binding)
			if err != nil {
				t.Fatal(err)
			}
			config, err := transportBinding(binding)
			if err != nil {
				t.Fatal(err)
			}
			turn := protocol.NewTurnID()
			message := func(role canonical.Role, blocks ...canonical.Block) canonical.Record {
				return canonical.NewRecord(turn, "message", canonical.NewMessage(role, blocks...), time.Time{})
			}
			schema := json.RawMessage(`{"type":"object","properties":{"path":{"type":"string"},"offset":{"type":"integer"}}}`)
			var inputSchema map[string]any
			if err := json.Unmarshal(schema, &inputSchema); err != nil {
				t.Fatal(err)
			}
			request := Request{
				System:  systemText,
				Records: []canonical.Record{message(canonical.User, canonical.Text(0, "Read the file"))},
				Tools: []Tool{
					{Name: "read", Description: "Read a file", InputSchema: inputSchema},
					{Name: "list", Description: "List files", InputSchema: map[string]any{"type": "object"}},
				},
				ToolSchemas: map[string]json.RawMessage{"read": schema},
			}
			developer := modelID == "gpt-6.2-sol"
			userIndex := 0
			if developer {
				userIndex = 1
			}
			send := func(t *testing.T, request Request) (capturedRequest, []fantasy.StreamPart) {
				t.Helper()
				stored, err := json.Marshal(request.Records)
				if err != nil {
					t.Fatal(err)
				}
				request.Records = nil
				if err := json.Unmarshal(stored, &request.Records); err != nil {
					t.Fatal(err)
				}
				call, err := BuildCall(binding, request)
				if err != nil {
					t.Fatal(err)
				}
				body, err := requestJSON(config, call)
				if err != nil {
					t.Fatal(err)
				}
				prepared, err := sourceRequestBytes(config, call, body)
				if err != nil {
					t.Fatal(err)
				}
				before := len(server.captured())
				parts := collect(t, language, call)
				captured := server.captured()
				if len(captured) != before+1 || !slices.ContainsFunc(parts, func(part fantasy.StreamPart) bool { return part.Type == fantasy.StreamPartTypeFinish }) {
					t.Fatal("expected one completed localhost Responses request")
				}
				wire := captured[before]
				if wire.Path != "/v1/responses" || !bytes.Equal(wire.Raw, prepared) {
					t.Fatal("SDK changed the prepared Responses body or route")
				}
				checkValue(t, wire.Body, "prompt_cache_key", sessionID)
				checkValue(t, wire.Body, "store", false)
				checkValue(t, wire.Body, "stream", true)
				if developer {
					checkValue(t, wire.Body, "instructions", nil)
					checkValue(t, wire.Body, "input.0.role", "developer")
					checkValue(t, wire.Body, "input.0.content.0.text", request.System)
				} else {
					checkValue(t, wire.Body, "instructions", request.System)
					checkValue(t, wire.Body, "input.0.role", "user")
				}
				return wire, parts
			}
			var previous capturedRequest
			for step := 0; step < 3; step++ {
				wire, parts := send(t, request)
				if step > 0 {
					if len(responsesInput(t, wire.Raw)) != len(responsesInput(t, previous.Raw))+3 {
						t.Fatal("continuation lost the reasoning, function call or function output")
					}
					checkResponsesPrefix(t, fmt.Sprintf("append-%d", step), previous, wire, false, false, len(responsesInput(t, previous.Raw)))
					checkValue(t, wire.Body, fmt.Sprintf("input.%d.summary", userIndex+1), []any{})
					checkValue(t, wire.Body, fmt.Sprintf("input.%d.encrypted_content", userIndex+1), "encrypted-reasoning")
				}
				previous = wire
				if step == 2 {
					break
				}
				assistant := canonical.NewMessage(canonical.Assistant)
				var results []canonical.Record
				for _, part := range parts {
					block := canonical.Block{Index: uint64(len(assistant.Content))}
					switch part.Type {
					case fantasy.StreamPartTypeReasoningEnd:
						block.Kind, block.Data = "thinking", &canonical.Thinking{}
						if err := ApplyStreamMetadata(&block, part); err != nil {
							t.Fatal(err)
						}
					case fantasy.StreamPartTypeToolCall:
						block.Kind, block.Data = "tool_request", &canonical.ToolRequest{ID: part.ID, Name: part.ToolCallName, Arguments: json.RawMessage(part.ToolCallInput)}
						results = append(results, message(canonical.User, canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{ID: part.ID, Name: part.ToolCallName, Status: canonical.ToolSuccess, Result: canonical.Payload{Value: json.RawMessage(`"File contents"`)}}}))
					default:
						continue
					}
					assistant.Content = append(assistant.Content, block)
				}
				if len(assistant.Content) != 2 || len(results) != 1 {
					t.Fatal("recorded fixture did not produce encrypted reasoning and one tool call")
				}
				assistant.PartCount = uint64(len(assistant.Content))
				record := canonical.NewRecord(turn, "message", assistant, time.Time{})
				record.Metadata.Runtime = &canonical.Runtime{AgentVisible: true, Inference: &canonical.Inference{Provider: "sailry:" + provider.ID.String(), RequestedModel: modelID}}
				request.Records = append(request.Records, record)
				request.Records = append(request.Records, results...)
			}
			repeated, _ := send(t, request)
			if !bytes.Equal(previous.Raw, repeated.Raw) {
				t.Fatal("rebuilding identical canonical input changed the serialized SDK body")
			}
			for _, control := range []string{"tool-order", "tool-description", "schema-order", "instructions", "old-message", "new-message"} {
				t.Run(control, func(t *testing.T) {
					variant := request
					variant.Tools, variant.ToolSchemas, variant.Records = slices.Clone(request.Tools), maps.Clone(request.ToolSchemas), slices.Clone(request.Records)
					toolsChanged, instructionsChanged := false, false
					shared := len(responsesInput(t, previous.Raw))
					switch control {
					case "tool-order":
						variant.Tools[0], variant.Tools[1] = variant.Tools[1], variant.Tools[0]
						toolsChanged = true
					case "tool-description":
						variant.Tools[0].Description += " Carefully."
						toolsChanged = true
					case "schema-order":
						variant.ToolSchemas["read"] = json.RawMessage(`{"type":"object","properties":{"offset":{"type":"integer"},"path":{"type":"string"}}}`)
						toolsChanged = true
					case "instructions":
						variant.System += " Be concise."
						instructionsChanged = !developer
						if developer {
							shared = 0
						}
					case "old-message":
						variant.Records[0] = message(canonical.User, canonical.Text(0, "Read another file"))
						shared = userIndex
					case "new-message":
						variant.Records = append(variant.Records, message(canonical.User, canonical.Text(0, "Continue")))
					}
					wire, _ := send(t, variant)
					wantItems := len(responsesInput(t, previous.Raw))
					if control == "new-message" {
						wantItems++
					}
					if len(responsesInput(t, wire.Raw)) != wantItems {
						t.Fatal("control changed unrelated input items")
					}
					checkResponsesPrefix(t, control, previous, wire, toolsChanged, instructionsChanged, shared)
					if control == "schema-order" && !reflect.DeepEqual(previous.Body["tools"], wire.Body["tools"]) {
						t.Fatal("schema-order control changed schema values, not just declaration order")
					}
				})
			}
		})
	}
}

func responsesInput(t *testing.T, raw []byte) []json.RawMessage {
	t.Helper()
	var body struct {
		Input []json.RawMessage `json:"input"`
	}
	if err := json.Unmarshal(raw, &body); err != nil {
		t.Fatal(err)
	}
	return body.Input
}

func checkResponsesPrefix(t *testing.T, name string, before, after capturedRequest, toolsChanged, instructionsChanged bool, shared int) {
	t.Helper()
	var left, right map[string]json.RawMessage
	if err := json.Unmarshal(before.Raw, &left); err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(after.Raw, &right); err != nil {
		t.Fatal(err)
	}
	for key, value := range left {
		if key == "input" {
			continue
		}
		changed := key == "tools" && toolsChanged || key == "instructions" && instructionsChanged
		if bytes.Equal(value, right[key]) == changed {
			t.Errorf("%s: field %s changed=%v, want %v", name, key, !bytes.Equal(value, right[key]), changed)
		}
	}
	if len(left) != len(right) {
		t.Errorf("%s: request fields changed", name)
	}
	oldInput, newInput := responsesInput(t, before.Raw), responsesInput(t, after.Raw)
	items := 0
	for items < len(oldInput) && items < len(newInput) && bytes.Equal(oldInput[items], newInput[items]) {
		items++
	}
	if items != shared {
		t.Errorf("%s: unchanged input prefix=%d items, want %d", name, items, shared)
	}
	prefix := 0
	for prefix < len(before.Raw) && prefix < len(after.Raw) && before.Raw[prefix] == after.Raw[prefix] {
		prefix++
	}
	t.Logf("%s: JSON bytes=%d->%d common byte prefix=%d; unchanged input items=%d/%d->%d; tools changed=%v instructions changed=%v (not upstream token/cache measurements)", name, len(before.Raw), len(after.Raw), prefix, items, len(oldInput), len(newInput), toolsChanged, instructionsChanged)
}
