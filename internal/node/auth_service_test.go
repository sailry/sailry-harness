package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type accountHTTP func(*http.Request) (*http.Response, error)

func (transport accountHTTP) RoundTrip(request *http.Request) (*http.Response, error) {
	return transport(request)
}

// Over 1 s: each Node start installs and verifies the bundled packages.
func TestProviderAuthorizationAdmissionAndCancellation(t *testing.T) {
	node := nodeFixture(t)
	for _, summary := range snapshotFixture(t, node).Plugins {
		if summary.Name == "openai-auth" {
			commandFixture(t, node, protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{Name: summary.Name, ExpectedRevision: summary.Revision, Enabled: false}})
		}
	}
	source := t.TempDir()
	documents := map[string]string{
		"plugin.json": `{"$schema":"` + plugins.ManifestSchema + `","name":"account-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["models.authenticate","credentials.inspect"],"model_auth":[{"id":"account","authentication":"chat_gpt","display":{"label":"Fixture account"},"client":{"client_id":"oaiapp_fixture"},"scopes":["resource.invoke","chatgpt.tokens.use.direct"],"flow":{"kind":"script","script":{"entry":"dev.sailry.platform/auth.js","resources":[]},"handler":"step","refresh":true,"requests":{"token":{"url":"https://auth.example.test/token","method":"GET"}}}}]}}}`,
		"dev.sailry.platform/auth.js": `export function step(input) {
  if (input.event.kind === "start" || input.event.kind === "refresh") return {kind:"request",name:"token",fields:{},state:null};
  if (input.event.kind === "response") return {kind:"finish",grant:{kind:"chat_gpt",access:input.event.body.access_token,refresh:"private-refresh",expires_at_ms:input.now_ms+3600000,client_id:"oaiapp_fixture",subject:"fixture-subject",email:"account@example.test",id_token:"private-identity",scopes:input.context.scopes,renewal_pending:false}};
  return {kind:"reject",code:"invalid_request"};
}`,
	}
	for name, body := range documents {
		filename := filepath.Join(source, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(filename), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filename, []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	info, err := node.plugins.manager.Install(t.Context(), source, "", "account-fixture")
	if err != nil || len(info.Issues) != 0 {
		t.Fatalf("account fixture package: %v, %+v", err, info.Issues)
	}
	err = node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		var err error
		info, err = store.InstallPlugin(t.Context(), db, info, 0, true)
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	var requests atomic.Int32
	var block atomic.Bool
	started := make(chan struct{}, 1)
	node.auth.client = &http.Client{Transport: accountHTTP(func(request *http.Request) (*http.Response, error) {
		requests.Add(1)
		if request.URL.String() != "https://auth.example.test/token" || request.Method != http.MethodGet {
			return nil, errors.New("unexpected account request")
		}
		if block.Load() {
			started <- struct{}{}
			<-request.Context().Done()
			return nil, request.Context().Err()
		}
		return &http.Response{StatusCode: http.StatusOK, Header: http.Header{"Content-Type": {"application/json"}}, Body: io.NopCloser(strings.NewReader(`{"access_token":"private-access"}`))}, nil
	})}
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Name, provider.Enabled = protocol.NewProviderID(), "Account fixture", true
	provider.Api, provider.Authentication, provider.Endpoint = protocol.ConversationModelApiResponses, protocol.AuthenticationChatGpt, "https://api.openai.com/v1"
	output := commandFixture(t, node, protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider}})
	provider, ok := dispatchPayload[protocol.ConversationProvider](output.Data)
	if !ok {
		t.Fatal("provider save did not return its configuration")
	}
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "begin_provider_login", Data: protocol.CommandBeginProviderLoginPayload{Provider: provider.ID, ExpectedRevision: provider.Revision}})
	first := dispatchFixture(t, node, node.ID(), request)
	if first.Fault != nil {
		t.Fatal(first.Fault)
	}
	attempt, ok := dispatchPayload[protocol.ConversationLoginAttempt](first.Output.Data)
	if !ok {
		t.Fatal("provider login did not return its captured attempt")
	}
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	stream, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "provider_login", Data: attempt.ID})
	if err != nil {
		t.Fatal(err)
	}
	defer stream.Close()
	for {
		update, err := stream.Next(ctx)
		if err != nil {
			t.Fatal(err)
		}
		value, ok := dispatchPayload[protocol.ConversationLoginUpdate](update.Data)
		if !ok {
			t.Fatal("provider login update has invalid shape")
		}
		if value.State.Kind == "failed" {
			t.Fatalf("captured account authorization failed: %#v", value.State.Data)
		}
		if value.State.Kind == "connected" {
			break
		}
	}
	second := dispatchFixture(t, node, node.ID(), request)
	firstBody, _ := json.Marshal(first.Output)
	secondBody, _ := json.Marshal(second.Output)
	if second.Fault != nil || !bytes.Equal(firstBody, secondBody) || requests.Load() != 1 {
		t.Fatalf("durable authorization replay repeated its HTTP effect: %d, %v", requests.Load(), second.Fault)
	}
	otherController := node.ID()
	otherController[0] ^= 1
	if _, err := node.Subscribe(ctx, otherController, protocol.Topic{Kind: "provider_login", Data: attempt.ID}); err == nil {
		t.Fatal("another controller read a private authorization attempt")
	}
	metadata := commandFixture(t, node, protocol.Command{Kind: "list_credentials"})
	body, _ := json.Marshal(metadata)
	if bytes.Contains(body, []byte("private-access")) || !bytes.Contains(body, []byte("account@example.test")) {
		t.Fatalf("public account metadata violated credential protection: %s", body)
	}
	snapshot := snapshotFixture(t, node)
	for _, saved := range snapshot.Providers {
		if saved.ID == provider.ID {
			provider = saved
		}
	}
	if provider.Credential == nil {
		t.Fatal("completed login did not attach its credential to the provider")
	}
	key := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "read_provider_key", Data: protocol.CommandReadProviderKeyPayload{Provider: provider.ID, ExpectedRevision: provider.Revision}}))
	if key.Fault == nil || key.Fault.Code != protocol.ErrorCodePermissionDenied {
		t.Fatal("provider settings exposed an account grant as an API key")
	}
	t.Run("queued account renews its admitted credential", func(t *testing.T) {
		captured := provider
		reference := *captured.Credential
		selected := protocol.CredentialRef{Node: node.ID(), ID: protocol.NewCredentialID()}
		err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			var body []byte
			if err := db.QueryRowContext(ctx, "SELECT body FROM providers WHERE id=?1", captured.ID.String()).Scan(&body); err != nil {
				return err
			}
			var configuration map[string]json.RawMessage
			if err := json.Unmarshal(body, &configuration); err != nil {
				return err
			}
			var authorizations map[protocol.CredentialID]map[string]json.RawMessage
			if err := json.Unmarshal(configuration["authorizations"], &authorizations); err != nil {
				return err
			}
			var secret protocol.Secret
			if err := json.Unmarshal(authorizations[reference.ID]["key"], &secret); err != nil {
				return err
			}
			grant, err := providers.DecodeAccount(&secret, captured.Authentication)
			if err != nil {
				return err
			}
			update := func(id protocol.CredentialID, grant providers.AccountGrant) error {
				entry := map[string]json.RawMessage{}
				for name, value := range authorizations[reference.ID] {
					entry[name] = value
				}
				body, err := json.Marshal(grant)
				if err != nil {
					return err
				}
				entry["key"], err = json.Marshal(protocol.NewSecret(string(body)))
				if err != nil {
					return err
				}
				entry["expires_at_ms"], err = json.Marshal(grant.ExpiresAtMs)
				authorizations[id] = entry
				return err
			}
			grant.ExpiresAtMs = uint64(time.Now().Add(-time.Minute).UnixMilli())
			if err := update(reference.ID, grant); err != nil {
				return err
			}
			grant.Access, grant.Subject, grant.ExpiresAtMs = "selected-access", "selected-subject", uint64(time.Now().Add(time.Hour).UnixMilli())
			if err := update(selected.ID, grant); err != nil {
				return err
			}
			configuration["authorizations"], err = json.Marshal(authorizations)
			if err != nil {
				return err
			}
			current := captured
			current.Credential, current.Revision = &selected, captured.Revision+1
			configuration["provider"], err = json.Marshal(current)
			if err != nil {
				return err
			}
			body, err = json.Marshal(configuration)
			if err != nil {
				return err
			}
			_, err = db.ExecContext(ctx, "UPDATE providers SET body=?2 WHERE id=?1", captured.ID.String(), body)
			return err
		})
		if err != nil {
			t.Fatal(err)
		}
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Credential = captured.ID, &reference
		invocation := &store.AgentInvocation{Provider: &captured, Turn: protocol.QueuedTurn{Config: config},
			AuthorizationFailure: &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "provider authorization expired"}}
		if err := node.authorizeTurnProvider(ctx, invocation); err != nil {
			t.Fatal(err)
		}
		grant, err := providers.DecodeAccount(invocation.Secret, captured.Authentication)
		if err != nil || invocation.AuthorizationFailure != nil || grant.Access != "private-access" || requests.Load() != 2 {
			t.Fatalf("queued preparation did not renew its admitted account: %v", err)
		}
		for _, current := range snapshotFixture(t, node).Providers {
			if current.ID == captured.ID {
				provider = current
			}
		}
		if provider.Credential == nil || *provider.Credential != selected {
			t.Fatal("queued renewal replaced the current selected account")
		}
		var currentSecret *protocol.Secret
		err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			currentSecret, err = store.ReadProviderAuthorization(ctx, db, node.ID(), provider, selected)
			return err
		})
		currentGrant, decodeErr := providers.DecodeAccount(currentSecret, provider.Authentication)
		if err != nil || decodeErr != nil || currentGrant.Access != "selected-access" {
			t.Fatal("queued renewal wrote another account's credential")
		}
	})
	t.Run("renewal tells a refused connection from a delivered request", func(t *testing.T) {
		listener, err := net.Listen("tcp", "127.0.0.1:0")
		if err != nil {
			t.Fatal(err)
		}
		address := listener.Addr().String()
		if err := listener.Close(); err != nil {
			t.Fatal(err)
		}
		exchange := func(base http.RoundTripper) bool {
			transport := &renewalTransport{base: base}
			request, err := http.NewRequestWithContext(ctx, http.MethodPost, "http://"+address+"/token", nil)
			if err != nil {
				t.Fatal(err)
			}
			if response, err := transport.RoundTrip(request); err == nil {
				response.Body.Close()
			}
			return transport.sent.Load()
		}
		if exchange(&http.Transport{}) {
			t.Fatal("a refused connection counted as a delivered request")
		}
		for name, base := range map[string]accountHTTP{
			"answered": func(*http.Request) (*http.Response, error) {
				return &http.Response{StatusCode: http.StatusBadGateway, Body: io.NopCloser(strings.NewReader(""))}, nil
			},
			"reset after writing": func(*http.Request) (*http.Response, error) {
				return nil, &net.OpError{Op: "read", Net: "tcp", Err: errors.New("connection reset")}
			},
			"cancelled": func(*http.Request) (*http.Response, error) { return nil, context.Canceled },
		} {
			if !exchange(base) {
				t.Errorf("%s: an exchange that may have been delivered counted as undelivered", name)
			}
		}
	})
	block.Store(true)
	output = commandFixture(t, node, protocol.Command{Kind: "begin_provider_login", Data: protocol.CommandBeginProviderLoginPayload{Provider: provider.ID, ExpectedRevision: provider.Revision}})
	cancelled, ok := dispatchPayload[protocol.ConversationLoginAttempt](output.Data)
	if !ok {
		t.Fatal("new authorization did not return its attempt")
	}
	select {
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	case <-started:
	}
	commandFixture(t, node, protocol.Command{Kind: "cancel_provider_login", Data: protocol.CommandCancelProviderLoginPayload{Attempt: cancelled.ID}})
	cancelledStream, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "provider_login", Data: cancelled.ID})
	if err != nil {
		t.Fatal(err)
	}
	defer cancelledStream.Close()
	update, err := cancelledStream.Next(ctx)
	value, ok := dispatchPayload[protocol.ConversationLoginUpdate](update.Data)
	if err != nil || !ok || value.State.Kind != "cancelled" {
		t.Fatalf("cancellation did not settle the attempt: %#v, %v", value, err)
	}
}

// TestCopilotSignIn drives the bundled Copilot sign-in package through the
// Node's workflow driver against a local server standing in for GitHub.
// Over 3 s: a Node start, then two polls at GitHub's one-second interval.
func TestCopilotSignIn(t *testing.T) {
	node := nodeFixture(t)
	var polls, exchanges atomic.Int32
	answer := func(writer http.ResponseWriter, status int, body string) {
		writer.Header().Set("Content-Type", "application/json")
		writer.WriteHeader(status)
		_, _ = io.WriteString(writer, body)
	}
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		switch request.Host + request.URL.Path {
		case "github.com/login/device/code":
			if request.Method != http.MethodPost || request.ParseForm() != nil || request.PostForm.Get("client_id") != "Iv1.b507a08c87ecfe98" || request.PostForm.Get("scope") != "read:user" || request.Header.Get("Accept") != "application/json" {
				t.Errorf("device request: %s %v", request.Method, request.PostForm)
			}
			answer(writer, http.StatusOK, `{"device_code":"private-device","user_code":"ABCD-1234","verification_uri":"https://github.com/login/device","expires_in":600,"interval":1}`)
		case "github.com/login/oauth/access_token":
			if request.Method != http.MethodPost || request.ParseForm() != nil || request.PostForm.Get("client_id") != "Iv1.b507a08c87ecfe98" || request.PostForm.Get("device_code") != "private-device" ||
				request.PostForm.Get("grant_type") != "urn:ietf:params:oauth:grant-type:device_code" {
				t.Errorf("token poll: %s %v", request.Method, request.PostForm)
			}
			if polls.Add(1) == 1 {
				answer(writer, http.StatusOK, `{"error":"authorization_pending"}`)
				return
			}
			answer(writer, http.StatusOK, `{"access_token":"github-token","token_type":"bearer","scope":"read:user"}`)
		case "api.github.com/copilot_internal/v2/token":
			if request.Method != http.MethodGet || request.Header.Get("Authorization") != "token github-token" || request.Header.Get("Editor-Version") != providers.DefaultCopilotEditor ||
				request.Header.Get("Editor-Plugin-Version") != providers.DefaultCopilotEditorPlugin || request.Header.Get("User-Agent") != providers.DefaultCopilotAgent {
				t.Errorf("session exchange: %s %v", request.Method, request.Header)
			}
			// The first two sessions end within the renewal window; the third
			// exchange finds the GitHub token revoked.
			switch count := exchanges.Add(1); count {
			case 1, 2:
				body, _ := json.Marshal(map[string]any{"token": "session-" + string(rune('0'+count)), "expires_at": time.Now().Add(330 * time.Second).Unix(),
					"endpoints": map[string]string{"api": "https://api.individual.githubcopilot.com"}})
				answer(writer, http.StatusOK, string(body))
			default:
				answer(writer, http.StatusUnauthorized, `{"message":"Bad credentials"}`)
			}
		default:
			t.Errorf("unexpected request to %s%s", request.Host, request.URL.Path)
			answer(writer, http.StatusNotFound, `null`)
		}
	}))
	defer server.Close()
	// The declared hosts are fixed; only the connection goes to the local server.
	node.auth.client = &http.Client{Transport: accountHTTP(func(request *http.Request) (*http.Response, error) {
		forwarded := request.Clone(request.Context())
		target := *request.URL
		target.Scheme, target.Host = "http", server.Listener.Addr().String()
		forwarded.URL, forwarded.Host = &target, request.URL.Host
		return http.DefaultTransport.RoundTrip(forwarded)
	})}
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Name, provider.Enabled = protocol.NewProviderID(), "Copilot", true
	provider.Api, provider.Authentication, provider.Endpoint = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationCopilot, "https://api.githubcopilot.com"
	output := commandFixture(t, node, protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider}})
	provider, ok := dispatchPayload[protocol.ConversationProvider](output.Data)
	if !ok {
		t.Fatal("provider save did not return its configuration")
	}
	output = commandFixture(t, node, protocol.Command{Kind: "begin_provider_login", Data: protocol.CommandBeginProviderLoginPayload{Provider: provider.ID, ExpectedRevision: provider.Revision}})
	attempt, ok := dispatchPayload[protocol.ConversationLoginAttempt](output.Data)
	if !ok {
		t.Fatal("provider login did not return its captured attempt")
	}
	ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
	defer cancel()
	stream, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "provider_login", Data: attempt.ID})
	if err != nil {
		t.Fatal(err)
	}
	defer stream.Close()
	prompted := false
	for connected := false; !connected; {
		update, err := stream.Next(ctx)
		if err != nil {
			t.Fatal(err)
		}
		value, ok := dispatchPayload[protocol.ConversationLoginUpdate](update.Data)
		if !ok {
			t.Fatal("provider login update has invalid shape")
		}
		switch value.State.Kind {
		case "pending":
			prompt, ok := dispatchPayload[protocol.ConversationLoginStatePendingPayload](value.State.Data)
			if !ok || prompt.UserCode != "ABCD-1234" || prompt.VerificationUrl != "https://github.com/login/device" {
				t.Fatalf("device prompt: %#v", value.State.Data)
			}
			prompted = true
		case "failed", "cancelled":
			t.Fatalf("Copilot sign-in ended %s: %#v", value.State.Kind, value.State.Data)
		case "connected":
			connected = true
		}
	}
	if !prompted || polls.Load() != 2 || exchanges.Load() != 1 {
		t.Fatalf("sign-in prompted=%v with %d polls and %d exchanges", prompted, polls.Load(), exchanges.Load())
	}
	for _, saved := range snapshotFixture(t, node).Providers {
		if saved.ID == provider.ID {
			provider = saved
		}
	}
	if provider.Credential == nil {
		t.Fatal("completed sign-in did not attach its credential")
	}
	listed, _ := json.Marshal(commandFixture(t, node, protocol.Command{Kind: "list_credentials"}))
	if bytes.Contains(listed, []byte("github-token")) || bytes.Contains(listed, []byte("session-1")) {
		t.Fatalf("credential metadata exposed a Copilot secret: %s", listed)
	}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Credential = provider.ID, provider.Credential
	authorize := func() (providers.AccountGrant, error) {
		invocation := &store.AgentInvocation{Provider: &provider, Turn: protocol.QueuedTurn{Config: config}}
		if err := node.authorizeTurnProvider(ctx, invocation); err != nil {
			return providers.AccountGrant{}, err
		}
		return providers.DecodeAccount(invocation.Secret, provider.Authentication)
	}
	// The stored session is within a minute of its early expiry, so the next
	// use repeats the exchange with the GitHub token and keeps the pinned endpoint.
	grant, err := authorize()
	if err != nil || grant.Access != "session-2" || grant.Github != "github-token" || grant.Endpoint != "https://api.githubcopilot.com" || grant.RenewalPending || exchanges.Load() != 2 {
		t.Fatalf("renewal returned %+v after %d exchanges: %v", grant, exchanges.Load(), err)
	}
	if _, headers, err := providers.AccountHeaders(provider, func() *protocol.Secret {
		body, _ := json.Marshal(grant)
		secret := protocol.NewSecret(string(body))
		return &secret
	}()); err != nil || headers.Get("Authorization") != "Bearer session-2" {
		t.Fatalf("renewed grant does not authorize its channel: %v", err)
	}
	// A revoked GitHub token is a refusal: the grant is marked and later uses
	// ask for a new sign-in without another request.
	var fault *protocol.Fault
	if _, err := authorize(); !errors.As(err, &fault) || fault.Code != protocol.ErrorCodePermissionDenied || exchanges.Load() != 3 {
		t.Fatalf("revoked sign-in returned %v after %d exchanges", err, exchanges.Load())
	}
	if _, err := authorize(); !errors.As(err, &fault) || fault.Code != protocol.ErrorCodePermissionDenied || exchanges.Load() != 3 {
		t.Fatalf("rejected grant was renewed again: %v after %d exchanges", err, exchanges.Load())
	}
}
