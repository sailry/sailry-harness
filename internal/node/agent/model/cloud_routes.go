package model

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"encoding/pem"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"

	"github.com/anthropics/anthropic-sdk-go/bedrock"
	"github.com/anthropics/anthropic-sdk-go/option"
	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/egoist/mygo/netproxy"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/oauth2"
	"golang.org/x/oauth2/jwt"
)

// cloudBinding configures an Amazon Bedrock or Vertex AI route. Bedrock
// carries Claude Messages authenticated by a Bedrock API key; Vertex AI
// carries Gemini authenticated by access tokens the Node mints from the
// provider's service-account key. The endpoint is the cloud's service host,
// the canonical one for the region or location unless the provider names a
// private one.
func cloudBinding(binding Binding, config transportConfig, u *url.URL) (transportConfig, error) {
	provider := binding.Provider
	if err := providers.ValidateCloud(provider); err != nil {
		return config, &Failure{Kind: NotConfigured, Message: "cloud connection options are invalid"}
	}
	if strings.Trim(u.Path, "/") != "" {
		return config, &Failure{Kind: InvalidRequest, Message: "cloud endpoint must be a service host without a path"}
	}
	config.Cloud = provider.Api
	config.Endpoint = strings.TrimRight(u.String(), "/")
	// The credential travels only as the route's bearer token.
	config.RemoveHeaders = []string{"X-Api-Key", "X-Goog-Api-Key", "Api-Key"}
	switch provider.Api {
	case protocol.ConversationModelApiBedrock:
		if config.Key == "" {
			return config, &Failure{Kind: NotConfigured, Message: "Bedrock API key is required"}
		}
		options, _ := provider.Options.Bedrock()
		config.Kind, config.Region = "anthropic", options.Region
	case protocol.ConversationModelApiVertexAi:
		account, err := parseServiceAccount(config.Key)
		if err != nil {
			return config, err
		}
		options, _ := provider.Options.VertexAi()
		config.Kind, config.Project, config.Location, config.vertex = "google", options.Project, options.Location, account
		config.Protection = append(config.Protection, protocol.NewSecret(string(account.key)))
	}
	for name, value := range provider.Headers {
		if !customHeaderAllowed(name, "authorization") {
			continue
		}
		for existing := range config.Headers {
			if strings.EqualFold(existing, name) {
				delete(config.Headers, existing)
			}
		}
		config.Headers[name] = value
	}
	return config, nil
}

// bedrockOptions route an Anthropic client's Messages requests through the
// SDK's Bedrock adaptation: the model and stream move into the path, betas
// into the body, and the EventStream response becomes SSE.
func bedrockOptions(config transportConfig, endpoint string) []option.RequestOption {
	key := config.Key
	return []option.RequestOption{
		bedrock.WithConfig(aws.Config{Region: config.Region, BearerAuthTokenProvider: bedrock.NewStaticBearerTokenProvider(key)}),
		// WithConfig addresses the region's public host; a private endpoint
		// replaces it.
		option.WithBaseURL(endpoint),
		// WithConfig prefers AWS_BEARER_TOKEN_BEDROCK from the process
		// environment over the configured key. Registered after it, this
		// middleware runs nearer the wire and restores the captured
		// provider's own key, so no ambient credential leaves the Node.
		option.WithMiddleware(func(request *http.Request, next option.MiddlewareNext) (*http.Response, error) {
			request.Header.Set("Authorization", "Bearer "+key)
			request.Header.Del("X-Api-Key")
			// The adaptation's GetBody rewinds and returns its one reader, so
			// the transport's request capture would drain the body it sends.
			// Each reader here is independent.
			if request.Body != nil && request.Body != http.NoBody {
				raw, err := io.ReadAll(request.Body)
				request.Body.Close()
				if err != nil {
					return nil, err
				}
				request.Body = io.NopCloser(bytes.NewReader(raw))
				request.GetBody = func() (io.ReadCloser, error) { return io.NopCloser(bytes.NewReader(raw)), nil }
				request.ContentLength = int64(len(raw))
			}
			return next(request)
		}),
	}
}

// bedrockBeta adds the betas a Bedrock request needs to the header value:
// manual thinking between tool calls is a beta on Bedrock as on the
// first-party API. Bedrock carries the list in the body.
func bedrockBeta(beta string, raw []byte) string {
	var payload struct {
		Thinking struct {
			Type string `json:"type"`
		} `json:"thinking"`
		Tools []json.RawMessage `json:"tools"`
	}
	if json.Unmarshal(raw, &payload) != nil || payload.Thinking.Type != "enabled" || len(payload.Tools) == 0 {
		return beta
	}
	const interleaved = "interleaved-thinking-2025-05-14"
	for _, token := range strings.Split(beta, ",") {
		if strings.TrimSpace(token) == interleaved {
			return beta
		}
	}
	if beta == "" {
		return interleaved
	}
	return beta + "," + interleaved
}

// vertexScope is the OAuth scope of Vertex AI's generative endpoints.
const vertexScope = "https://www.googleapis.com/auth/cloud-platform"

// vertexTokenMargin refreshes a cached access token before it expires, so a
// long request does not start with a token about to lapse.
const vertexTokenMargin = time.Minute

// serviceAccount is a parsed Google service-account key.
type serviceAccount struct {
	email, keyID, tokenURL string
	key                    []byte
	digest                 [32]byte
}

func parseServiceAccount(raw string) (*serviceAccount, error) {
	invalid := &Failure{Kind: NotConfigured, Message: "Vertex AI service account key is invalid"}
	if strings.TrimSpace(raw) == "" {
		return nil, &Failure{Kind: NotConfigured, Message: "Vertex AI service account key is required"}
	}
	var document struct {
		Type         string `json:"type"`
		ClientEmail  string `json:"client_email"`
		PrivateKey   string `json:"private_key"`
		PrivateKeyID string `json:"private_key_id"`
		TokenURI     string `json:"token_uri"`
	}
	if json.Unmarshal([]byte(raw), &document) != nil || document.Type != "service_account" || document.ClientEmail == "" {
		return nil, invalid
	}
	if block, _ := pem.Decode([]byte(document.PrivateKey)); block == nil {
		return nil, invalid
	}
	tokenURL := document.TokenURI
	if tokenURL == "" {
		tokenURL = "https://oauth2.googleapis.com/token"
	}
	// The signed assertion is a credential: it goes to an HTTPS token
	// endpoint, or over plain HTTP only to this machine.
	target, err := url.Parse(tokenURL)
	if err != nil || target.Host == "" || target.User != nil ||
		target.Scheme != "https" && !(target.Scheme == "http" && loopbackHost(target.Hostname())) {
		return nil, invalid
	}
	account := &serviceAccount{email: document.ClientEmail, keyID: document.PrivateKeyID, tokenURL: tokenURL, key: []byte(document.PrivateKey)}
	account.digest = sha256.Sum256([]byte(raw))
	return account, nil
}

func loopbackHost(host string) bool {
	if host == "localhost" {
		return true
	}
	ip := net.ParseIP(host)
	return ip != nil && ip.IsLoopback()
}

// vertexToken is one service account's cached access token. Its lock makes
// concurrent requests wait for one token request instead of each minting.
type vertexToken struct {
	sync.Mutex
	token *oauth2.Token
}

// vertexTokens are the cached tokens by service-account key digest, bounded
// by vertexTokenCapacity.
var vertexTokens = struct {
	sync.Mutex
	entries map[[32]byte]*vertexToken
}{entries: map[[32]byte]*vertexToken{}}

const vertexTokenCapacity = 32

func (account *serviceAccount) cached() *vertexToken {
	vertexTokens.Lock()
	defer vertexTokens.Unlock()
	entry := vertexTokens.entries[account.digest]
	if entry == nil {
		if len(vertexTokens.entries) >= vertexTokenCapacity {
			for digest := range vertexTokens.entries {
				delete(vertexTokens.entries, digest)
				break
			}
		}
		entry = &vertexToken{}
		vertexTokens.entries[account.digest] = entry
	}
	return entry
}

// accessToken is the account's cached access token, minted again by the
// JWT bearer grant when it is absent or about to expire.
func (account *serviceAccount) accessToken(ctx context.Context) (string, error) {
	entry := account.cached()
	entry.Lock()
	defer entry.Unlock()
	if entry.token != nil && entry.token.AccessToken != "" && time.Until(entry.token.Expiry) > vertexTokenMargin {
		return entry.token.AccessToken, nil
	}
	entry.token = nil
	client := &http.Client{Transport: netproxy.Transport(), Timeout: 30 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	grant := &jwt.Config{Email: account.email, PrivateKey: account.key, PrivateKeyID: account.keyID, Scopes: []string{vertexScope}, TokenURL: account.tokenURL}
	token, err := grant.TokenSource(context.WithValue(ctx, oauth2.HTTPClient, client)).Token()
	if err != nil {
		return "", tokenFailure(ctx, err)
	}
	if token.AccessToken == "" {
		return "", &Failure{Kind: InvalidResponse, Message: "Vertex AI token response has no access token"}
	}
	entry.token = token
	return token.AccessToken, nil
}

// tokenFailure classifies a failed token request by its status alone. The
// token endpoint's body may describe the key, so it is not repeated.
func tokenFailure(ctx context.Context, err error) *Failure {
	if ctx.Err() != nil {
		return &Failure{Kind: Cancelled, Message: "model request cancelled"}
	}
	var retrieve *oauth2.RetrieveError
	if !errors.As(err, &retrieve) || retrieve.Response == nil {
		return &Failure{Kind: Network, Message: "Vertex AI token request failed"}
	}
	status := retrieve.Response.StatusCode
	failure := &Failure{Kind: InvalidResponse, Message: fmt.Sprintf("Vertex AI token request failed (HTTP %d)", status)}
	switch {
	case status == 400 || status == 401 || status == 403:
		// invalid_grant: a revoked, disabled or deleted key.
		failure.Kind = Authentication
	case status == 429:
		failure.Kind = RateLimited
	case status >= 500:
		failure.Kind = Unavailable
	}
	value := uint16(status)
	failure.status = &value
	return failure
}

// vertexTransport authorizes each Vertex AI request with the account's
// access token.
type vertexTransport struct {
	base    http.RoundTripper
	account *serviceAccount
}

func (transport vertexTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	token, err := transport.account.accessToken(request.Context())
	if err != nil {
		if request.Body != nil {
			request.Body.Close()
		}
		return nil, err
	}
	request = request.Clone(request.Context())
	request.Header.Del("X-Goog-Api-Key")
	request.Header.Set("Authorization", "Bearer "+token)
	return transport.base.RoundTrip(request)
}
