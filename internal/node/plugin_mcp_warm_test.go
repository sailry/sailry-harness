package node

import (
	"context"
	"log/slog"
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
	t.Parallel()
	const name = "mcp-warm"
	var opened, refuse atomic.Int32
	options := &mcp.Options{Warm: true, Open: func(ctx context.Context, launch mcp.Launch, elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error) {
		if launch.URL != "https://fixture.test/mcp" || refuse.Add(-1) >= 0 {
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
	install := func(t *testing.T, target *Node) protocol.PluginInfo {
		t.Helper()
		return commandFixture(t, target, protocol.Command{Kind: "install_mcp", Data: protocol.CommandInstallMcpPayload{Name: name,
			Definition: protocol.PluginMcpDefinition{Kind: "streamable-http", Data: &protocol.PluginMcpDefinitionStreamableHttpPayload{Url: "https://fixture.test/mcp", Headers: []string{}}}}}).Data.(protocol.PluginInfo)
	}
	subscription := start(t)
	defer func() { _ = node.Stop(context.Background()) }()
	t.Run("a server installed on a running Node connects and publishes its package", func(t *testing.T) {
		install(t, node)
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
	t.Run("a server that failed to start is tried again at the next wake", func(t *testing.T) {
		_ = subscription.Close()
		if err := node.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
		refuse.Store(1)
		subscription = start(t)
		defer subscription.Close()
		waiting, cancel := context.WithTimeout(t.Context(), 20*time.Second)
		defer cancel()
		for refuse.Load() > 0 || read(t).Mcp[0].Runtime == nil || read(t).Mcp[0].Runtime.State != protocol.PluginMcpRuntimeStateFailed {
			if _, err := subscription.Next(waiting); err != nil {
				t.Fatalf("server did not fail: %v", err)
			}
		}
		node.wakeMCP()
		connected(t, subscription, 0)
		if opened.Load() != 4 {
			t.Fatalf("server opened %d times", opened.Load())
		}
	})
	t.Run("disable and enable between warm passes", func(t *testing.T) {
		manual := *options
		manual.Warm = false
		owner, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{MCP: &manual, Environment: &environment})
		if err != nil {
			t.Fatal(err)
		}
		defer owner.Stop(context.Background())
		info := install(t, owner)
		warmed := map[protocol.UserID]map[string]string{}
		initial := opened.Load()
		owner.warmUserMCP(t.Context(), warmed)
		for _, enabled := range []bool{false, true} {
			info = commandFixture(t, owner, protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{
				Name: name, ExpectedRevision: info.Summary.Revision, Enabled: enabled,
			}}).Data.(protocol.PluginInfo)
		}
		owner.warmUserMCP(t.Context(), warmed)
		owner.warmUserMCP(t.Context(), warmed)
		if opened.Load() != initial+2 {
			t.Fatalf("expected two opens across disable and re-enable, got %d", opened.Load()-initial)
		}
	})
}

// A bundled server nobody connected is tried at every start; that it waits
// for its account is no warning, while a configured server that fails is.
func TestWarmFailureLevel(t *testing.T) {
	t.Parallel()
	for code, level := range map[protocol.ErrorCode]slog.Level{
		protocol.ErrorCodeNotConfigured:    slog.LevelDebug,
		protocol.ErrorCodeUnavailable:      slog.LevelWarn,
		protocol.ErrorCodePermissionDenied: slog.LevelWarn,
		protocol.ErrorCodeInternal:         slog.LevelWarn,
	} {
		if got := warmFailureLevel(code); got != level {
			t.Errorf("%s logs at %s, want %s", code, got, level)
		}
	}
}
