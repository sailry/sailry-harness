package model

import (
	"bufio"
	"bytes"
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/aws/aws-sdk-go-v2/aws/protocol/eventstream"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/oauth2"
)

func cloudProvider(api protocol.ConversationModelApi, endpoint string, options protocol.ConversationConnectionOptions, model string) (protocol.ConversationProvider, protocol.SessionConfig) {
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Enabled, provider.Name = protocol.NewProviderID(), true, "Cloud"
	provider.Api, provider.Authentication, provider.Endpoint = api, protocol.AuthenticationApiKey, endpoint
	provider.Options = &options
	selection := protocol.DefaultSessionConfig()
	selection.Provider, selection.Model = provider.ID, model
	selection.Mode, selection.Permission = protocol.WorkModeCode, protocol.PermissionAsk
	return provider, selection
}

func cloudCall(t *testing.T, binding Binding) fantasy.Call {
	t.Helper()
	record := canonical.NewRecord(protocol.NewTurnID(), "message", canonical.NewMessage(canonical.User, canonical.Text(0, "hello")), policyClock{}.now)
	call, err := BuildCall(binding, Request{Records: []canonical.Record{record}})
	if err != nil {
		t.Fatal(err)
	}
	return call
}

// streamParts runs a stream to its end and returns its parts and failure.
func streamParts(t *testing.T, model fantasy.LanguageModel, call fantasy.Call) ([]fantasy.StreamPart, error) {
	t.Helper()
	stream, err := model.Stream(t.Context(), call)
	if err != nil {
		return nil, err
	}
	var parts []fantasy.StreamPart
	for part := range stream {
		parts = append(parts, part)
		if part.Type == fantasy.StreamPartTypeError {
			return parts, part.Error
		}
	}
	return parts, nil
}

func streamText(parts []fantasy.StreamPart) string {
	var text strings.Builder
	for _, part := range parts {
		if part.Type == fantasy.StreamPartTypeTextDelta {
			text.WriteString(part.Delta)
		}
	}
	return text.String()
}

// eventStream encodes an Anthropic SSE fixture as Bedrock's EventStream
// response: each event's JSON, base64-encoded in a chunk event.
func eventStream(t *testing.T, sse []byte, exception string) []byte {
	t.Helper()
	var out bytes.Buffer
	encoder := eventstream.NewEncoder()
	scanner := bufio.NewScanner(bytes.NewReader(sse))
	scanner.Buffer(make([]byte, 1<<20), 1<<20)
	for scanner.Scan() {
		data, ok := strings.CutPrefix(scanner.Text(), "data: ")
		if !ok {
			continue
		}
		payload, _ := json.Marshal(map[string]string{"bytes": base64.StdEncoding.EncodeToString([]byte(data))})
		message := eventstream.Message{Payload: payload}
		message.Headers.Set(":message-type", eventstream.StringValue("event"))
		message.Headers.Set(":event-type", eventstream.StringValue("chunk"))
		message.Headers.Set(":content-type", eventstream.StringValue("application/json"))
		if err := encoder.Encode(&out, message); err != nil {
			t.Fatal(err)
		}
		if exception != "" {
			// The exception follows the stream's first event.
			message := eventstream.Message{Payload: []byte(`{"message":"slow down"}`)}
			message.Headers.Set(":message-type", eventstream.StringValue("exception"))
			message.Headers.Set(":exception-type", eventstream.StringValue(exception))
			if err := encoder.Encode(&out, message); err != nil {
				t.Fatal(err)
			}
			break
		}
	}
	return out.Bytes()
}

func TestBedrockMessagesRoute(t *testing.T) {
	// The process's ambient Bedrock token must not replace the provider's key.
	t.Setenv("AWS_BEARER_TOKEN_BEDROCK", "ambient-token")
	const key, model = "bedrock-key", "us.anthropic.claude-sonnet-4-6"
	response := eventStream(t, fixture(t, "anthropic-text.sse"), "")
	var requests atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		if got := r.URL.EscapedPath(); got != "/model/us.anthropic.claude-sonnet-4-6/invoke-with-response-stream" || r.URL.RawQuery != "" {
			t.Errorf("Bedrock request %s %s?%s", r.Method, got, r.URL.RawQuery)
		}
		if r.Header.Get("Authorization") != "Bearer "+key || r.Header.Get("X-Api-Key") != "" || r.Header.Get("anthropic-beta") != "" {
			t.Errorf("Bedrock headers: authorization %q, x-api-key %q, beta %q", r.Header.Get("Authorization"), r.Header.Get("X-Api-Key"), r.Header.Get("anthropic-beta"))
		}
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Errorf("decode Bedrock body: %v", err)
		}
		checkValue(t, body, "anthropic_version", "bedrock-2023-05-31")
		for _, absent := range []string{"model", "stream", "metadata"} {
			if _, ok := body[absent]; ok {
				t.Errorf("Bedrock body carries %q", absent)
			}
		}
		// The Claude Code identity is a first-party gateway convention.
		if system, _ := json.Marshal(body["system"]); bytes.Contains(system, []byte(claudeIdentity)) {
			t.Error("Bedrock body carries the Claude Code identity")
		}
		// Adaptive thinking comes from the Claude name behind the profile.
		checkValue(t, body, "thinking.type", "adaptive")
		w.Header().Set("Content-Type", "application/vnd.amazon.eventstream")
		w.Write(response)
	}))
	t.Cleanup(server.Close)
	provider, selection := cloudProvider(protocol.ConversationModelApiBedrock, server.URL,
		protocol.ConversationConnectionOptions{Kind: "bedrock", Data: protocol.ConversationConnectionOptionsBedrockPayload{Region: "us-west-2"}}, model)
	selection.Effort = protocol.Effort{Kind: "high"}
	secret := protocol.NewSecret(key)
	binding, err := Capture(provider, selection, &secret, protocol.NewSessionID())
	if err != nil {
		t.Fatal(err)
	}
	config, err := transportBinding(binding)
	if err != nil || !config.Bedrock() || config.Kind != "anthropic" || config.Region != "us-west-2" || config.AnthropicAPIKey {
		t.Fatalf("Bedrock route = %+v, %v", config, err)
	}
	language, err := New(t.Context(), binding)
	if err != nil {
		t.Fatal(err)
	}
	parts, err := streamParts(t, language, cloudCall(t, binding))
	if err != nil {
		t.Fatalf("Bedrock stream failed: %v", err)
	}
	if streamText(parts) == "" || parts[len(parts)-1].Type != fantasy.StreamPartTypeFinish || requests.Load() != 1 {
		t.Errorf("Bedrock stream parts %+v after %d requests", parts, requests.Load())
	}
	if limit := ContextLimit(binding); limit != 1000000 {
		t.Errorf("Bedrock profile context limit = %d", limit)
	}
}

func TestBedrockFailures(t *testing.T) {
	t.Parallel()
	for _, row := range []struct {
		name    string
		respond func(w http.ResponseWriter)
		kind    FailureKind
	}{
		{"forbidden", func(w http.ResponseWriter) {
			w.Header().Set("Content-Type", "application/json")
			w.Header().Set("X-Amzn-ErrorType", "AccessDeniedException:http://internal.amazon.com/coral/com.amazon.bedrock/")
			w.WriteHeader(http.StatusForbidden)
			io.WriteString(w, `{"message":"Authentication failed: invalid API key"}`)
		}, Authentication},
		{"unauthorized", func(w http.ResponseWriter) {
			w.WriteHeader(http.StatusUnauthorized)
			io.WriteString(w, `{"message":"Invalid API Key format"}`)
		}, Authentication},
		{"throttled", func(w http.ResponseWriter) {
			w.WriteHeader(http.StatusTooManyRequests)
			io.WriteString(w, `{"message":"Too many requests"}`)
		}, RateLimited},
		{"stream exception", func(w http.ResponseWriter) {
			w.Header().Set("Content-Type", "application/vnd.amazon.eventstream")
			w.Write(eventStream(t, fixture(t, "anthropic-text.sse"), "throttlingException"))
		}, RateLimited},
	} {
		t.Run(row.name, func(t *testing.T) {
			t.Parallel()
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { row.respond(w) }))
			t.Cleanup(server.Close)
			provider, selection := cloudProvider(protocol.ConversationModelApiBedrock, server.URL,
				protocol.ConversationConnectionOptions{Kind: "bedrock", Data: protocol.ConversationConnectionOptionsBedrockPayload{Region: "eu-west-1"}}, "eu.anthropic.claude-haiku-4-5-20251001-v1:0")
			secret := protocol.NewSecret("bedrock-key")
			binding, err := Capture(provider, selection, &secret, protocol.NewSessionID())
			if err != nil {
				t.Fatal(err)
			}
			language, err := New(t.Context(), binding)
			if err != nil {
				t.Fatal(err)
			}
			_, err = streamParts(t, language, cloudCall(t, binding))
			if failure := Classify(err, secret); failure == nil || failure.Kind != row.kind {
				t.Errorf("failure %+v; want %s", failure, row.kind)
			}
		})
	}
}

// serviceAccountJSON is a service-account key with a fresh RSA key whose
// token endpoint is the fake server's.
func serviceAccountJSON(t *testing.T, tokenURL string) (string, string) {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	der, err := x509.MarshalPKCS8PrivateKey(key)
	if err != nil {
		t.Fatal(err)
	}
	private := string(pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: der}))
	raw, _ := json.Marshal(map[string]string{
		"type": "service_account", "project_id": "proj-123", "private_key_id": "key-1", "private_key": private,
		"client_email": "sailry@proj-123.iam.gserviceaccount.com", "token_uri": tokenURL,
	})
	return string(raw), private
}

// vertexServer is a fake token endpoint and Vertex AI service. Each minted
// token is numbered; lifetime is the expires_in it reports.
type vertexServer struct {
	*httptest.Server
	minted, generated atomic.Int32
	lifetime          int
	tokenStatus       int
	authorizations    chan string
}

func newVertexServer(t *testing.T, lifetime int) *vertexServer {
	t.Helper()
	fake := &vertexServer{lifetime: lifetime, authorizations: make(chan string, 8)}
	response := fixture(t, "google-text.sse")
	fake.Server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/token":
			if err := r.ParseForm(); err != nil || r.Form.Get("grant_type") != "urn:ietf:params:oauth:grant-type:jwt-bearer" {
				t.Errorf("token request form %v: %v", r.Form, err)
			}
			parts := strings.Split(r.Form.Get("assertion"), ".")
			var claims struct {
				Issuer   string `json:"iss"`
				Scope    string `json:"scope"`
				Audience string `json:"aud"`
			}
			if len(parts) != 3 {
				t.Errorf("assertion is not a JWT")
			} else if payload, err := base64.RawURLEncoding.DecodeString(parts[1]); err != nil || json.Unmarshal(payload, &claims) != nil {
				t.Errorf("assertion claims are invalid: %v", err)
			}
			if claims.Issuer != "sailry@proj-123.iam.gserviceaccount.com" || claims.Scope != vertexScope || claims.Audience != fake.URL+"/token" {
				t.Errorf("assertion claims %+v", claims)
			}
			if fake.tokenStatus != 0 {
				w.WriteHeader(fake.tokenStatus)
				io.WriteString(w, `{"error":"invalid_grant","error_description":"Invalid JWT Signature."}`)
				return
			}
			n := fake.minted.Add(1)
			w.Header().Set("Content-Type", "application/json")
			fmt.Fprintf(w, `{"access_token":"token-%d","expires_in":%d,"token_type":"Bearer"}`, n, fake.lifetime)
		case "/v1beta1/projects/proj-123/locations/us-central1/publishers/google/models/gemini-3.5-flash:streamGenerateContent":
			fake.generated.Add(1)
			if r.URL.Query().Get("alt") != "sse" || r.Header.Get("X-Goog-Api-Key") != "" {
				t.Errorf("Vertex request query %q, API key header %q", r.URL.RawQuery, r.Header.Get("X-Goog-Api-Key"))
			}
			fake.authorizations <- r.Header.Get("Authorization")
			w.Header().Set("Content-Type", "text/event-stream")
			w.Write(response)
		default:
			t.Errorf("unexpected request %s %s", r.Method, r.URL.Path)
			http.NotFound(w, r)
		}
	}))
	t.Cleanup(fake.Close)
	return fake
}

func vertexBinding(t *testing.T, endpoint, account string) Binding {
	t.Helper()
	provider, selection := cloudProvider(protocol.ConversationModelApiVertexAi, endpoint,
		protocol.ConversationConnectionOptions{Kind: "vertex_ai", Data: protocol.ConversationConnectionOptionsVertexAiPayload{Project: "proj-123", Location: "us-central1"}}, "gemini-3.5-flash")
	secret := protocol.NewSecret(account)
	binding, err := Capture(provider, selection, &secret, protocol.NewSessionID())
	if err != nil {
		t.Fatal(err)
	}
	return binding
}

func TestVertexGeminiRoute(t *testing.T) {
	t.Setenv("GOOGLE_API_KEY", "ambient-key")
	t.Setenv("GOOGLE_VERTEX_BASE_URL", "https://ambient.invalid")
	for _, row := range []struct {
		name     string
		lifetime int
		tokens   []string
	}{
		// A token outliving the refresh margin serves both requests.
		{"cached", 3600, []string{"Bearer token-1", "Bearer token-1"}},
		// A token within the margin is minted again before each request.
		{"refreshed", 30, []string{"Bearer token-1", "Bearer token-2"}},
	} {
		t.Run(row.name, func(t *testing.T) {
			fake := newVertexServer(t, row.lifetime)
			account, _ := serviceAccountJSON(t, fake.URL+"/token")
			binding := vertexBinding(t, fake.URL, account)
			config, err := transportBinding(binding)
			if err != nil || config.Kind != "google" || config.Cloud != protocol.ConversationModelApiVertexAi || config.Project != "proj-123" || config.Location != "us-central1" {
				t.Fatalf("Vertex route = %+v, %v", config, err)
			}
			for i := range row.tokens {
				language, err := New(t.Context(), binding)
				if err != nil {
					t.Fatal(err)
				}
				parts, err := streamParts(t, language, cloudCall(t, binding))
				if err != nil {
					t.Fatalf("Vertex stream %d failed: %v", i, err)
				}
				if streamText(parts) != "Read complete" {
					t.Errorf("Vertex stream %d text %q", i, streamText(parts))
				}
				if got := <-fake.authorizations; got != row.tokens[i] {
					t.Errorf("Vertex request %d authorization %q; want %q", i, got, row.tokens[i])
				}
			}
			if int(fake.minted.Load()) != len(slicesUnique(row.tokens)) {
				t.Errorf("minted %d tokens", fake.minted.Load())
			}
			if limit := ContextLimit(binding); limit != 1048576 {
				t.Errorf("Vertex Gemini context limit = %d", limit)
			}
		})
	}
}

func slicesUnique(values []string) map[string]bool {
	seen := map[string]bool{}
	for _, value := range values {
		seen[value] = true
	}
	return seen
}

func TestVertexTokenFailure(t *testing.T) {
	t.Parallel()
	fake := newVertexServer(t, 3600)
	fake.tokenStatus = http.StatusBadRequest
	account, private := serviceAccountJSON(t, fake.URL+"/token")
	binding := vertexBinding(t, fake.URL, account)
	language, err := New(t.Context(), binding)
	if err != nil {
		t.Fatal(err)
	}
	_, err = streamParts(t, language, cloudCall(t, binding))
	failure := Classify(err)
	if failure == nil || failure.Kind != Authentication || fake.generated.Load() != 0 {
		t.Fatalf("token failure %+v after %d model requests", failure, fake.generated.Load())
	}
	for _, secret := range []string{private, "invalid_grant", "Invalid JWT"} {
		if strings.Contains(failure.Message, secret) {
			t.Errorf("token failure repeats %q: %q", secret, failure.Message)
		}
	}
}

func TestCloudCredentialProtection(t *testing.T) {
	t.Parallel()
	account, private := serviceAccountJSON(t, "https://oauth2.googleapis.com/token")
	binding := vertexBinding(t, "https://us-central1-aiplatform.googleapis.com", account)
	config, err := transportBinding(binding)
	if err != nil {
		t.Fatal(err)
	}
	// Run redacts the route's protection and key from every failure.
	secrets := append(config.Protection, protocol.NewSecret(config.Key))
	keyLine := strings.Split(private, "\n")[1]
	failure := Classify(errors.New("upstream echoed "+account+" and "+private), secrets...)
	if strings.Contains(failure.Message, keyLine) || !strings.Contains(failure.Message, "[redacted]") {
		t.Errorf("failure message keeps the service account key: %q", failure.Message)
	}
	// An unusable key is refused without repeating it.
	for _, raw := range []string{"", `{"type":"authorized_user","client_email":"a@b"}`, strings.Replace(account, "BEGIN PRIVATE KEY", "BEGIN NOTHING", 1)[:40],
		strings.Replace(account, "https://oauth2.googleapis.com/token", "http://tokens.example/token", 1),
		// Keys the JWT grant cannot sign with are refused before any request.
		serviceAccountKey(t, account, "PRIVATE KEY", ecdsaKey(t)), serviceAccountKey(t, account, "PUBLIC KEY", rsaPublicKey(t)),
		serviceAccountKey(t, account, "PRIVATE KEY", []byte("not a key"))} {
		bad := vertexBinding(t, "https://us-central1-aiplatform.googleapis.com", account)
		value := protocol.NewSecret(raw)
		bad.Secret = &value
		_, err := transportBinding(bad)
		var failure *Failure
		if !errors.As(err, &failure) || failure.Kind != NotConfigured || raw != "" && strings.Contains(failure.Message, raw) {
			t.Errorf("service account %.30q: %v", raw, err)
		}
	}
	// Options that name no region, or a pathed endpoint, are refused.
	provider, selection := cloudProvider(protocol.ConversationModelApiBedrock, "https://bedrock-runtime.us-east-1.amazonaws.com",
		protocol.ConversationConnectionOptions{Kind: "bedrock", Data: protocol.ConversationConnectionOptionsBedrockPayload{Region: "US EAST"}}, "anthropic.claude-opus-4-8")
	secret := protocol.NewSecret("bedrock-key")
	if _, err := Capture(provider, selection, &secret, protocol.NewSessionID()); err == nil {
		t.Error("a Bedrock region with spaces was captured")
	}
	provider.Options = &protocol.ConversationConnectionOptions{Kind: "bedrock", Data: protocol.ConversationConnectionOptionsBedrockPayload{Region: "us-east-1"}}
	provider.Endpoint += "/v1"
	if _, err := Capture(provider, selection, &secret, protocol.NewSessionID()); err == nil {
		t.Error("a Bedrock endpoint with a path was captured")
	}
}

// serviceAccountKey is the account with its private key replaced by a PEM
// block of the type and bytes given.
func serviceAccountKey(t *testing.T, account, kind string, der []byte) string {
	t.Helper()
	var document map[string]string
	if err := json.Unmarshal([]byte(account), &document); err != nil {
		t.Fatal(err)
	}
	document["private_key"] = string(pem.EncodeToMemory(&pem.Block{Type: kind, Bytes: der}))
	raw, _ := json.Marshal(document)
	return string(raw)
}

func ecdsaKey(t *testing.T) []byte {
	t.Helper()
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	der, err := x509.MarshalPKCS8PrivateKey(key)
	if err != nil {
		t.Fatal(err)
	}
	return der
}

func rsaPublicKey(t *testing.T) []byte {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	der, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		t.Fatal(err)
	}
	return der
}

// tokenServer is a token endpoint whose replies the test sets: each request
// takes the next reply, and a nil reply blocks until the test sends one.
type tokenServer struct {
	*httptest.Server
	replies chan func(http.ResponseWriter)
	minted  atomic.Int32
}

func newTokenServer(t *testing.T) *tokenServer {
	t.Helper()
	fake := &tokenServer{replies: make(chan func(http.ResponseWriter), 8)}
	fake.Server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		select {
		case reply := <-fake.replies:
			reply(w)
		case <-r.Context().Done():
		}
	}))
	t.Cleanup(fake.Close)
	return fake
}

func (fake *tokenServer) grant(lifetime int) {
	fake.replies <- func(w http.ResponseWriter) {
		n := fake.minted.Add(1)
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprintf(w, `{"access_token":"token-%d","expires_in":%d,"token_type":"Bearer"}`, n, lifetime)
	}
}

func (fake *tokenServer) fail(status int) {
	fake.replies <- func(w http.ResponseWriter) { w.WriteHeader(status) }
}

func testServiceAccount(t *testing.T, tokenURL string) *serviceAccount {
	t.Helper()
	raw, _ := serviceAccountJSON(t, tokenURL)
	account, err := parseServiceAccount(raw)
	if err != nil {
		t.Fatal(err)
	}
	return account
}

func TestVertexTokenCache(t *testing.T) {
	t.Parallel()
	t.Run("waiters keep their own cancellation", func(t *testing.T) {
		t.Parallel()
		fake := newTokenServer(t)
		account := testServiceAccount(t, fake.URL)
		minted := make(chan string, 1)
		go func() {
			token, _ := account.accessToken(t.Context())
			minted <- token
		}()
		// Wait until the first request holds the mint.
		for entry := account.cached(); ; time.Sleep(time.Millisecond) {
			entry.mu.Lock()
			minting := entry.minting != nil
			entry.mu.Unlock()
			if minting {
				break
			}
		}
		ctx, cancel := context.WithCancel(t.Context())
		cancel()
		if _, err := account.accessToken(ctx); Classify(err).Kind != Cancelled {
			t.Fatalf("a cancelled waiter returned %v", err)
		}
		fake.grant(3600)
		if token := <-minted; token != "token-1" {
			t.Fatalf("the minting request received %q", token)
		}
		if token, err := account.accessToken(t.Context()); err != nil || token != "token-1" || fake.minted.Load() != 1 {
			t.Fatalf("the cached token is %q, %v after %d mints", token, err, fake.minted.Load())
		}
	})
	t.Run("a valid token outlives a failed refresh", func(t *testing.T) {
		t.Parallel()
		fake := newTokenServer(t)
		account := testServiceAccount(t, fake.URL)
		// Thirty seconds is within the refresh margin but not yet expired.
		fake.grant(30)
		if token, err := account.accessToken(t.Context()); err != nil || token != "token-1" {
			t.Fatalf("first token %q, %v", token, err)
		}
		fake.fail(http.StatusServiceUnavailable)
		if token, err := account.accessToken(t.Context()); err != nil || token != "token-1" {
			t.Fatalf("a failed refresh discarded a valid token: %q, %v", token, err)
		}
		fake.grant(3600)
		if token, err := account.accessToken(t.Context()); err != nil || token != "token-2" {
			t.Fatalf("the refresh after a failure returned %q, %v", token, err)
		}
	})
	t.Run("a refused token is minted again", func(t *testing.T) {
		t.Parallel()
		fake := newTokenServer(t)
		account := testServiceAccount(t, fake.URL)
		var refuse atomic.Bool
		refuse.Store(true)
		service := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if refuse.Swap(false) {
				w.WriteHeader(http.StatusUnauthorized)
				return
			}
			io.WriteString(w, r.Header.Get("Authorization"))
		}))
		t.Cleanup(service.Close)
		client := &http.Client{Transport: vertexTransport{base: http.DefaultTransport, account: account}}
		fake.grant(3600)
		fake.grant(3600)
		for i, want := range []string{"", "Bearer token-2"} {
			response, err := client.Get(service.URL)
			if err != nil {
				t.Fatal(err)
			}
			body, _ := io.ReadAll(response.Body)
			response.Body.Close()
			if string(body) != want {
				t.Errorf("request %d (HTTP %d) authorized with %q; want %q", i, response.StatusCode, body, want)
			}
		}
	})
}

// TestVertexTokenEviction replaces the shared cache, so it does not run in
// parallel with the tests that use it.
func TestVertexTokenEviction(t *testing.T) {
	vertexTokens.Lock()
	saved := vertexTokens.entries
	entries := map[[32]byte]*vertexToken{}
	now := time.Now()
	for i := range vertexTokenCapacity {
		entries[[32]byte{byte(i)}] = &vertexToken{token: &oauth2.Token{AccessToken: "token", Expiry: now.Add(time.Duration(i+1) * time.Hour)}}
	}
	// The entry expiring first is minting its replacement.
	entries[[32]byte{0}].minting = &vertexMint{done: make(chan struct{})}
	vertexTokens.entries = entries
	vertexTokens.Unlock()
	t.Cleanup(func() {
		vertexTokens.Lock()
		vertexTokens.entries = saved
		vertexTokens.Unlock()
	})
	account := &serviceAccount{digest: [32]byte{0xff}}
	account.cached()
	vertexTokens.Lock()
	defer vertexTokens.Unlock()
	_, minting := vertexTokens.entries[[32]byte{0}]
	_, earliest := vertexTokens.entries[[32]byte{1}]
	if !minting || earliest || len(vertexTokens.entries) != vertexTokenCapacity {
		t.Errorf("eviction kept minting %t, earliest idle %t, %d entries", minting, earliest, len(vertexTokens.entries))
	}
}

// A gateway that echoes the access token in its error body does not carry it
// into the failure.
func TestVertexTokenRedaction(t *testing.T) {
	t.Parallel()
	fake := newTokenServer(t)
	fake.grant(3600)
	account, _ := serviceAccountJSON(t, fake.URL)
	service := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusBadRequest)
		fmt.Fprintf(w, `{"error":{"code":400,"status":"INVALID_ARGUMENT","message":"rejected header %s"}}`, r.Header.Get("Authorization"))
	}))
	t.Cleanup(service.Close)
	binding := vertexBinding(t, service.URL, account)
	language, err := New(t.Context(), binding)
	if err != nil {
		t.Fatal(err)
	}
	var failure *Failure
	Run(t.Context(), language, cloudCall(t, binding), Main, &policyClock{}, func(attempt Attempt) error {
		failure = attempt.Failure
		return nil
	}, func(fantasy.StreamPart) bool { return true }, nil)
	if failure == nil || strings.Contains(failure.Message, "token-1") || !strings.Contains(failure.Message, "[redacted]") {
		t.Fatalf("failure %+v", failure)
	}
}

// The HTTP/1.1 fallback replaces the wire transport under the Vertex AI
// authorization.
func TestVertexHTTP1Fallback(t *testing.T) {
	t.Parallel()
	account := &serviceAccount{email: "fallback"}
	model := &sdkModel{client: &http.Client{Transport: vertexTransport{base: responseTransport{base: modelTransport(), stall: stallTimeout}, account: account}}}
	fallback, release := model.http1Fallback()
	if fallback == nil {
		t.Fatal("a Vertex AI model has no HTTP/1.1 fallback")
	}
	defer release()
	vertex, ok := fallback.client.Transport.(vertexTransport)
	if !ok || vertex.account != account {
		t.Fatalf("fallback transport %T lost its authorization", fallback.client.Transport)
	}
	wire := vertex.base.(responseTransport).base.(*http.Transport)
	if wire == modelTransport() || wire.Protocols == nil || wire.Protocols.HTTP2() || !wire.Protocols.HTTP1() {
		t.Errorf("fallback wire protocols %v", wire.Protocols)
	}
	if model.client.Transport.(vertexTransport).base.(responseTransport).base != modelTransport() {
		t.Error("the fallback changed the shared transport")
	}
}

// A full Vertex AI model path is shaped by its model name.
func TestVertexModelPathParameters(t *testing.T) {
	t.Parallel()
	account, _ := serviceAccountJSON(t, "https://oauth2.googleapis.com/token")
	binding := vertexBinding(t, "https://aiplatform.googleapis.com", account)
	for _, row := range []struct {
		model, effort, path string
		value               any
	}{
		{"publishers/google/models/gemini-2.5-flash", "none", "generationConfig.thinkingConfig.thinkingBudget", 0},
		{"publishers/google/models/gemini-3-pro", "medium", "generationConfig.thinkingConfig.thinkingLevel", "low"},
	} {
		binding.Config.Model, binding.Config.Effort = row.model, protocol.Effort{Kind: row.effort}
		params := requestParameters(binding, &fantasy.Call{})
		raw, _ := json.Marshal(params.Body)
		var body map[string]any
		json.Unmarshal(raw, &body)
		if got := valueAt(body, row.path); fmt.Sprint(got) != fmt.Sprint(row.value) {
			t.Errorf("%s at %s: %s = %v; want %v", row.model, row.effort, row.path, got, row.value)
		}
	}
}

func TestCloudCatalogMapping(t *testing.T) {
	t.Parallel()
	for _, row := range []struct{ id, claude string }{
		{"anthropic.claude-opus-4-6-v1", "claude-opus-4-6"},
		{"us.anthropic.claude-sonnet-4-5-20250929-v1:0", "claude-sonnet-4-5-20250929"},
		{"global.anthropic.claude-haiku-5-5", "claude-haiku-5-5"},
		{"apac.anthropic.claude-sonnet-4-20250514-v1:0", "claude-sonnet-4-20250514"},
		{"arn:aws:bedrock:us-west-2:123456789012:inference-profile/eu.anthropic.claude-opus-4-8", "claude-opus-4-8"},
		{"amazon.nova-pro-v1:0", "amazon.nova-pro-v1:0"},
		{"claude-opus-4-8", "claude-opus-4-8"},
	} {
		if got := bedrockClaudeModel(row.id); got != row.claude {
			t.Errorf("bedrockClaudeModel(%q) = %q; want %q", row.id, got, row.claude)
		}
	}
	if entry, ok := catalogEntry("anthropic", "eu.anthropic.claude-opus-4-6-v1"); !ok || !entry.Reasoning {
		t.Errorf("a Bedrock profile has no Claude capabilities: %+v, %v", entry, ok)
	}
	for _, row := range []struct {
		api   protocol.ConversationModelApi
		model string
		limit uint32
	}{
		{protocol.ConversationModelApiBedrock, "us.anthropic.claude-sonnet-4-6", 1000000},
		{protocol.ConversationModelApiBedrock, "global.anthropic.claude-haiku-4-5-20251001-v1:0", 200000},
		{protocol.ConversationModelApiBedrock, "anthropic.claude-opus-4-5-20251101-v1:0", 200000},
		{protocol.ConversationModelApiVertexAi, "gemini-3.5-flash", 1048576},
		{protocol.ConversationModelApiVertexAi, "publishers/google/models/gemini-2.5-pro", 1048576},
	} {
		if got := catalogContext(row.api, row.model); got != row.limit {
			t.Errorf("catalogContext(%s, %q) = %d; want %d", row.api, row.model, got, row.limit)
		}
	}
}
