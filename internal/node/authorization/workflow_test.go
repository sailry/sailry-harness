package authorization

import (
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"io"
	"net/http"
	"net/url"
	"strings"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/oauth2"
)

type fixtureTransport func(*http.Request) (*http.Response, error)

func (transport fixtureTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	return transport(request)
}

func fixtureResponse(value any) *http.Response {
	body, _ := json.Marshal(value)
	return &http.Response{StatusCode: 200, Header: make(http.Header), Body: io.NopCloser(strings.NewReader(string(body)))}
}

func TestConnectorOAuthPKCE(t *testing.T) {
	clock := clockwork.NewFakeClockAt(time.Unix(1000, 0))
	callback := make(chan string, 1)
	var challenge string
	calls := 0
	mapping := protocol.PluginModelAuthMapping{Access: "/access_token", Extra: map[string]string{}}
	result, err := Run(t.Context(), Input{
		Clock: clock, Callback: callback, Redirect: "http://127.0.0.1:12345/auth/callback",
		Fields: map[string]string{"client_id": "captured-client"},
		Declaration: protocol.PluginModelAuthDeclaration{
			Scopes: []string{"profile"}, Token: &mapping,
			Flow: protocol.PluginModelAuthFlow{Kind: "pkce", Data: protocol.PluginModelAuthFlowPkcePayload{
				Authorize: "https://login.example/authorize", Redirect: protocol.PluginModelAuthRedirectLoopback,
				Token: protocol.PluginModelAuthEndpoint{Url: "https://login.example/token", Method: protocol.PluginModelAuthMethodPost},
			}},
		},
		Client: &http.Client{Transport: fixtureTransport(func(request *http.Request) (*http.Response, error) {
			calls++
			if request.URL.String() != "https://login.example/token" || request.ParseForm() != nil {
				t.Fatal("the declaration did not retain its token endpoint")
			}
			if request.Form.Get("code") != "private-code" || request.Form.Get("client_id") != "captured-client" || request.Form.Get("grant_type") != "authorization_code" || oauth2.S256ChallengeFromVerifier(request.Form.Get("code_verifier")) != challenge {
				t.Fatal("the captured proof was not used for token exchange")
			}
			return fixtureResponse(map[string]any{"access_token": "private-access"}), nil
		})},
		Progress: func(state protocol.ConversationLoginState) error {
			prompt := state.Data.(protocol.ConversationLoginStateBrowserPayload)
			link, err := url.Parse(prompt.AuthorizationUrl)
			if err != nil || link.Query().Get("code_challenge_method") != "S256" || link.Query().Get("scope") != "profile" {
				t.Fatal("PKCE authorization prompt did not preserve its declared inputs")
			}
			challenge = link.Query().Get("code_challenge")
			callback <- "http://127.0.0.1:12345/auth/callback?code=private-code&state=" + url.QueryEscape(link.Query().Get("state"))
			return nil
		},
	})
	if err != nil || result.Issued == nil || result.Issued.Access != "private-access" || calls != 1 {
		t.Fatalf("authorization result=%v calls=%d error=%v", result, calls, err)
	}
	for _, entry := range []struct {
		value   string
		manual  bool
		allowed bool
	}{
		{"https://example.test/callback?code=private-code&state=proof", false, true},
		{"https://example.test/callback?code=private-code&state=other", false, false},
		{"https://example.test/callback?code=private-code&state=proof&state=proof", false, false},
		{"https://example.test/other?code=private-code&state=proof", false, false},
		{"private-code", true, true}, {"private-code#proof", true, true},
		{"code=private-code&state=proof", true, true}, {"private-code#other", true, false},
		{"private-code", false, false},
	} {
		_, err := CallbackFields(entry.value, "https://example.test/callback", "proof", entry.manual)
		if (err == nil) != entry.allowed {
			t.Fatalf("callback=%q manual=%v error=%v", entry.value, entry.manual, err)
		}
	}
	t.Run("token answers separate a refusal from an open outcome", func(t *testing.T) {
		for _, entry := range []struct {
			status   int
			document any
			want     protocol.ErrorCode
		}{
			{200, map[string]any{"access_token": "issued"}, ""},
			{401, nil, protocol.ErrorCodePermissionDenied},
			{400, map[string]any{"error": "invalid_grant"}, protocol.ErrorCodePermissionDenied},
			{200, map[string]any{"error": "access_denied"}, protocol.ErrorCodePermissionDenied},
			{400, map[string]any{"error": "temporarily_unavailable"}, protocol.ErrorCodeUnavailable},
			{400, map[string]any{"error": "unrecognised"}, protocol.ErrorCodeUnavailable},
			{403, map[string]any{}, protocol.ErrorCodeUnavailable},
			{502, nil, protocol.ErrorCodeUnavailable},
		} {
			var code protocol.ErrorCode
			if err := Granted(entry.status, entry.document); err != nil {
				code = err.(*protocol.Fault).Code
			}
			if code != entry.want {
				t.Errorf("status=%d document=%v code=%q, want %q", entry.status, entry.document, code, entry.want)
			}
		}
	})
}

func TestAuthorizationIdentity(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Unix(1000, 0)
	nonce := "captured-nonce"
	for _, entry := range []struct {
		name   string
		alter  func(jwt.MapClaims)
		issuer string
		valid  bool
	}{
		{"bound", func(jwt.MapClaims) {}, "https://issuer.example", true},
		{"subject", func(claims jwt.MapClaims) { claims["sub"] = "" }, "https://issuer.example", false},
		{"nonce", func(claims jwt.MapClaims) { claims["nonce"] = "other" }, "https://issuer.example", false},
		{"multi_audience", func(claims jwt.MapClaims) { claims["aud"] = []string{"client", "other"} }, "https://issuer.example", false},
		{"authorized_party", func(claims jwt.MapClaims) { claims["aud"], claims["azp"] = []string{"client", "other"}, "client" }, "https://issuer.example", true},
		{"expired", func(claims jwt.MapClaims) { claims["exp"] = 990 }, "https://issuer.example", false},
		{"discovery_issuer", func(jwt.MapClaims) {}, "https://other.example", false},
	} {
		t.Run(entry.name, func(t *testing.T) {
			claims := jwt.MapClaims{"sub": "subject", "iss": "https://issuer.example", "aud": "client", "iat": 999, "exp": 1100, "nonce": nonce}
			entry.alter(claims)
			token := jwt.NewWithClaims(jwt.SigningMethodEdDSA, claims)
			token.Header["kid"] = "key"
			encoded, err := token.SignedString(private)
			if err != nil {
				t.Fatal(err)
			}
			client := &http.Client{Transport: fixtureTransport(func(request *http.Request) (*http.Response, error) {
				switch request.URL.Path {
				case "/.well-known/openid-configuration":
					return fixtureResponse(map[string]any{"issuer": entry.issuer, "jwks_uri": "https://issuer.example/keys"}), nil
				case "/keys":
					return fixtureResponse(map[string]any{"keys": []any{map[string]any{"kty": "OKP", "kid": "key", "crv": "Ed25519", "alg": "EdDSA", "use": "sig", "key_ops": []string{"verify"}, "x": base64.RawURLEncoding.EncodeToString(public)}}}), nil
				default:
					t.Fatal("identity lookup left its captured endpoints")
					return nil, context.Canceled
				}
			})}
			_, err = VerifyIdentity(t.Context(), client, "https://issuer.example", encoded, "client", &nonce, func() time.Time { return now })
			if (err == nil) != entry.valid {
				t.Fatalf("identity validation=%v expected valid=%v", err, entry.valid)
			}
		})
	}
}
