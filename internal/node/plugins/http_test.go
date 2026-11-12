package plugins

import (
	"context"
	"io"
	"net/http"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type scriptHTTPTransport func(*http.Request) (*http.Response, error)

func (transport scriptHTTPTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	return transport(request)
}

func TestPluginHTTPAndAliases(t *testing.T) {
	t.Parallel()
	t.Run("protected settings and transport headers", func(t *testing.T) {
		field, origin, header, prefix := "token", "https://service.example", "Authorization", "Bearer "
		schema := &protocol.PluginSettingsSchema{Properties: map[string]protocol.PluginSettingsField{
			field: {Secret: &protocol.PluginSettingsBinding{Origin: &origin, Header: &header, Prefix: &prefix}},
		}}
		request := protocol.PluginHttpRequest{Method: "GET", Url: origin + "/resource", Headers: map[string]string{"Accept": "application/json"}, TimeoutMs: 1000, Credential: &field}
		secrets := map[string]protocol.Secret{field: protocol.NewSecret("private-token")}
		input, err := PrepareHTTP(request, schema, secrets)
		if err != nil || input.Headers.Get(header) != "Bearer private-token" {
			t.Fatalf("protected header was not injected: %v", err)
		}
		for _, name := range []string{"Host", "Connection", "Content-Length", "Transfer-Encoding", "Proxy-Authorization", "Upgrade", "Bad Header"} {
			changed := request
			changed.Headers = map[string]string{name: "value"}
			if _, err := PrepareHTTP(changed, schema, secrets); err == nil {
				t.Fatalf("accepted transport header %s", name)
			}
		}
		changed := request
		changed.Headers = map[string]string{"X-Test": "first", "x-test": "second"}
		if _, err := PrepareHTTP(changed, schema, secrets); err == nil {
			t.Fatal("accepted duplicate header")
		}
		changed.Headers = map[string]string{"Authorization": "override"}
		if _, err := PrepareHTTP(changed, schema, secrets); err == nil {
			t.Fatal("accepted credential override")
		}
		changed = request
		changed.Url = "https://other.example/resource"
		_, err = PrepareHTTP(changed, schema, secrets)
		scriptTestFault(t, err, protocol.ErrorCodePermissionDenied)
		for _, address := range []string{"https://user:secret@service.example/", "https://service.example/#", "file:///tmp/input"} {
			changed.Url = address
			if _, err := PrepareHTTP(changed, schema, secrets); err == nil {
				t.Fatalf("accepted protected URL %s", address)
			}
		}
		prefix = strings.Repeat("x", 129)
		if _, _, actual, err := HTTPBinding(*schema.Properties[field].Secret); err != nil || actual != prefix {
			t.Fatalf("long prefix was not retained: %v", err)
		}
		schema.Properties[field].Secret.Unsupported = true
		if _, err := PrepareHTTP(request, schema, secrets); err == nil {
			t.Fatal("accepted an unsupported binding")
		}
	})
	t.Run("connector selects declared secret fields", func(t *testing.T) {
		origin, header, field := "https://service.example", "Authorization", "beta"
		declaration := protocol.PluginConnectorDeclaration{
			Origins: []string{origin},
			Auth:    []protocol.PluginConnectorAuth{{Kind: "secret", Data: protocol.PluginConnectorAuthSecretPayload{Fields: []string{"alpha", "beta"}}}},
			Fields:  []protocol.PluginConnectorField{{Name: "alpha", Secret: true}, {Name: "beta", Secret: true}},
			Inject:  []protocol.PluginConnectorInjection{{Origin: &origin, Header: &header, Field: &field, Prefix: "Bearer "}},
		}
		input, err := PrepareHTTP(protocol.PluginHttpRequest{Method: "GET", Url: origin + "/resource", TimeoutMs: 1000}, nil, nil)
		if err != nil {
			t.Fatal(err)
		}
		secrets := map[string]protocol.Secret{"alpha": protocol.NewSecret("alpha-token"), "beta": protocol.NewSecret("beta-token")}
		bound, err := BindConnectorHTTP(input, declaration, protocol.PluginConnectorKindSecret, secrets, nil)
		if err != nil || bound.Headers.Get(header) != "Bearer beta-token" {
			t.Fatalf("wrong connector field: %v", err)
		}
		delete(secrets, field)
		_, err = BindConnectorHTTP(input, declaration, protocol.PluginConnectorKindSecret, secrets, nil)
		scriptTestFault(t, err, protocol.ErrorCodeNotConfigured)
		declaration.Inject[0].Field = nil
		bound, err = BindConnectorHTTP(input, declaration, protocol.PluginConnectorKindSecret, secrets, nil)
		if err != nil || bound.Headers.Get(header) != "Bearer alpha-token" {
			t.Fatalf("default connector field changed: %v", err)
		}
	})
	t.Run("bounded response protection redirects and uncertain effects", func(t *testing.T) {
		input, err := PrepareHTTP(protocol.PluginHttpRequest{Method: "GET", Url: "https://service.example/resource", TimeoutMs: 1000}, nil, nil)
		if err != nil {
			t.Fatal(err)
		}
		input.Protection = []protocol.Secret{protocol.NewSecret("private-token")}
		calls := 0
		client := &http.Client{Transport: scriptHTTPTransport(func(request *http.Request) (*http.Response, error) {
			calls++
			return &http.Response{StatusCode: 302, Header: http.Header{
				"Location": {"https://other.example/"}, "Content-Type": {"application/json"}, "X-Private": {"private-token"},
			}, Body: io.NopCloser(strings.NewReader(`{"ok":true}`)), Request: request}, nil
		})}
		response, err := ExecuteHTTP(context.Background(), input, client)
		if err != nil || response.Status != 302 || calls != 1 || len(response.Headers) != 1 {
			t.Fatalf("redirect/protection = %v; calls = %d; error = %v", response, calls, err)
		}
		client.Transport = scriptHTTPTransport(func(request *http.Request) (*http.Response, error) {
			return &http.Response{StatusCode: 200, Header: http.Header{"Content-Type": {"text/plain"}}, Body: io.NopCloser(strings.NewReader("private-token")), Request: request}, nil
		})
		_, err = ExecuteHTTP(context.Background(), input, client)
		scriptTestFault(t, err, protocol.ErrorCodePermissionDenied)
		client.Transport = scriptHTTPTransport(func(request *http.Request) (*http.Response, error) {
			return &http.Response{StatusCode: 200, Header: make(http.Header), Body: io.NopCloser(strings.NewReader(strings.Repeat("x", HTTPMaxBody+1))), Request: request}, nil
		})
		_, err = ExecuteHTTP(context.Background(), input, client)
		scriptTestFault(t, err, protocol.ErrorCodeOutcomeUnknown)
		ctx, cancel := context.WithCancel(context.Background())
		cancel()
		_, err = ExecuteHTTP(ctx, input, client)
		scriptTestFault(t, err, protocol.ErrorCodeOutcomeUnknown)
	})
	t.Run("stable alias namespace", func(t *testing.T) {
		alias := ToolAlias("fixture", "lookup")
		if ToolAlias("fixture", "lookup") != alias || ToolAlias("other", "lookup") == alias {
			t.Fatal("alias identity is not stable")
		}
	})
}
