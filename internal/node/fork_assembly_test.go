package node_test

import (
	"context"
	"errors"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/jonboulle/clockwork"
	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestForkNodeOptionsMCP(t *testing.T) {
	const endpoint = "https://fork.invalid/mcp"
	ctx, cancel := context.WithTimeout(t.Context(), 20*time.Second)
	defer cancel()
	var opens atomic.Int32
	var connected atomic.Pointer[sdk.ServerSession]
	owner, err := node.StartWithOptions(ctx, filepath.Join(t.TempDir(), "profile"), node.Options{MCP: &mcp.Options{
		Open: func(ctx context.Context, launch mcp.Launch, elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error) {
			if launch.Kind != "streamable-http" || launch.URL != endpoint {
				return nil, errors.New("unexpected MCP launch in fork fixture")
			}
			if opens.Add(1) == 1 {
				return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fork service failed to start"}
			}
			server := sdk.NewServer(&sdk.Implementation{Name: "fork-server", Version: "1"}, nil)
			sdk.AddTool(server, &sdk.Tool{Name: "fork_status"}, func(context.Context, *sdk.CallToolRequest, struct{}) (*sdk.CallToolResult, any, error) {
				return &sdk.CallToolResult{Content: []sdk.Content{&sdk.TextContent{Text: "ready"}}}, nil, nil
			})
			clientWire, serverWire := sdk.NewInMemoryTransports()
			serverSession, err := server.Connect(ctx, serverWire, nil)
			if err != nil {
				return nil, err
			}
			client := sdk.NewClient(&sdk.Implementation{Name: "fork-client", Version: "1"}, &sdk.ClientOptions{ElicitationHandler: elicit})
			session, err := client.Connect(ctx, clientWire, nil)
			if err != nil {
				return nil, errors.Join(err, serverSession.Close())
			}
			connected.Store(serverSession)
			return session, nil
		},
	}})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := owner.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})

	install := protocol.Command{Kind: "install_mcp", Data: protocol.CommandInstallMcpPayload{
		Name: "mcp-fork-fixture", Secrets: map[string]protocol.PluginSettingsSecretUpdate{},
		Definition: protocol.PluginMcpDefinition{Kind: "streamable-http", Data: &protocol.PluginMcpDefinitionStreamableHttpPayload{Url: endpoint, Headers: []string{}}},
	}}
	output, err := forkDispatch(ctx, owner, owner.ID(), install)
	info, ok := output.Data.(protocol.PluginInfo)
	if err != nil || output.Kind != "plugin" || !ok {
		t.Fatalf("install fork MCP: %v, %+v", err, output)
	}
	_, err = forkDispatch(ctx, owner, owner.ID(), install)
	requireForkFault(t, err, protocol.ErrorCodeRevisionConflict)
	check := protocol.Command{Kind: "check_plugin_mcp", Data: protocol.CommandCheckPluginMcpPayload{Package: plugins.PackageReference(info), Server: "server"}}
	unpaired := owner.ID()
	unpaired[0] ^= 1
	_, err = forkDispatch(ctx, owner, unpaired, check)
	requireForkFault(t, err, protocol.ErrorCodePermissionDenied)
	if got := opens.Load(); got != 0 {
		t.Fatalf("fork opener ran before an authorized check: %d opens", got)
	}

	_, err = forkDispatch(ctx, owner, owner.ID(), check)
	requireForkFault(t, err, protocol.ErrorCodeUnavailable)
	output, err = forkDispatch(ctx, owner, owner.ID(), check)
	checked, ok := output.Data.(protocol.PluginMcpCheck)
	if err != nil || output.Kind != "plugin_mcp_checked" || !ok || len(checked.Tools) != 1 || checked.Tools[0].Name != "fork_status" {
		t.Fatalf("check fork MCP: %v, %+v", err, output)
	}
	peer := connected.Load()
	if got := opens.Load(); got != 2 || peer == nil {
		t.Fatalf("fork startup attempts = %d, connected = %v", got, peer != nil)
	}
	defer peer.Close()
	if err := peer.Ping(ctx, nil); err != nil {
		t.Fatalf("fork MCP did not remain connected: %v", err)
	}
	if err := owner.Stop(ctx); err != nil {
		t.Fatal(err)
	}
	closed := make(chan struct{})
	go func() {
		_ = peer.Wait()
		close(closed)
	}()
	select {
	case <-closed:
	case <-ctx.Done():
		t.Fatal("Node shutdown did not close the fork's MCP connection")
	}
}

func TestForkNodeStartupFailureClosesMCPPool(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	clock := startupClock{Clock: clockwork.NewFakeClock(), cancel: cancel, stopped: make(chan struct{})}
	path := filepath.Join(t.TempDir(), "profile")
	owner, err := node.StartWithOptions(ctx, path, node.Options{MCP: &mcp.Options{Clock: clock}})
	if owner != nil {
		_ = owner.Stop(context.Background())
	}
	if owner != nil || !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled startup returned Node %v and error %v", owner != nil, err)
	}
	select {
	case <-clock.stopped:
	default:
		t.Fatal("failed Node startup left the MCP pool running")
	}
	profile, err := node.OpenProfile(t.Context(), path)
	if err != nil {
		t.Fatalf("failed Node startup did not release the profile: %v", err)
	}
	if err := profile.Close(); err != nil {
		t.Fatal(err)
	}
}

type startupClock struct {
	clockwork.Clock
	cancel  context.CancelFunc
	stopped chan struct{}
}

func (clock startupClock) NewTicker(duration time.Duration) clockwork.Ticker {
	ticker := clock.Clock.NewTicker(duration)
	clock.cancel()
	return startupTicker{Ticker: ticker, stopped: clock.stopped}
}

type startupTicker struct {
	clockwork.Ticker
	stopped chan struct{}
}

func (ticker startupTicker) Stop() {
	ticker.Ticker.Stop()
	close(ticker.stopped)
}

func forkDispatch(ctx context.Context, owner *node.Node, caller protocol.NodeID, command protocol.Command) (protocol.Output, error) {
	accepted, err := owner.Dispatch(ctx, caller, protocol.NewRequest(owner.ID(), command))
	if err != nil {
		return protocol.Output{}, err
	}
	select {
	case result, ok := <-accepted.Completion:
		if !ok {
			return protocol.Output{}, errors.New("Node closed completion without a response")
		}
		if result.Fault != nil {
			return protocol.Output{}, result.Fault
		}
		return result.Output, nil
	case <-ctx.Done():
		return protocol.Output{}, ctx.Err()
	}
}

func requireForkFault(t *testing.T, err error, code protocol.ErrorCode) {
	t.Helper()
	var fault *protocol.Fault
	if !errors.As(err, &fault) || fault.Code != code {
		t.Fatalf("expected fault %s, got %v", code, err)
	}
}
