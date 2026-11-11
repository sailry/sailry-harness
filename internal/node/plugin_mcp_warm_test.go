package node

import (
	"context"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Over 1 s: the Node starts twice, and each start installs and verifies the
// bundled packages.
func TestMCPWarm(t *testing.T) {
	const name = "mcp-warm"
	var opened atomic.Int32
	options := &mcp.Options{Warm: true, Open: func(ctx context.Context, launch mcp.Launch, elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error) {
		if launch.URL != "https://fixture.test/mcp" {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture MCP service is unavailable"}
		}
		opened.Add(1)
		server := sdk.NewServer(&sdk.Implementation{Name: "warm-fixture", Version: "1"}, nil)
		sdk.AddTool(server, &sdk.Tool{Name: "echo"}, func(context.Context, *sdk.CallToolRequest, struct{}) (*sdk.CallToolResult, any, error) {
			return &sdk.CallToolResult{}, nil, nil
		})
		clientWire, serverWire := sdk.NewInMemoryTransports()
		serverSession, err := server.Connect(ctx, serverWire, nil)
		if err != nil {
			return nil, err
		}
		t.Cleanup(func() { _ = serverSession.Close() })
		return sdk.NewClient(&sdk.Implementation{Name: "warm-client", Version: "1"}, &sdk.ClientOptions{ElicitationHandler: elicit}).Connect(ctx, clientWire, nil)
	}}
	profile := filepath.Join(t.TempDir(), "profile")
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	var node *Node
	start := func(t *testing.T) link.Subscription {
		t.Helper()
		var err error
		if node, err = StartWithOptions(t.Context(), profile, Options{MCP: options, Environment: &environment}); err != nil {
			t.Fatal(err)
		}
		subscription, err := node.Subscribe(t.Context(), node.ID(), protocol.Topic{Kind: "node"})
		if err != nil {
			t.Fatal(err)
		}
		return subscription
	}
	read := func(t *testing.T) protocol.PluginInfo {
		t.Helper()
		return commandFixture(t, node, protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: name}}).Data.(protocol.PluginInfo)
	}
	// connected waits until the server reports its tool and the package was
	// published at least the given number of times.
	connected := func(t *testing.T, subscription link.Subscription, published int) {
		t.Helper()
		waiting, cancel := context.WithTimeout(t.Context(), 20*time.Second)
		defer cancel()
		events := 0
		for {
			runtime := read(t).Mcp[0].Runtime
			if events >= published && runtime != nil && runtime.State == protocol.PluginMcpRuntimeStateConnected && runtime.Tools == 1 {
				return
			}
			update, err := subscription.Next(waiting)
			if err != nil {
				t.Fatalf("server did not report its connection: runtime=%+v published=%d: %v", runtime, events, err)
			}
			if envelope, ok := update.Data.(protocol.EventEnvelope); ok && envelope.Event.Kind == "plugin_changed" {
				if summary, ok := dispatchPayload[protocol.PluginSummary](envelope.Event.Data); ok && summary.Name == name {
					events++
				}
			}
		}
	}
	subscription := start(t)
	defer func() { _ = node.Stop(context.Background()) }()
	t.Run("a server installed on a running Node connects and publishes its package", func(t *testing.T) {
		commandFixture(t, node, protocol.Command{Kind: "install_mcp", Data: protocol.CommandInstallMcpPayload{Name: name,
			Definition: protocol.PluginMcpDefinition{Kind: "streamable-http", Data: &protocol.PluginMcpDefinitionStreamableHttpPayload{Url: "https://fixture.test/mcp", Headers: []string{}}}}})
		// One event for the installation, one at least for the runtime.
		connected(t, subscription, 2)
		if opened.Load() != 1 {
			t.Fatalf("server opened %d times", opened.Load())
		}
	})
	t.Run("a disabled package stops its server and an enabled one starts it", func(t *testing.T) {
		info := read(t)
		commandFixture(t, node, protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{Name: name, ExpectedRevision: info.Summary.Revision, Enabled: false}})
		info = read(t)
		if info.Mcp[0].Runtime != nil {
			t.Fatalf("disabled package reports %+v", info.Mcp[0].Runtime)
		}
		commandFixture(t, node, protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{Name: name, ExpectedRevision: info.Summary.Revision, Enabled: true}})
		connected(t, subscription, 0)
		if opened.Load() != 2 {
			t.Fatalf("server opened %d times", opened.Load())
		}
	})
	t.Run("a restarted Node connects before any command", func(t *testing.T) {
		_ = subscription.Close()
		if err := node.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
		subscription = start(t)
		defer subscription.Close()
		connected(t, subscription, 0)
		if opened.Load() != 3 {
			t.Fatalf("server opened %d times", opened.Load())
		}
	})
}
