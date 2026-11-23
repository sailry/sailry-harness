package model

import (
	"encoding/json"
	"reflect"
	"slices"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestModelSwitchPreservesCanonicalHistory(t *testing.T) {
	for _, api := range []protocol.ConversationModelApi{protocol.ConversationModelApiChatCompletions, protocol.ConversationModelApiResponses} {
		t.Run(string(api), func(t *testing.T) {
			kind := "chat"
			if api == protocol.ConversationModelApiResponses {
				kind = "responses"
			}
			server := serverFixture(t, string(fixture(t, "anthropic-text.sse")), string(fixture(t, "anthropic-text.sse")), string(fixture(t, kind+"-text.sse")))
			session := protocol.NewSessionID()
			binding := func(api protocol.ConversationModelApi, model string) Binding {
				provider := protocol.DefaultConversationProvider()
				provider.ID, provider.Enabled = protocol.NewProviderID(), true
				provider.Api, provider.Authentication, provider.Endpoint = api, protocol.AuthenticationApiKey, server.URL
				entry := protocol.DefaultConversationModel()
				entry.ID, entry.Output, entry.Reasoning = model, 8192, true
				provider.Models = []protocol.ConversationModel{entry}
				config := protocol.DefaultSessionConfig()
				config.Provider, config.Model, config.Effort = provider.ID, model, protocol.Effort{Kind: "high"}
				config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
				secret := protocol.NewSecret("fixture-key")
				captured, err := Capture(provider, config, &secret, session)
				if err != nil {
					t.Fatal(err)
				}
				return captured
			}
			gpt := binding(api, "gpt-6.1-sol")
			claude := binding(protocol.ConversationModelApiAnthropic, "claude-opus-5-5")
			turn := protocol.NewTurnID()
			message := func(role canonical.Role, blocks ...canonical.Block) canonical.Record {
				return canonical.NewRecord(turn, "message", canonical.NewMessage(role, blocks...), time.Time{})
			}
			assistant := func(source Binding, signature, text, id string) canonical.Record {
				record := message(canonical.Assistant,
					canonical.Block{Index: 0, Kind: "thinking", Data: &canonical.Thinking{Text: source.Config.Model + " private reasoning", Signature: &signature}},
					canonical.Text(1, text),
					canonical.Block{Index: 2, Kind: "tool_request", Data: &canonical.ToolRequest{ID: id, Name: "read", Arguments: json.RawMessage(`{"path":"file"}`)}},
				)
				record.Metadata.Runtime = &canonical.Runtime{AgentVisible: true, Inference: &canonical.Inference{Provider: "sailry:" + source.Provider.ID.String(), RequestedModel: source.Config.Model}}
				return record
			}
			result := func(id, text string) canonical.Record {
				payload, err := json.Marshal(text)
				if err != nil {
					t.Fatal(err)
				}
				return message(canonical.User, canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{ID: id, Name: "read", Status: canonical.ToolSuccess, Result: canonical.Payload{Value: payload}}})
			}
			records := []canonical.Record{
				message(canonical.User, canonical.Text(0, "Read the project status")),
				assistant(gpt, "gpt-encrypted-reasoning", "I will read the status", "call_gpt"),
				result("call_gpt", "Project status from GPT's tool"),
				message(canonical.Assistant, canonical.Text(0, "The project is ready")),
				message(canonical.User, canonical.Text(0, "Continue on Claude")),
			}
			lastPrefix := ""
			send := func(selected Binding) capturedRequest {
				before, err := json.Marshal(records)
				if err != nil {
					t.Fatal(err)
				}
				call, err := BuildCall(selected, Request{System: systemText, Records: records, Tools: []Tool{{Name: "read", Description: "Read a file", InputSchema: objectFixture(t, "schema.json")}}})
				if err != nil {
					t.Fatal(err)
				}
				lastPrefix = ReplayPrefix(call)
				model, err := New(t.Context(), selected)
				if err != nil {
					t.Fatal(err)
				}
				parts := collect(t, model, call)
				if !slices.ContainsFunc(parts, func(part fantasy.StreamPart) bool { return part.Type == fantasy.StreamPartTypeFinish }) {
					t.Fatal("model continuation did not finish")
				}
				after, err := json.Marshal(records)
				if err != nil || string(before) != string(after) {
					t.Fatal("model switch changed canonical history")
				}
				requests := server.captured()
				return requests[len(requests)-1]
			}
			first := send(claude)
			checkValue(t, first.Body, "messages.1.content.0.text", "I will read the status")
			checkValue(t, first.Body, "messages.1.content.1.id", "call_gpt")
			checkValue(t, first.Body, "messages.2.content.0.tool_use_id", "call_gpt")
			checkValue(t, first.Body, "messages.3.content.0.text", "The project is ready")
			for _, foreign := range []string{"gpt-encrypted-reasoning", "private reasoning", "prompt_cache_key", "reasoning_effort", "encrypted_content"} {
				if strings.Contains(string(first.Raw), foreign) {
					t.Errorf("Claude request carried GPT-only data %q", foreign)
				}
			}
			continued := assistant(claude, "claude-signed-thinking", "I will verify the status", "toolu_claude")
			continued.Metadata.Runtime.Inference.ReplayPrefix = lastPrefix
			records = append(records, continued, result("toolu_claude", "Project status from Claude's tool"))
			second := send(claude)
			checkValue(t, second.Body, "messages.5.content.0.signature", "claude-signed-thinking")
			checkValue(t, second.Body, "messages.5.content.2.id", "toolu_claude")
			checkValue(t, second.Body, "messages.6.content.0.tool_use_id", "toolu_claude")
			if !reflect.DeepEqual(first.Body["system"], second.Body["system"]) || !reflect.DeepEqual(first.Body["tools"], second.Body["tools"]) {
				t.Fatal("tool continuation changed stable instructions or declarations")
			}
			records = append(records, message(canonical.Assistant, canonical.Text(0, "Claude verified the status")), message(canonical.User, canonical.Text(0, "Continue on GPT")))
			third := send(gpt)
			if strings.Contains(string(third.Raw), "claude-signed-thinking") || strings.Contains(string(third.Raw), "claude-opus-5-5 private reasoning") {
				t.Error("GPT request carried Claude-only thinking")
			}
			for _, retained := range []string{"The project is ready", "Claude verified the status", "call_gpt", "toolu_claude", "Project status from GPT's tool", "Project status from Claude's tool"} {
				if !strings.Contains(string(third.Raw), retained) {
					t.Errorf("switching back lost history %q", retained)
				}
			}
			if kind == "responses" {
				checkValue(t, third.Body, "input.1.encrypted_content", "gpt-encrypted-reasoning")
				checkValue(t, third.Body, "input.3.call_id", "call_gpt")
				checkValue(t, third.Body, "input.4.call_id", "call_gpt")
				checkValue(t, third.Body, "input.8.call_id", "toolu_claude")
				checkValue(t, third.Body, "input.9.call_id", "toolu_claude")
			} else {
				checkValue(t, third.Body, "messages.2.reasoning_content", "gpt-6.1-sol private reasoning")
				checkValue(t, third.Body, "messages.2.tool_calls.0.id", "call_gpt")
				checkValue(t, third.Body, "messages.3.tool_call_id", "call_gpt")
				checkValue(t, third.Body, "messages.6.tool_calls.0.id", "toolu_claude")
				checkValue(t, third.Body, "messages.7.tool_call_id", "toolu_claude")
			}
			if len(server.captured()) != 3 || gpt.Session != session || claude.Session != session {
				t.Fatal("continuation retried a completed request or replaced the session")
			}
		})
	}
}

func TestRecordedSessionSwitchesAcrossAnthropicResponsesAndGemini(t *testing.T) {
	responses := []string{"Sonnet response", "GPT response", "Gemini response"}
	fixtureResponse := func(name, text string) string {
		t.Helper()
		return strings.ReplaceAll(string(fixture(t, name)), "Read complete", text)
	}
	server := serverFixture(t,
		fixtureResponse("anthropic-text.sse", responses[0]),
		fixtureResponse("responses-text.sse", responses[1]),
		fixtureResponse("google-text.sse", responses[2]),
	)
	session := protocol.NewSessionID()
	bind := func(api protocol.ConversationModelApi, model string) Binding {
		provider := protocol.DefaultConversationProvider()
		provider.ID, provider.Enabled = protocol.NewProviderID(), true
		provider.Api, provider.Authentication, provider.Endpoint = api, protocol.AuthenticationApiKey, server.URL
		provider.Name = string(api) + " fixture"
		entry := protocol.DefaultConversationModel()
		entry.ID, entry.Output, entry.Reasoning = model, 8192, true
		provider.Models = []protocol.ConversationModel{entry}
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model = provider.ID, model
		config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
		config.Effort = protocol.Effort{Kind: "high"}
		secret := protocol.NewSecret("fixture-key")
		binding, err := Capture(provider, config, &secret, session)
		if err != nil {
			t.Fatal(err)
		}
		return binding
	}
	channels := []struct {
		api   protocol.ConversationModelApi
		model string
		path  string
	}{
		{protocol.ConversationModelApiAnthropic, "claude-sonnet-5-5", "/v1/messages"},
		{protocol.ConversationModelApiResponses, "gpt-6.1-sol", "/v1/responses"},
		{protocol.ConversationModelApiGemini, "gemini-3.8-flash", "/v1beta/models/gemini-3.8-flash:streamGenerateContent?alt=sse"},
	}
	message := func(turn protocol.TurnID, role canonical.Role, text string) canonical.Record {
		return canonical.NewRecord(turn, "message", canonical.NewMessage(role, canonical.Text(0, text)), time.Time{})
	}
	turns := make([]protocol.TurnID, len(channels))
	for index := range turns {
		turns[index] = protocol.NewTurnID()
	}
	records := []canonical.Record{message(turns[0], canonical.User, "Read the project status")}
	bindings := make([]Binding, len(channels))
	for index, channel := range channels {
		bindings[index] = bind(channel.api, channel.model)
	}
	send := func(binding Binding, turn protocol.TurnID) capturedRequest {
		t.Helper()
		before, err := json.Marshal(records)
		if err != nil {
			t.Fatal(err)
		}
		call, err := BuildCall(binding, Request{System: systemText, Records: records})
		if err != nil {
			t.Fatal(err)
		}
		language, err := New(t.Context(), binding)
		if err != nil {
			t.Fatal(err)
		}
		parts := collect(t, language, call)
		var reply strings.Builder
		finished := false
		for _, part := range parts {
			switch part.Type {
			case fantasy.StreamPartTypeTextDelta:
				reply.WriteString(part.Delta)
			case fantasy.StreamPartTypeFinish:
				finished = true
			}
		}
		if !finished || reply.Len() == 0 {
			t.Fatal("recorded provider response did not finish with text")
		}
		after, err := json.Marshal(records)
		if err != nil || string(before) != string(after) {
			t.Fatal("switching channels changed canonical session history")
		}
		assistant := message(turn, canonical.Assistant, reply.String())
		assistant.Metadata.Runtime = &canonical.Runtime{AgentVisible: true, Inference: &canonical.Inference{
			Provider: "sailry:" + binding.Provider.ID.String(), RequestedModel: binding.Config.Model,
		}}
		records = append(records, assistant)
		requests := server.captured()
		return requests[len(requests)-1]
	}

	for index, binding := range bindings {
		if binding.Session != session {
			t.Fatalf("channel %s replaced the session identity", binding.Provider.Api)
		}
		if index > 0 {
			records = append(records, message(turns[index], canonical.User, "Continue with "+binding.Config.Model))
		}
		request := send(binding, turns[index])
		if request.Path != channels[index].path {
			t.Errorf("%s route = %q; want %q", binding.Provider.Api, request.Path, channels[index].path)
		}
		if binding.Provider.Api != protocol.ConversationModelApiGemini && request.Body["model"] != binding.Config.Model {
			t.Errorf("%s request model = %v; want %q", binding.Provider.Api, request.Body["model"], binding.Config.Model)
		}
		for previous := 0; previous <= index; previous++ {
			if !strings.Contains(string(request.Raw), responses[previous]) && previous < index {
				t.Errorf("%s request lost prior %s response %q", binding.Config.Model, channels[previous].api, responses[previous])
			}
		}
	}
	requests := server.captured()
	if len(requests) != len(channels) {
		t.Fatalf("same-session channel switches made %d requests, want %d", len(requests), len(channels))
	}
}
