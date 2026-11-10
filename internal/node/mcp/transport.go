package mcp

import (
	"context"
	"io"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"sort"
	"strings"

	"github.com/egoist/mygo/netproxy"
	"github.com/modelcontextprotocol/go-sdk/auth"
	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Launch struct {
	Kind        string
	Command     string
	Args        []string
	Env         map[string]string
	Directory   string
	URL         string
	Headers     map[string]string
	OAuth       auth.OAuthHandler
	Client      *http.Client
	Environment []string
}

func (launch Launch) identity() any {
	environment := make(map[string]string, len(launch.Env))
	for name, value := range launch.Env {
		if name != "SAILRY_PLUGIN_SETTINGS_FILE" {
			environment[name] = value
		}
	}
	return []any{launch.Kind, launch.Command, launch.Args, environment, launch.Directory, launch.URL, launch.Headers, launch.Environment}
}

func Open(ctx context.Context, launch Launch, elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error) {
	client := sdk.NewClient(&sdk.Implementation{Name: "Sailry", Version: "1"}, &sdk.ClientOptions{
		Capabilities: &sdk.ClientCapabilities{Elicitation: &sdk.ElicitationCapabilities{
			Form: &sdk.FormElicitationCapabilities{}, URL: &sdk.URLElicitationCapabilities{},
		}}, ElicitationHandler: elicit,
	})
	var transport sdk.Transport
	switch launch.Kind {
	case "stdio":
		command := exec.Command(launch.Command, launch.Args...)
		command.Dir = launch.Directory
		command.Stderr = io.Discard
		command.Env = launch.Environment
		if command.Env == nil {
			command.Env = os.Environ()
		}
		names := make([]string, 0, len(launch.Env))
		for name := range launch.Env {
			names = append(names, name)
		}
		sort.Strings(names)
		for _, name := range names {
			command.Env = append(command.Env, name+"="+launch.Env[name])
		}
		transport = commandTransport{command: command}
	case "streamable-http", "sse":
		httpClient := launch.Client
		if httpClient == nil {
			httpClient = &http.Client{Transport: netproxy.Transport()}
		}
		configured := *httpClient
		base := configured.Transport
		if base == nil {
			base = http.DefaultTransport
		}
		origin, err := url.Parse(launch.URL)
		if err != nil || origin.Host == "" || origin.User != nil || origin.Scheme != "http" && origin.Scheme != "https" {
			return nil, invalid("MCP URL is invalid")
		}
		configured.Transport = headerTransport{base: base, headers: launch.Headers, origin: origin}
		configured.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
		if launch.Kind == "sse" {
			transport = &sdk.SSEClientTransport{Endpoint: launch.URL, HTTPClient: &configured, MaxEventSize: maxMessage}
		} else {
			transport = &sdk.StreamableClientTransport{
				Endpoint: launch.URL, HTTPClient: &configured, OAuthHandler: launch.OAuth, MaxRetries: -1, MaxEventSize: maxMessage,
			}
		}
	default:
		return nil, invalid("unsupported MCP transport")
	}
	session, err := client.Connect(ctx, transport, nil)
	if err != nil {
		return nil, unavailable()
	}
	return session, nil
}

type headerTransport struct {
	base    http.RoundTripper
	headers map[string]string
	origin  *url.URL
}

func (transport headerTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	copy := request.Clone(request.Context())
	copy.Header = request.Header.Clone()
	if transport.origin == nil || !strings.EqualFold(copy.URL.Scheme, transport.origin.Scheme) || !strings.EqualFold(copy.URL.Host, transport.origin.Host) {
		return transport.base.RoundTrip(copy)
	}
	for name, value := range transport.headers {
		switch strings.ToLower(name) {
		case "host", "content-length", "transfer-encoding", "content-type", "accept", "mcp-session-id", "mcp-protocol-version", "last-event-id":
			continue
		}
		if copy.Header.Get(name) == "" {
			copy.Header.Set(name, value)
		}
	}
	return transport.base.RoundTrip(copy)
}

func ServerOptions(server protocol.PluginMcpServer) protocol.PluginMcpOptions {
	switch value := server.Data.(type) {
	case protocol.PluginMcpServerStdioPayload:
		return protocol.PluginMcpOptions{Exclusive: value.Exclusive, KeepImages: value.KeepImages, Reuse: value.Reuse}
	case *protocol.PluginMcpServerStdioPayload:
		return protocol.PluginMcpOptions{Exclusive: value.Exclusive, KeepImages: value.KeepImages, Reuse: value.Reuse}
	case protocol.PluginMcpServerHttpPayload:
		return protocol.PluginMcpOptions{Exclusive: value.Exclusive, KeepImages: value.KeepImages, Reuse: value.Reuse}
	case *protocol.PluginMcpServerHttpPayload:
		return protocol.PluginMcpOptions{Exclusive: value.Exclusive, KeepImages: value.KeepImages, Reuse: value.Reuse}
	case protocol.PluginMcpServerSsePayload:
		return protocol.PluginMcpOptions{Exclusive: value.Exclusive, KeepImages: value.KeepImages, Reuse: value.Reuse}
	case *protocol.PluginMcpServerSsePayload:
		return protocol.PluginMcpOptions{Exclusive: value.Exclusive, KeepImages: value.KeepImages, Reuse: value.Reuse}
	default:
		return protocol.DefaultPluginMcpOptions()
	}
}
