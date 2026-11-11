package mcp

import (
	"context"
	"errors"
	"sync/atomic"
	"testing"
	"time"

	"github.com/jonboulle/clockwork"
	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestMCPPoolKey(t *testing.T) {
	clock := clockwork.NewFakeClockAt(time.Unix(100, 0))
	var opened atomic.Int32
	pool := New(t.Context(), Options{Clock: clock, Open: func(ctx context.Context, launch Launch, elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error) {
		opened.Add(1)
		return fakeSession(t, ctx, elicit, false)
	}})
	defer pool.Close()
	base := Configuration{
		Package: protocol.PluginReference{Name: "fixture", Digest: "capture-one", SettingsRevision: 1},
		Server:  "server", Launch: Launch{Kind: "stdio", Command: "fixture", Env: map[string]string{"TOKEN": "private"}},
		Authorization: []any{"connection", uint64(1)}, Options: protocol.DefaultPluginMcpOptions(),
	}
	first, err := pool.Acquire(t.Context(), base)
	if err != nil {
		t.Fatal(err)
	}
	second, err := pool.Acquire(t.Context(), base)
	if err != nil {
		t.Fatal(err)
	}
	if first.connection != second.connection || opened.Load() != 1 {
		t.Fatal("identical captured configurations did not share their MCP owner")
	}
	tools, err := first.List(t.Context())
	if err != nil || len(tools) != 1 || tools[0].Name != "echo" {
		t.Fatalf("captured server tools=%v error=%v", tools, err)
	}
	result, err := first.Call(t.Context(), "echo", map[string]any{"text": "retained capture"}, true, nil)
	if err != nil || len(result.Content) != 1 || result.Content[0].(*sdk.TextContent).Text != "retained capture" {
		t.Fatalf("captured call result=%v error=%v", result, err)
	}
	t.Run("revalidate admitted authority", func(t *testing.T) {
		denied := &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "fixture authority was revoked"}
		allowed, checks := false, 0
		config := base
		config.Validate = func(context.Context) error {
			checks++
			if !allowed {
				return denied
			}
			return nil
		}
		lease, err := pool.Acquire(t.Context(), config)
		if err != nil {
			t.Fatal(err)
		}
		defer lease.Close()
		result, err := lease.Call(t.Context(), "echo", map[string]any{"text": "denied"}, true, nil)
		if result != nil || !errors.Is(err, denied) || checks != 1 {
			t.Fatalf("revoked authority reached RPC: result=%v error=%v checks=%d", result, err, checks)
		}
		allowed = true
		result, err = lease.Call(t.Context(), "echo", map[string]any{"text": "validated"}, true, nil)
		if err != nil || checks != 2 || len(result.Content) != 1 || result.Content[0].(*sdk.TextContent).Text != "validated" {
			t.Fatalf("admitted authority was not revalidated: result=%v error=%v checks=%d", result, err, checks)
		}
		cancelledContext, cancel := context.WithCancel(t.Context())
		cancel()
		result, err = lease.Call(cancelledContext, "echo", nil, true, nil)
		var failure *protocol.Fault
		if result != nil || !errors.As(err, &failure) || failure.Code != protocol.ErrorCodeCancelled || checks != 2 {
			t.Fatalf("pre-dispatch cancellation=%v error=%v checks=%d", result, err, checks)
		}
	})
	for _, entry := range []struct {
		name   string
		change func(*Configuration)
	}{
		{"package", func(config *Configuration) { config.Package.Name = "other" }},
		{"digest", func(config *Configuration) { config.Package.Digest = "capture-two" }},
		{"settings", func(config *Configuration) { config.Package.SettingsRevision++ }},
		{"server", func(config *Configuration) { config.Server = "other" }},
		{"authorization", func(config *Configuration) { config.Authorization = []any{"connection", uint64(2)} }},
		{"launch", func(config *Configuration) { config.Launch.Args = []string{"different"} }},
	} {
		t.Run(entry.name, func(t *testing.T) {
			config := base
			entry.change(&config)
			current, err := pool.Acquire(t.Context(), config)
			if err != nil {
				t.Fatal(err)
			}
			if current.connection == first.connection {
				t.Fatal("a changed capture reused another MCP authority")
			}
			if err := current.Close(); err != nil {
				t.Fatal(err)
			}
		})
	}
	result, err = first.Call(t.Context(), "echo", map[string]any{"text": "old turn"}, true, nil)
	if err != nil || len(result.Content) != 1 {
		t.Fatalf("retired capture lost its admitted lease: %v", err)
	}
	if err := first.Close(); err != nil {
		t.Fatal(err)
	}
	if err := second.Close(); err != nil {
		t.Fatal(err)
	}
	clock.Advance(idleTTL)
	if err := pool.Expire(); err != nil {
		t.Fatal(err)
	}
	pool.mu.Lock()
	remaining := len(pool.entries)
	pool.mu.Unlock()
	if remaining != 0 {
		t.Fatal("idle owners survived expiration")
	}
	if Alias("plugin", "server", "tool") == Alias("plugin", "server", "other") || Alias("a/b", "server", "tool") == Alias("a_b", "server", "tool") {
		t.Fatal("tool aliases did not retain their full namespaced identity")
	}
}

func TestMCPCleanupUnknown(t *testing.T) {
	t.Run("retain both acquisition cleanup failures", func(t *testing.T) {
		pool := New(t.Context(), Options{Open: func(ctx context.Context, launch Launch, elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error) {
			return fakeSession(t, ctx, elicit, false)
		}})
		defer pool.Close()
		retired := errors.New("fixture retired resource cleanup failed")
		newlyOpened := errors.New("fixture acquired resource cleanup failed")
		config := Configuration{
			Package: protocol.PluginReference{Name: "cleanup", Digest: "capture"}, Server: "echo", Options: protocol.DefaultPluginMcpOptions(),
			Authorization: 1, Release: func() error { return retired },
		}
		lease, err := pool.Acquire(t.Context(), config)
		if err != nil {
			t.Fatal(err)
		}
		if err := lease.Close(); err != nil {
			t.Fatal(err)
		}
		config.Authorization = 2
		config.Options.Reuse = false
		config.Release = func() error { return newlyOpened }
		lease, err = pool.Acquire(t.Context(), config)
		if lease != nil || !errors.Is(err, retired) || !errors.Is(err, newlyOpened) {
			t.Fatalf("acquisition discarded a resource owner: lease=%v error=%v", lease, err)
		}
	})
	t.Run("dead reusable server is replaced", func(t *testing.T) {
		var servers []*sdk.ServerSession
		pool := New(t.Context(), Options{Open: func(ctx context.Context, launch Launch, elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error) {
			server := sdk.NewServer(&sdk.Implementation{Name: "fixture", Version: "1"}, nil)
			sdk.AddTool(server, &sdk.Tool{Name: "echo"}, func(ctx context.Context, request *sdk.CallToolRequest, input struct{}) (*sdk.CallToolResult, any, error) {
				return &sdk.CallToolResult{}, nil, nil
			})
			clientWire, serverWire := sdk.NewInMemoryTransports()
			session, err := server.Connect(t.Context(), serverWire, nil)
			if err != nil {
				return nil, err
			}
			servers = append(servers, session)
			return sdk.NewClient(&sdk.Implementation{Name: "fixture", Version: "1"}, &sdk.ClientOptions{ElicitationHandler: elicit}).Connect(ctx, clientWire, nil)
		}})
		defer pool.Close()
		options := protocol.DefaultPluginMcpOptions()
		options.Reuse = true
		config := Configuration{Package: protocol.PluginReference{Name: "dead", Digest: "capture"}, Server: "echo", Options: options, Authorization: 1}
		list := func() error {
			lease, err := pool.Acquire(t.Context(), config)
			if err != nil {
				return err
			}
			_, err = lease.List(t.Context())
			return errors.Join(err, lease.Close())
		}
		if err := list(); err != nil {
			t.Fatal(err)
		}
		_ = servers[0].Close()
		_ = servers[0].Wait()
		// The closed transport is noticed either before or by the first use.
		if err := list(); err != nil {
			if err = list(); err != nil {
				t.Fatalf("dead connection stayed in the pool: %v", err)
			}
		}
		if len(servers) != 2 {
			t.Fatalf("connections opened=%d", len(servers))
		}
	})
	for _, fail := range []bool{false, true} {
		t.Run(map[bool]string{false: "confirmed", true: "unconfirmed"}[fail], func(t *testing.T) {
			pool := New(t.Context(), Options{Open: func(ctx context.Context, launch Launch, elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error) {
				return fakeSession(t, ctx, elicit, fail)
			}})
			config := Configuration{Package: protocol.PluginReference{Name: "cleanup", Digest: "captured"}, Server: "echo", Options: protocol.DefaultPluginMcpOptions()}
			lease, err := pool.Acquire(t.Context(), config)
			if err != nil {
				t.Fatal(err)
			}
			if err := lease.Close(); err != nil {
				t.Fatal(err)
			}
			err = pool.Close()
			if !fail {
				if err != nil {
					t.Fatal(err)
				}
			} else {
				var fault *protocol.Fault
				if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeOutcomeUnknown {
					t.Fatalf("uncertain transport cleanup=%v", err)
				}
			}
		})
	}
}

func fakeSession(t *testing.T, ctx context.Context, elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error), fail bool) (*sdk.ClientSession, error) {
	t.Helper()
	server := sdk.NewServer(&sdk.Implementation{Name: "fixture", Version: "1"}, nil)
	sdk.AddTool(server, &sdk.Tool{Name: "echo"}, func(ctx context.Context, request *sdk.CallToolRequest, input struct {
		Text string `json:"text"`
	}) (*sdk.CallToolResult, any, error) {
		return &sdk.CallToolResult{Content: []sdk.Content{&sdk.TextContent{Text: input.Text}}}, nil, nil
	})
	clientWire, serverWire := sdk.NewInMemoryTransports()
	serverSession, err := server.Connect(t.Context(), serverWire, nil)
	if err != nil {
		return nil, err
	}
	t.Cleanup(func() { _ = serverSession.Close() })
	var transport sdk.Transport = clientWire
	if fail {
		transport = failingTransport{clientWire}
	}
	client := sdk.NewClient(&sdk.Implementation{Name: "fixture", Version: "1"}, &sdk.ClientOptions{ElicitationHandler: elicit})
	return client.Connect(ctx, transport, nil)
}

type failingTransport struct{ sdk.Transport }

func (transport failingTransport) Connect(ctx context.Context) (sdk.Connection, error) {
	connection, err := transport.Transport.Connect(ctx)
	if err != nil {
		return nil, err
	}
	return failingConnection{connection}, nil
}

type failingConnection struct{ sdk.Connection }

func (connection failingConnection) Close() error {
	_ = connection.Connection.Close()
	return errors.New("fixture transport cleanup was not acknowledged")
}
