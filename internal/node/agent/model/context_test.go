package model

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func contextFixture(t *testing.T) {
	secret := protocol.NewSecret("context-secret")
	bind := func(t *testing.T, api protocol.ConversationModelApi, endpoint, name string, listed ...protocol.ConversationModel) (Binding, error) {
		t.Helper()
		provider := protocol.DefaultConversationProvider()
		provider.ID, provider.Name, provider.Enabled = protocol.NewProviderID(), "Fixture", true
		provider.Api, provider.Authentication, provider.Endpoint, provider.Models = api, protocol.AuthenticationApiKey, endpoint, listed
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Mode, config.Permission = provider.ID, name, protocol.WorkModeCode, protocol.PermissionAsk
		return Capture(provider, config, &secret, protocol.NewSessionID())
	}
	entry := func(id string, context uint32) protocol.ConversationModel {
		listed := protocol.DefaultConversationModel()
		listed.ID, listed.Context, listed.Output = id, context, 1000
		return listed
	}
	t.Run("selection", func(t *testing.T) {
		if _, err := bind(t, protocol.ConversationModelApiAnthropic, "https://example.test", "removed", entry("kept", 64000)); err == nil {
			t.Fatal("a model missing from a non-empty list was captured")
		} else if failure := (*Failure)(nil); !errors.As(err, &failure) || failure.Kind != NotConfigured {
			t.Fatalf("failure = %v", err)
		}
		if _, err := bind(t, protocol.ConversationModelApiAnthropic, "https://example.test", "anything"); err != nil {
			t.Fatalf("an empty list refused a model: %v", err)
		}
	})
	t.Run("catalog", func(t *testing.T) {
		for _, row := range []struct {
			name   string
			api    protocol.ConversationModelApi
			model  string
			listed []protocol.ConversationModel
			want   uint32
		}{
			{"listed", protocol.ConversationModelApiAnthropic, "claude-sonnet-4-5", []protocol.ConversationModel{entry("claude-sonnet-4-5", 64000)}, 64000},
			{"family", protocol.ConversationModelApiResponses, "gpt-5", nil, 400000},
			{"dated", protocol.ConversationModelApiAnthropic, "claude-sonnet-4-5-20250929", nil, catalogContext(protocol.ConversationModelApiAnthropic, "claude-sonnet-4-5")},
			{"gateway prefix", protocol.ConversationModelApiChatCompletions, "openai/gpt-5", nil, 400000},
			{"other vendor", protocol.ConversationModelApiChatCompletions, "DeepSeek-Flash", nil, catalogWindows()["deepseek-flash"]},
			{"unknown", protocol.ConversationModelApiChatCompletions, "fixture/unknown", nil, 128000},
			{"unknown anthropic", protocol.ConversationModelApiAnthropic, "fixture", nil, 128000},
		} {
			t.Run(row.name, func(t *testing.T) {
				binding, err := bind(t, row.api, "https://example.test", row.model, row.listed...)
				if err != nil {
					t.Fatal(err)
				}
				if got := ContextLimit(binding); got != row.want || got == 0 {
					t.Fatalf("context = %d, want %d", got, row.want)
				}
			})
		}
		if got := catalogWindows()["deepseek-flash"]; got == 0 || got == 128000 {
			t.Fatalf("cross-vendor window = %d", got)
		}
	})
	t.Run("discovery", func(t *testing.T) {
		for _, row := range []struct {
			name, model, body string
			status            int
			api               protocol.ConversationModelApi
			want              uint32
			requests          int
		}{
			{"only entry", "alias", `{"data":[{"id":"loaded","meta":{"n_ctx":32768}}]}`, 200, protocol.ConversationModelApiChatCompletions, 32768, 1},
			{"matching entry", "second", `{"data":[{"id":"first","meta":{"n_ctx":16384}},{"id":"second","meta":{"n_ctx":8192}}]}`, 200, protocol.ConversationModelApiResponses, 8192, 1},
			{"no entry", "missing", `{"data":[{"id":"first","meta":{"n_ctx":16384}},{"id":"second","meta":{"n_ctx":8192}}]}`, 200, protocol.ConversationModelApiChatCompletions, 128000, 1},
			{"zero", "zero", `{"data":[{"id":"zero","meta":{"n_ctx":0}}]}`, 200, protocol.ConversationModelApiChatCompletions, 128000, 1},
			{"no size", "gpt-5", `{"data":[{"id":"gpt-5"}]}`, 200, protocol.ConversationModelApiChatCompletions, 400000, 1},
			{"not found", "fixture", `{}`, 404, protocol.ConversationModelApiChatCompletions, 128000, 1},
			{"failure", "fixture", `{}`, 500, protocol.ConversationModelApiChatCompletions, 128000, 1},
			{"redirect", "fixture", `{}`, 302, protocol.ConversationModelApiChatCompletions, 128000, 1},
			{"invalid", "fixture", `not json`, 200, protocol.ConversationModelApiChatCompletions, 128000, 1},
			{"other wire", "fixture", `{"data":[{"id":"fixture","meta":{"n_ctx":32768}}]}`, 200, protocol.ConversationModelApiAnthropic, 128000, 0},
		} {
			t.Run(row.name, func(t *testing.T) {
				var lock sync.Mutex
				var seen []*http.Request
				server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
					lock.Lock()
					seen = append(seen, request)
					lock.Unlock()
					if row.status == 302 {
						writer.Header().Set("Location", "/elsewhere")
					}
					writer.WriteHeader(row.status)
					writer.Write([]byte(row.body))
				}))
				t.Cleanup(server.Close)
				binding, err := bind(t, row.api, server.URL+"/v1", row.model)
				if err != nil {
					t.Fatal(err)
				}
				// A turn creates its model once; a later turn reuses the answer.
				for range 2 {
					if _, err = New(t.Context(), binding); err != nil {
						t.Fatal(err)
					}
				}
				if got := ContextLimit(binding); got != row.want {
					t.Fatalf("context = %d, want %d", got, row.want)
				}
				lock.Lock()
				defer lock.Unlock()
				if len(seen) != row.requests {
					t.Fatalf("requests = %d, want %d", len(seen), row.requests)
				}
				if row.requests == 1 && (seen[0].Method != http.MethodGet || seen[0].URL.Path != "/v1/models" || seen[0].Header.Get("Authorization") != "Bearer context-secret" || seen[0].Header.Get("User-Agent") != "Sailry/0.1.0") {
					t.Fatalf("request = %s %s %v", seen[0].Method, seen[0].URL.Path, seen[0].Header)
				}
			})
		}
		t.Run("listed", func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) { t.Error("a listed model was probed") }))
			t.Cleanup(server.Close)
			binding, err := bind(t, protocol.ConversationModelApiChatCompletions, server.URL+"/v1", "listed", entry("listed", 64000))
			if err != nil {
				t.Fatal(err)
			}
			if _, err = New(t.Context(), binding); err != nil {
				t.Fatal(err)
			}
			if got := ContextLimit(binding); got != 64000 {
				t.Fatalf("context = %d", got)
			}
		})
		t.Run("cancelled", func(t *testing.T) {
			release := make(chan struct{})
			server := httptest.NewServer(http.HandlerFunc(func(_ http.ResponseWriter, request *http.Request) {
				select {
				case <-request.Context().Done():
				case <-release:
				}
			}))
			t.Cleanup(server.Close)
			t.Cleanup(func() { close(release) })
			binding, err := bind(t, protocol.ConversationModelApiChatCompletions, server.URL+"/v1", "fixture")
			if err != nil {
				t.Fatal(err)
			}
			ctx, cancel := context.WithCancel(t.Context())
			cancel()
			if _, err = New(ctx, binding); err != nil {
				t.Fatal(err)
			}
			if _, ok := reportedContext(contextKey(binding)); ok {
				t.Fatal("a cancelled probe was remembered")
			}
		})
	})
}
