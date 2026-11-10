package providers

import (
	"encoding/json"
	"reflect"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAccountCatalogAndAuthorization(t *testing.T) {
	t.Parallel()
	t.Run("request identities and destination confinement", func(t *testing.T) {
		provider := protocol.DefaultConversationProvider()
		provider.ID, provider.Api = protocol.NewProviderID(), protocol.ConversationModelApiResponses
		provider.Authentication, provider.Endpoint = protocol.AuthenticationChatGptOAuth, "https://chatgpt.com/backend-api/codex"
		version, account := "0.161.0", "workspace"
		provider.Oauth = &protocol.ConversationOauthOptions{Kind: "chat_gpt_oauth", Data: protocol.ConversationOauthOptionsChatGptOAuthPayload{CatalogVersion: &version, UserAgent: "CapturedAgent/1"}}
		grant := AccountGrant{Kind: provider.Authentication, Access: "private-access", Refresh: "private-refresh", IDToken: "private-identity", Subject: "subject", AccountID: &account, ExpiresAtMs: uint64(time.Now().Add(time.Hour).UnixMilli())}
		secret := func() *protocol.Secret {
			body, err := json.Marshal(grant)
			if err != nil {
				t.Fatal(err)
			}
			value := protocol.NewSecret(string(body))
			return &value
		}
		request, err := accountDiscoveryRequest(t.Context(), provider, secret())
		if err != nil {
			t.Fatal(err)
		}
		if request.URL.String() != "https://chatgpt.com/backend-api/codex/models?client_version=0.161.0" ||
			request.Header.Get("Authorization") != "Bearer private-access" || request.Header.Get("chatgpt-account-id") != account ||
			request.Header.Get("originator") != "codex_cli_rs" || request.Header.Get("version") != version || request.Header.Get("User-Agent") != "CapturedAgent/1" {
			t.Fatalf("incorrect Codex discovery identity: %s", request.URL)
		}
		provider.Endpoint = "https://other.example.test"
		if _, err := accountDiscoveryRequest(t.Context(), provider, secret()); err == nil {
			t.Fatal("account credential escaped its authorized endpoint")
		}
		grant.Kind, grant.ClientID, grant.Scopes = protocol.AuthenticationChatGpt, "oaiapp_fixture", []string{"resource.invoke", "chatgpt.tokens.use.direct"}
		provider.Authentication, provider.Endpoint = grant.Kind, "https://api.openai.com/v1"
		provider.Oauth = &protocol.ConversationOauthOptions{Kind: "chat_gpt", Data: protocol.ConversationOauthOptionsChatGptPayload{CatalogVersion: &version, UserAgent: "CapturedAgent/1"}}
		request, err = accountDiscoveryRequest(t.Context(), provider, secret())
		if err != nil || request.URL.String() != "https://api.openai.com/v1/models" {
			t.Fatalf("incorrect ChatGPT discovery identity: %v", err)
		}
		for _, header := range []string{"chatgpt-account-id", "originator", "version"} {
			if request.Header.Get(header) != "" {
				t.Fatalf("ChatGPT discovery inherited %s", header)
			}
		}
		grant.Scopes = nil
		if _, _, err := AccountHeaders(provider, secret()); err == nil {
			t.Fatal("ChatGPT model use ignored its plan scopes")
		}
		if _, _, err := AccountResourceHeaders(provider, secret(), "https://chatgpt.com/backend-api/accounts/check/v4-2023-04-27"); err != nil {
			t.Fatalf("account resource incorrectly required generation scopes: %v", err)
		}
		if _, _, err := AccountResourceHeaders(provider, secret(), "https://other.example.test/status"); err == nil {
			t.Fatal("account resource escaped its declared origin")
		}
	})
	t.Run("native catalogs retain reported metadata", func(t *testing.T) {
		value := object([]byte(`{"models":[
			{"slug":"z-first","visibility":"list","display_name":"First","context_window":4096,"max_output_tokens":128,"input_modalities":["text","image"],"supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}],"default_reasoning_level":"high","service_tiers":[{"id":"ultrafast"},{"id":"priority"}]},
			{"slug":"hidden","visibility":"hide","context_window":false},
			{"slug":"a-second","visibility":"list","display_name":"Second","supported_reasoning_levels":[{"effort":"future"}],"default_reasoning_level":"future"},
			{"slug":"a-second","visibility":"list","display_name":"Second","supported_reasoning_levels":[{"effort":"future"}],"default_reasoning_level":"future"}
		]}`))
		models, err := parseAccountDiscovery(protocol.AuthenticationChatGptOAuth, protocol.ConversationModelApiResponses, value)
		if err != nil {
			t.Fatal(err)
		}
		if len(models) != 2 || models[0].ID != "z-first" || models[1].ID != "a-second" || *models[0].Context != 4096 || *models[0].Output != 128 ||
			!reflect.DeepEqual(*models[0].Capabilities.Efforts, []protocol.Effort{{Kind: "low"}, {Kind: "high"}}) ||
			models[1].Context != nil || models[1].Capabilities.DefaultEffort != nil || len(*models[1].Capabilities.Efforts) != 0 {
			t.Fatalf("native model metadata changed: %#v", models)
		}
		features := models[0].Capabilities.Features
		if len(features) != 1 || len(features[0].Options) != 2 || features[0].Options[0].ID != "fast" || features[0].Options[1].ID != "ultrafast" {
			t.Fatalf("native service tiers changed: %#v", features)
		}
		models, err = parseAccountDiscovery(protocol.AuthenticationChatGpt, protocol.ConversationModelApiResponses, value)
		if err != nil || len(models[0].Capabilities.Features) != 0 {
			t.Fatal("ChatGPT catalog borrowed Codex service tiers")
		}
		copilot := object([]byte(`{"data":[
			{"id":"responses","capabilities":{"type":"chat","limits":{"max_context_window_tokens":8192,"max_prompt_tokens":4096,"max_output_tokens":1024},"supports":{"vision":false,"tool_calls":true,"reasoning_effort":["none","low","high"]}},"supported_endpoints":["/responses"]},
			{"id":"chat","capabilities":{"type":"chat"}},
			{"id":"embedding","capabilities":{"type":"embeddings"},"supported_endpoints":["/responses"]}
		]}`))
		models, err = parseAccountDiscovery(protocol.AuthenticationCopilot, protocol.ConversationModelApiResponses, copilot)
		if err != nil || len(models) != 1 || models[0].ID != "responses" || *models[0].Context != 8192 || !*models[0].Capabilities.Tools || *models[0].Capabilities.Vision {
			t.Fatalf("Copilot endpoint selection changed: %#v, %v", models, err)
		}
		models, err = parseAccountDiscovery(protocol.AuthenticationCopilot, protocol.ConversationModelApiChatCompletions, copilot)
		if err != nil || len(models) != 1 || models[0].ID != "chat" || models[0].Context != nil {
			t.Fatalf("Copilot chat metadata changed: %#v, %v", models, err)
		}
		for _, document := range []string{
			`{"models":[{"slug":"same","display_name":"First","visibility":"list"},{"slug":"same","display_name":"Second","visibility":"list"}]}`,
			`{"models":[{"slug":"bad","display_name":"Bad","visibility":"list","context_window":2,"max_output_tokens":4}]}`,
			`{"models":[{"slug":"bad","display_name":"Bad","visibility":"list","supported_reasoning_levels":[{"effort":"high"},{"effort":"high"}]}]}`,
			`{"models":[{"slug":"bad","display_name":"Bad","visibility":"list","supported_reasoning_levels":[{"effort":"high"}],"default_reasoning_level":"low"}]}`,
			`{"models":[{"slug":"bad","display_name":"Bad","visibility":"list","input_modalities":[false]}]}`,
			`{"models":[{"slug":"bad","visibility":"list"}]}`,
			`{"models":[{"slug":"bad","display_name":"Bad","visibility":"list","service_tiers":[{"id":"priority"},{"id":"priority"}]}]}`,
		} {
			if _, err := parseAccountDiscovery(protocol.AuthenticationChatGptOAuth, protocol.ConversationModelApiResponses, object([]byte(document))); err == nil {
				t.Fatalf("invalid native metadata was accepted: %s", document)
			}
		}
	})
	t.Run("Copilot grants use the GitHub renewal credential", func(t *testing.T) {
		grant := AccountGrant{Kind: protocol.AuthenticationCopilot, Access: "copilot-access", Github: "github-access", ExpiresAtMs: uint64(time.Now().Add(time.Hour).UnixMilli()), Endpoint: "https://api.githubcopilot.com"}
		body, err := json.Marshal(grant)
		if err != nil || authorization.ValidateGrant(body, grant.Kind, time.Now()) != nil {
			t.Fatalf("valid Copilot grant rejected: %v", err)
		}
		grant.Endpoint = "https://githubcopilot.com.evil.test"
		body, _ = json.Marshal(grant)
		if authorization.ValidateGrant(body, grant.Kind, time.Now()) == nil {
			t.Fatal("Copilot grant escaped its service domain")
		}
	})
}
