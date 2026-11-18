package model

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestFoundryMessageCacheUsesResolvedTransport(t *testing.T) {
	for _, route := range []struct {
		publisher, upstream, path, response string
	}{
		{"anthropic", "claude-sonnet-4-6", "/anthropic/v1/messages", "anthropic-text.sse"},
		{"openai", "gpt-6.2-sol", "/api/projects/cache-audit/openai/v1/responses", "responses-text.sse"},
	} {
		t.Run(route.publisher, func(t *testing.T) {
			reply := string(fixture(t, route.response))
			captured := make(chan map[string]any, 1)
			mux := http.NewServeMux()
			mux.HandleFunc("GET /api/projects/cache-audit/deployments", func(writer http.ResponseWriter, _ *http.Request) {
				writer.Header().Set("Content-Type", "application/json")
				fmt.Fprintf(writer, `{"value":[{"name":"project-model","modelName":%q,"modelPublisher":%q}]}`, route.upstream, route.publisher)
			})
			mux.HandleFunc("POST "+route.path, func(writer http.ResponseWriter, request *http.Request) {
				var body map[string]any
				if err := json.NewDecoder(request.Body).Decode(&body); err != nil {
					t.Error(err)
					writer.WriteHeader(http.StatusBadRequest)
					return
				}
				select {
				case captured <- body:
				default:
					t.Error("unexpected extra model request")
				}
				writer.Header().Set("Content-Type", "text/event-stream")
				io.WriteString(writer, reply)
			})
			server := httptest.NewServer(mux)
			defer server.Close()
			provider := protocol.DefaultConversationProvider()
			provider.ID, provider.Enabled = protocol.NewProviderID(), true
			provider.Api, provider.Authentication = protocol.ConversationModelApiAzureAi, protocol.AuthenticationApiKey
			provider.Endpoint = server.URL + "/api/projects/cache-audit"
			selection := protocol.DefaultSessionConfig()
			selection.Provider, selection.Model = provider.ID, "project-model"
			selection.Mode, selection.Permission = protocol.WorkModeCode, protocol.PermissionAsk
			secret := protocol.NewSecret("fixture-key")
			binding, err := Capture(provider, selection, &secret, protocol.NewSessionID())
			if err != nil {
				t.Fatal(err)
			}
			language, err := New(t.Context(), binding)
			if err != nil {
				t.Fatal(err)
			}
			maxOutput := int64(512)
			record := canonical.NewRecord(protocol.NewTurnID(), "message", canonical.NewMessage(canonical.User, canonical.Text(0, "A model request without tools")), policyClock{}.now)
			call, err := BuildCall(binding, Request{System: systemText, Records: []canonical.Record{record}, MaxOutputTokens: &maxOutput})
			if err != nil {
				t.Fatal(err)
			}
			collect(t, language, call)
			select {
			case body := <-captured:
				checkValue(t, body, "model", selection.Model)
				if route.publisher == "anthropic" {
					want := map[string]any{"type": "ephemeral"}
					checkValue(t, body, "system.0.cache_control", want)
					checkValue(t, body, "messages.0.content.0.cache_control", want)
				} else {
					checkValue(t, body, "instructions", nil)
					checkValue(t, body, "input.0.role", "developer")
					checkValue(t, body, "input.0.content.0.text", systemText)
					checkValue(t, body, "input.0.content.0.cache_control", nil)
				}
			default:
				t.Fatal("no resolved deployment request was captured")
			}
		})
	}
}
