package model

import (
	"encoding/base64"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestToolResultMedia holds every wire to the same rule: an image a tool
// returned reaches a vision model as an image and never as base64 text.
func TestToolResultMedia(t *testing.T) {
	pixels := []byte("\x89PNG\r\n\x1a\nfixture")
	encoded := base64.StdEncoding.EncodeToString(pixels)
	request := func(response canonical.ToolResponse) Request {
		turn, created := protocol.NewTurnID(), time.Unix(100, 0)
		call := canonical.Block{Kind: "tool_request", Data: &canonical.ToolRequest{ID: "call", Name: "browser", Arguments: json.RawMessage(`{}`)}}
		return Request{Assets: map[string][]byte{"digest": pixels}, Records: []canonical.Record{
			canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Look at the page")), created),
			canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, call), created),
			canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Block{Kind: "tool_response", Data: &response}), created),
			canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Continue")), created),
		}}
	}
	lifted := canonical.ToolResponse{ID: "call", Name: "browser", Status: canonical.ToolSuccess, Result: canonical.Payload{Value: json.RawMessage(`{"kind":"browser","data":{"tab":1}}`)},
		Attachments: []canonical.Attachment{{Asset: canonical.AssetRef{Digest: "digest", Mime: "image/png", Size: uint64(len(pixels))}}}}
	mcp := canonical.ToolResponse{ID: "call", Name: "browser", Status: canonical.ToolSuccess, Format: canonical.MCP,
		Result: canonical.Payload{Value: json.RawMessage(`{"content":[{"type":"text","text":"captured"},{"type":"image","data":"` + encoded + `","mimeType":"image/png"}]}`)}}
	body := func(t *testing.T, api protocol.ConversationModelApi, kind, name string, vision bool, response canonical.ToolResponse) (map[string]any, string) {
		t.Helper()
		provider := protocol.DefaultConversationProvider()
		provider.ID, provider.Api, provider.Authentication, provider.Enabled = protocol.NewProviderID(), api, protocol.AuthenticationApiKey, true
		entry := protocol.DefaultConversationModel()
		entry.ID, entry.Vision = name, vision
		provider.Models = []protocol.ConversationModel{entry}
		binding := Binding{Provider: provider, Config: protocol.SessionConfig{Provider: provider.ID, Model: name, Effort: protocol.Effort{Kind: "default"}}}
		call, err := BuildCall(binding, request(response))
		if err != nil {
			t.Fatal(err)
		}
		config := transportConfig{Kind: kind, Model: name}
		value, err := requestJSON(config, call)
		if err != nil {
			t.Fatal(err)
		}
		raw, err := sourceRequestBytes(config, call, value)
		if err != nil {
			t.Fatal(err)
		}
		var decoded map[string]any
		if err := json.Unmarshal(raw, &decoded); err != nil {
			t.Fatal(err)
		}
		return decoded, string(raw)
	}
	for name, response := range map[string]canonical.ToolResponse{"lifted": lifted, "mcp": mcp} {
		t.Run(name, func(t *testing.T) {
			t.Run("anthropic", func(t *testing.T) {
				decoded, _ := body(t, protocol.ConversationModelApiAnthropic, "anthropic", "claude", true, response)
				content := wireArray(valueAt(decoded, "messages.2.content.0.content"))
				image := wireObject(content[len(content)-1])
				if valueAt(decoded, "messages.2.content.0.type") != "tool_result" || image["type"] != "image" || valueAt(image, "source.data") != encoded || valueAt(image, "source.media_type") != "image/png" {
					t.Errorf("tool result = %#v", valueAt(decoded, "messages.2.content.0"))
				}
				if wireObject(content[0])["type"] != "text" {
					t.Errorf("tool result text = %#v", content[0])
				}
			})
			t.Run("responses", func(t *testing.T) {
				decoded, _ := body(t, protocol.ConversationModelApiResponses, "responses", "gpt", true, response)
				var output []any
				for _, item := range wireArray(decoded["input"]) {
					if wireObject(item)["type"] == "function_call_output" {
						output = wireArray(wireObject(item)["output"])
					}
				}
				if len(output) != 2 || valueAt(output, "0.type") != "input_text" || valueAt(output, "1.type") != "input_image" || valueAt(output, "1.image_url") != "data:image/png;base64,"+encoded {
					t.Errorf("function output = %#v", output)
				}
			})
			t.Run("chat", func(t *testing.T) {
				decoded, _ := body(t, protocol.ConversationModelApiChatCompletions, "chat", "vision", true, response)
				messages := wireArray(decoded["messages"])
				roles := []string{}
				for _, message := range messages {
					roles = append(roles, wireString(wireObject(message)["role"]))
				}
				if strings.Join(roles, ",") != "user,assistant,tool,user,user" {
					t.Fatalf("message roles = %v", roles)
				}
				if text := wireString(valueAt(decoded, "messages.2.content")); !strings.HasSuffix(text, toolImageNext) {
					t.Errorf("tool message = %q", text)
				}
				if valueAt(decoded, "messages.3.content.0.image_url.url") != "data:image/png;base64,"+encoded {
					t.Errorf("image message = %#v", messages[3])
				}
			})
			t.Run("gemini", func(t *testing.T) {
				decoded, _ := body(t, protocol.ConversationModelApiGemini, "google", "gemini-3-pro", true, response)
				if valueAt(decoded, "contents.2.parts.0.functionResponse.parts.0.inlineData.data") != encoded {
					t.Errorf("function response = %#v", valueAt(decoded, "contents.2.parts"))
				}
				decoded, _ = body(t, protocol.ConversationModelApiGemini, "google", "gemini-2.5-pro", true, response)
				if valueAt(decoded, "contents.2.parts.0.inlineData.data") != encoded || valueAt(decoded, "contents.2.parts.1.functionResponse.name") != "browser" {
					t.Errorf("function response parts = %#v", valueAt(decoded, "contents.2.parts"))
				}
			})
			t.Run("without vision", func(t *testing.T) {
				for kind, api := range map[string]protocol.ConversationModelApi{"anthropic": protocol.ConversationModelApiAnthropic, "responses": protocol.ConversationModelApiResponses, "chat": protocol.ConversationModelApiChatCompletions, "google": protocol.ConversationModelApiGemini} {
					_, raw := body(t, api, kind, "text-only", false, response)
					if strings.Contains(raw, encoded) || !strings.Contains(raw, toolImageOmitted) {
						t.Errorf("%s request carries the image or no note", kind)
					}
				}
			})
		})
	}
	t.Run("large image is refused", func(t *testing.T) {
		response := lifted
		response.Attachments = []canonical.Attachment{{Asset: canonical.AssetRef{Digest: "digest", Mime: "image/png", Size: MaxToolMediaBytes + 1}}}
		_, raw := body(t, protocol.ConversationModelApiAnthropic, "anthropic", "claude", true, response)
		if strings.Contains(raw, encoded) || !strings.Contains(raw, toolImageLarge) {
			t.Error("oversized image was sent or left no note")
		}
	})
	t.Run("model adapter", func(t *testing.T) {
		for _, vision := range []bool{true, false} {
			model, binding, _ := pluginFixture(t, "", nil)
			entry := protocol.DefaultConversationModel()
			entry.ID, entry.Vision = "captured", true
			binding.Provider.Models = []protocol.ConversationModel{entry}
			(**binding.Provider.Binding).Adapter.Capabilities.Vision = vision
			call, err := BuildCall(binding, request(lifted))
			if err != nil {
				t.Fatal(err)
			}
			wire, err := model.request(call)
			if err != nil {
				t.Fatal(err)
			}
			var result map[string]any
			for _, message := range wire.Messages {
				if tool, ok := message.Data.(*protocol.PluginModelAdapterWireMessageToolPayload); ok {
					if err := json.Unmarshal(tool.Parts[0].Data.(*protocol.PluginModelAdapterWirePartJsonPayload).Value, &result); err != nil {
						t.Fatal(err)
					}
				}
			}
			content := wireArray(result["content"])
			if sent := len(content) == 2 && valueAt(content, "1.type") == "image" && valueAt(content, "1.data") == encoded; sent != vision || valueAt(content, "0.type") != "text" {
				t.Errorf("adapter vision %v received %#v", vision, content)
			}
		}
	})
	t.Run("grouped chat results precede their images", func(t *testing.T) {
		provider := protocol.DefaultConversationProvider()
		provider.ID, provider.Api, provider.Authentication, provider.Enabled = protocol.NewProviderID(), protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, true
		entry := protocol.DefaultConversationModel()
		entry.ID, entry.Vision = "vision", true
		provider.Models = []protocol.ConversationModel{entry}
		binding := Binding{Provider: provider, Config: protocol.SessionConfig{Provider: provider.ID, Model: "vision", Effort: protocol.Effort{Kind: "default"}}}
		value := request(lifted)
		second := lifted
		second.ID = "other"
		value.Records[1] = canonical.NewRecord(value.Records[1].Turn, "message", canonical.NewMessage(canonical.Assistant,
			canonical.Block{Kind: "tool_request", Data: &canonical.ToolRequest{ID: "call", Name: "browser", Arguments: json.RawMessage(`{}`)}},
			canonical.Block{Index: 1, Kind: "tool_request", Data: &canonical.ToolRequest{ID: "other", Name: "browser", Arguments: json.RawMessage(`{}`)}}), time.Unix(100, 0))
		value.Records[2] = canonical.NewRecord(value.Records[2].Turn, "message", canonical.NewMessage(canonical.User,
			canonical.Block{Kind: "tool_response", Data: &lifted}, canonical.Block{Index: 1, Kind: "tool_response", Data: &second}), time.Unix(100, 0))
		call, err := BuildCall(binding, value)
		if err != nil {
			t.Fatal(err)
		}
		decoded, err := requestJSON(transportConfig{Kind: "chat", Model: "vision"}, call)
		if err != nil {
			t.Fatal(err)
		}
		roles := []string{}
		for _, message := range wireArray(decoded["messages"]) {
			roles = append(roles, wireString(wireObject(message)["role"]))
		}
		if strings.Join(roles, ",") != "user,assistant,tool,tool,user,user" || len(wireArray(valueAt(decoded, "messages.4.content"))) != 2 {
			t.Errorf("message roles = %v", roles)
		}
	})
}
