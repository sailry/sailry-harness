package plugins

import (
	"context"
	"encoding/json"
	"io"
	"net"
	"net/http"
	"net/url"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/net/http/httpguts"
	"golang.org/x/net/idna"
)

const HTTPMaxBody = 1024 * 1024
const HTTPMaxTimeout = 60 * time.Second
const httpMaxHeaders = 32768

type HTTPInput struct {
	URL        *url.URL
	Method     string
	Headers    http.Header
	Body       *string
	Timeout    time.Duration
	Protection []protocol.Secret
}

func PrepareHTTP(request protocol.PluginHttpRequest, schema *protocol.PluginSettingsSchema, secrets map[string]protocol.Secret) (HTTPInput, error) {
	address, err := pluginHTTPURL(request.Url)
	if err != nil {
		return HTTPInput{}, err
	}
	if request.TimeoutMs == 0 || time.Duration(request.TimeoutMs)*time.Millisecond > HTTPMaxTimeout ||
		request.Body != nil && len(*request.Body) > HTTPMaxBody {
		return HTTPInput{}, scriptInvalid("invalid plugin HTTP method, timeout or size")
	}
	switch request.Method {
	case "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS":
	default:
		return HTTPInput{}, scriptInvalid("invalid plugin HTTP method, timeout or size")
	}
	if len(request.Headers) > httpMaxHeaders {
		return HTTPInput{}, scriptInvalid("plugin HTTP headers exceed the HTTP implementation capacity")
	}
	input := HTTPInput{
		URL: address, Method: request.Method, Body: request.Body,
		Headers: make(http.Header), Timeout: time.Duration(request.TimeoutMs) * time.Millisecond,
	}
	for name, value := range request.Headers {
		canonical, err := pluginHTTPHeader(name)
		if err != nil {
			return HTTPInput{}, err
		}
		if !httpguts.ValidHeaderFieldValue(value) {
			return HTTPInput{}, scriptInvalid("invalid plugin HTTP header value")
		}
		if _, exists := input.Headers[canonical]; exists {
			return HTTPInput{}, scriptInvalid("duplicate plugin HTTP header")
		}
		input.Headers[canonical] = []string{value}
	}
	if request.Credential != nil {
		if schema == nil {
			return HTTPInput{}, scriptInvalid("HTTP credential is not declared")
		}
		field, found := schema.Properties[*request.Credential]
		if !found || field.Secret == nil {
			return HTTPInput{}, scriptInvalid("HTTP credential is not declared")
		}
		origin, name, prefix, err := HTTPBinding(*field.Secret)
		if err != nil {
			return HTTPInput{}, err
		}
		if pluginHTTPOrigin(address) != pluginHTTPOrigin(origin) {
			return HTTPInput{}, scriptPermission("HTTP credential belongs to another origin")
		}
		if _, exists := input.Headers[name]; exists {
			return HTTPInput{}, scriptInvalid("HTTP credential header cannot be overridden")
		}
		if len(input.Headers) >= httpMaxHeaders {
			return HTTPInput{}, scriptInvalid("plugin HTTP headers exceed the HTTP implementation capacity")
		}
		secret, found := secrets[*request.Credential]
		if !found {
			return HTTPInput{}, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "HTTP credential is not configured"}
		}
		if !httpguts.ValidHeaderFieldValue(prefix + secret.Expose()) {
			return HTTPInput{}, scriptInvalid("HTTP credential is not a valid header value")
		}
		input.Headers[name] = []string{prefix + secret.Expose()}
		input.Protection = append(input.Protection, secret)
	}
	if pluginHTTPMetadata(input.URL, input.Headers) > protocol.MAX_FRAME_BYTES {
		return HTTPInput{}, scriptInvalid("plugin HTTP metadata exceeds the transport budget")
	}
	return input, nil
}

func HTTPBinding(binding protocol.PluginSettingsBinding) (*url.URL, string, string, error) {
	if binding.Unsupported {
		return nil, "", "", scriptInvalid("unsupported protected setting binding")
	}
	if binding.Server != nil || binding.Env != nil {
		return nil, "", "", scriptInvalid("HTTP credentials cannot target an MCP slot")
	}
	if binding.Origin == nil {
		return nil, "", "", scriptInvalid("HTTP credential origin is required")
	}
	address, err := pluginHTTPURL(*binding.Origin)
	if err != nil {
		return nil, "", "", err
	}
	if address.Path != "" && address.Path != "/" || address.RawQuery != "" || address.ForceQuery {
		return nil, "", "", scriptInvalid("HTTP credential must declare an origin without a path or query")
	}
	if binding.Header == nil {
		return nil, "", "", scriptInvalid("HTTP credential header is required")
	}
	name, err := pluginHTTPHeader(*binding.Header)
	if err != nil {
		return nil, "", "", err
	}
	prefix := ""
	if binding.Prefix != nil {
		prefix = *binding.Prefix
	}
	if !httpguts.ValidHeaderFieldValue(prefix) {
		return nil, "", "", scriptInvalid("invalid HTTP credential prefix")
	}
	return address, name, prefix, nil
}

func BindConnectorHTTP(input HTTPInput, declaration protocol.PluginConnectorDeclaration, method protocol.PluginConnectorKind, secrets map[string]protocol.Secret, grant *protocol.Secret) (HTTPInput, error) {
	if input.URL == nil {
		return HTTPInput{}, scriptInvalid("invalid plugin HTTP URL")
	}
	origin := pluginHTTPOrigin(input.URL)
	declared := false
	for _, candidate := range declaration.Origins {
		if candidate == origin {
			declared = true
			break
		}
	}
	if !declared {
		return HTTPInput{}, scriptPermission("connector endpoint is outside its declared origins")
	}
	input.Headers = input.Headers.Clone()
	input.Protection = append([]protocol.Secret(nil), input.Protection...)
	var authentication *protocol.PluginConnectorAuth
	for index := range declaration.Auth {
		if declaration.Auth[index].Kind == string(method) {
			authentication = &declaration.Auth[index]
			break
		}
	}
	if authentication == nil {
		return HTTPInput{}, pluginConnectorCredentialUnavailable()
	}
	matched := false
	for _, injection := range declaration.Inject {
		if injection.Origin == nil || *injection.Origin != origin {
			continue
		}
		matched = true
		if injection.Header == nil || injection.Server != nil || injection.Env != nil {
			return HTTPInput{}, scriptInvalid("connector HTTP header is unavailable")
		}
		name, err := pluginHTTPHeader(*injection.Header)
		if err != nil {
			return HTTPInput{}, err
		}
		if _, exists := input.Headers[name]; exists {
			return HTTPInput{}, scriptInvalid("connector header cannot be overridden")
		}
		if len(input.Headers) >= httpMaxHeaders {
			return HTTPInput{}, scriptInvalid("plugin HTTP headers exceed the HTTP implementation capacity")
		}
		field := injection.Field
		if field == nil && authentication.Kind == "secret" {
			encoded, err := json.Marshal(authentication.Data)
			if err != nil {
				return HTTPInput{}, pluginConnectorCredentialUnavailable()
			}
			var selection protocol.PluginConnectorAuthSecretPayload
			if json.Unmarshal(encoded, &selection) != nil || len(selection.Fields) == 0 {
				return HTTPInput{}, pluginConnectorCredentialUnavailable()
			}
			field = &selection.Fields[0]
		}
		var token *protocol.Secret
		if field != nil {
			var definition *protocol.PluginConnectorField
			for index := range declaration.Fields {
				if declaration.Fields[index].Name == *field && declaration.Fields[index].Secret {
					definition = &declaration.Fields[index]
					break
				}
			}
			if definition == nil {
				return HTTPInput{}, pluginConnectorCredentialUnavailable()
			}
			if secret, found := secrets[*field]; found && secret.Expose() != "" {
				token = &secret
			} else if !definition.Optional {
				return HTTPInput{}, pluginConnectorCredentialUnavailable()
			}
		} else {
			token = grant
			if token == nil {
				return HTTPInput{}, pluginConnectorCredentialUnavailable()
			}
		}
		if token == nil {
			continue
		}
		value := injection.Prefix + token.Expose()
		if !httpguts.ValidHeaderFieldValue(value) {
			return HTTPInput{}, scriptInvalid("connector credential is not a valid header value")
		}
		input.Headers[name] = []string{value}
		input.Protection = append(input.Protection, *token)
	}
	if !matched {
		return HTTPInput{}, scriptPermission("connector does not authorize this HTTP origin")
	}
	for _, secret := range secrets {
		input.Protection = append(input.Protection, secret)
	}
	if pluginHTTPMetadata(input.URL, input.Headers) > protocol.MAX_FRAME_BYTES {
		return HTTPInput{}, scriptInvalid("plugin HTTP metadata exceeds the transport budget")
	}
	return input, nil
}

func ExecuteHTTP(ctx context.Context, input HTTPInput, client *http.Client) (protocol.PluginHttpResponse, error) {
	if input.URL == nil {
		return protocol.PluginHttpResponse{}, scriptInvalid("invalid plugin HTTP URL")
	}
	if pluginHTTPMetadata(input.URL, input.Headers) > protocol.MAX_FRAME_BYTES {
		return protocol.PluginHttpResponse{}, scriptInvalid("plugin HTTP metadata exceeds the transport budget")
	}
	if ctx.Err() != nil {
		return protocol.PluginHttpResponse{}, pluginHTTPUnknown("plugin HTTP request was interrupted")
	}
	if client == nil {
		return protocol.PluginHttpResponse{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "plugin HTTP client is unavailable"}
	}
	bounded := *client
	bounded.Timeout = input.Timeout
	bounded.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	var body io.Reader
	if input.Body != nil {
		body = strings.NewReader(*input.Body)
	}
	request, err := http.NewRequestWithContext(ctx, input.Method, input.URL.String(), body)
	if err != nil {
		return protocol.PluginHttpResponse{}, scriptInvalid("invalid plugin HTTP request")
	}
	request.Header = input.Headers.Clone()
	response, err := bounded.Do(request)
	if err != nil {
		if ctx.Err() != nil {
			return protocol.PluginHttpResponse{}, pluginHTTPUnknown("plugin HTTP request was interrupted")
		}
		return protocol.PluginHttpResponse{}, pluginHTTPUnknown("plugin HTTP result is unknown")
	}
	defer response.Body.Close()
	if response.ContentLength > HTTPMaxBody {
		return protocol.PluginHttpResponse{}, pluginHTTPUnknown("plugin HTTP response exceeds the size limit")
	}
	if pluginHTTPMetadata(input.URL, response.Header) > protocol.MAX_FRAME_BYTES {
		return protocol.PluginHttpResponse{}, pluginHTTPUnknown("plugin HTTP response metadata exceeds the size budget")
	}
	headers := make(map[string][]string, len(response.Header))
	for name, values := range response.Header {
		for _, value := range values {
			for _, character := range value {
				if character < 0x20 && character != '\t' || character > 0x7e {
					return protocol.PluginHttpResponse{}, pluginHTTPUnknown("plugin HTTP response has a non-text header")
				}
			}
		}
		headers[strings.ToLower(name)] = append(headers[strings.ToLower(name)], values...)
	}
	bytes, err := io.ReadAll(io.LimitReader(response.Body, HTTPMaxBody+1))
	if ctx.Err() != nil {
		return protocol.PluginHttpResponse{}, pluginHTTPUnknown("plugin HTTP request was interrupted")
	}
	if err != nil {
		return protocol.PluginHttpResponse{}, pluginHTTPUnknown("plugin HTTP response was interrupted")
	}
	if len(bytes) > HTTPMaxBody {
		return protocol.PluginHttpResponse{}, pluginHTTPUnknown("plugin HTTP response exceeds the size limit")
	}
	if !utf8.Valid(bytes) {
		return protocol.PluginHttpResponse{}, pluginHTTPUnknown("plugin HTTP response is not UTF-8")
	}
	value := protocol.PluginHttpResponse{Status: uint16(response.StatusCode), Headers: headers, Body: string(bytes)}
	if input.Protection != nil {
		if err := ProtectHTTPResponse(&value, input.Protection); err != nil {
			return protocol.PluginHttpResponse{}, err
		}
	}
	encoded, err := json.Marshal(value)
	if err != nil {
		return protocol.PluginHttpResponse{}, pluginHTTPUnknown("invalid plugin HTTP response")
	}
	if len(encoded) > protocol.MAX_FRAME_BYTES-64*1024 {
		return protocol.PluginHttpResponse{}, pluginHTTPUnknown("plugin HTTP response exceeds the transport budget")
	}
	return value, nil
}

func ProtectHTTPResponse(response *protocol.PluginHttpResponse, secrets []protocol.Secret) error {
	for name := range response.Headers {
		switch name {
		case "content-type", "retry-after", "x-request-id", "request-id":
		default:
			delete(response.Headers, name)
		}
	}
	for _, secret := range secrets {
		if secret.Expose() == "" {
			continue
		}
		if strings.Contains(response.Body, secret.Expose()) {
			return scriptPermission("provider response exposed a private credential")
		}
		for _, values := range response.Headers {
			for _, value := range values {
				if strings.Contains(value, secret.Expose()) {
					return scriptPermission("provider response exposed a private credential")
				}
			}
		}
	}
	return nil
}

func pluginHTTPURL(value string) (*url.URL, error) {
	address, err := url.Parse(value)
	if err != nil {
		return nil, scriptInvalid("invalid plugin HTTP URL")
	}
	address.Scheme = strings.ToLower(address.Scheme)
	if address.Scheme != "http" && address.Scheme != "https" || address.Hostname() == "" || address.User != nil ||
		strings.Contains(value, "#") || address.Opaque != "" {
		return nil, scriptInvalid("plugin HTTP URL must use HTTP(S) without credentials or a fragment")
	}
	host := address.Hostname()
	if net.ParseIP(host) == nil {
		host, err = idna.Lookup.ToASCII(host)
		if err != nil {
			return nil, scriptInvalid("invalid plugin HTTP URL")
		}
	}
	host = strings.ToLower(host)
	if port := address.Port(); port != "" {
		address.Host = net.JoinHostPort(host, port)
	} else if strings.Contains(host, ":") {
		address.Host = "[" + host + "]"
	} else {
		address.Host = host
	}
	return address, nil
}

func pluginHTTPOrigin(address *url.URL) string {
	host := address.Hostname()
	port := address.Port()
	if port == "80" && address.Scheme == "http" || port == "443" && address.Scheme == "https" {
		port = ""
	}
	if port != "" {
		host = net.JoinHostPort(host, port)
	} else if strings.Contains(host, ":") {
		host = "[" + host + "]"
	}
	return address.Scheme + "://" + host
}

func pluginHTTPHeader(name string) (string, error) {
	if !httpguts.ValidHeaderFieldName(name) {
		return "", scriptInvalid("invalid plugin HTTP header")
	}
	switch strings.ToLower(name) {
	case "host", "connection", "content-length", "transfer-encoding", "proxy-authorization", "upgrade":
		return "", scriptInvalid("plugin HTTP header is controlled by the transport")
	}
	return http.CanonicalHeaderKey(name), nil
}

func pluginHTTPMetadata(address *url.URL, headers http.Header) int {
	size := len(address.String())
	for name, values := range headers {
		for _, value := range values {
			size += len(name) + len(value)
		}
	}
	return size
}

func pluginHTTPUnknown(message string) *protocol.Fault {
	return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: message}
}

func pluginConnectorCredentialUnavailable() *protocol.Fault {
	return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "connector is unavailable or changed"}
}
