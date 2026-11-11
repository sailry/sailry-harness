package mcp

import (
	"context"
	"errors"
	"io"
	"sync/atomic"
	"testing"
	"testing/synctest"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/modelcontextprotocol/go-sdk/jsonrpc"
	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestMCPPoolKey(t *testing.T) {
	clock := clockwork.NewFakeClockAt(time.Unix(100, 0))
	var opened atomic.Int32
	pool := New(t.Context(), Options{Clock: clock, Open: func(ctx context.Context, launch Launch, elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error) {
		opened.Add(1)
		return fakeSession(t, ctx, elicit, nil)
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
			return fakeSession(t, ctx, elicit, nil)
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
	// The source checks the connection when it is acquired. A closed session
	// is replaced before its next use; a transport that only fails its
	// requests is retired by the failed listing.
	for _, row := range []struct {
		name     string
		attempts int
	}{{"dead reusable server is replaced", 1}, {"failed reusable transport is replaced", 2}} {
		t.Run(row.name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				var servers []*sdk.ServerSession
				var wires []*atomic.Bool
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
					wires = append(wires, &atomic.Bool{})
					return sdk.NewClient(&sdk.Implementation{Name: "fixture", Version: "1"}, &sdk.ClientOptions{ElicitationHandler: elicit}).Connect(ctx, brokenTransport{clientWire, wires[len(wires)-1]}, nil)
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
				if row.attempts == 1 {
					_ = servers[0].Close()
					synctest.Wait()
				} else {
					wires[0].Store(true)
					if err := list(); err == nil {
						t.Fatal("listing passed a failed transport")
					}
				}
				if err := list(); err != nil {
					t.Fatalf("dead connection stayed in the pool: %v", err)
				}
				if len(servers) != 2 {
					t.Fatalf("connections opened=%d", len(servers))
				}
			})
		})
	}
	for _, row := range []struct {
		name string
		wrap func(sdk.Transport) sdk.Transport
		fail bool
	}{
		{"confirmed", nil, false},
		{"unconfirmed", func(wire sdk.Transport) sdk.Transport { return failingTransport{wire} }, true},
		// How the stream ended says nothing of the transport's own close.
		{"confirmed with a severed stream", func(wire sdk.Transport) sdk.Transport { return severedTransport{wire} }, false},
	} {
		t.Run(row.name, func(t *testing.T) {
			fail := row.fail
			pool := New(t.Context(), Options{Open: func(ctx context.Context, launch Launch, elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error) {
				return fakeSession(t, ctx, elicit, row.wrap)
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

func fakeSession(t *testing.T, ctx context.Context, elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error), wrap func(sdk.Transport) sdk.Transport) (*sdk.ClientSession, error) {
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
	if wrap != nil {
		transport = wrap(clientWire)
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

// severedTransport ends its stream with an error of its own instead of the
// end of the stream, as a pipe closed from this side does.
type severedTransport struct{ sdk.Transport }

func (transport severedTransport) Connect(ctx context.Context) (sdk.Connection, error) {
	connection, err := transport.Transport.Connect(ctx)
	return severedConnection{connection}, err
}

type severedConnection struct{ sdk.Connection }

func (connection severedConnection) Read(ctx context.Context) (jsonrpc.Message, error) {
	message, err := connection.Connection.Read(ctx)
	if err != nil {
		err = io.ErrClosedPipe
	}
	return message, err
}

// brokenTransport fails every write once its flag is set while the session
// itself stays open, as a remote endpoint that stopped answering does.
type brokenTransport struct {
	sdk.Transport
	broken *atomic.Bool
}

func (transport brokenTransport) Connect(ctx context.Context) (sdk.Connection, error) {
	connection, err := transport.Transport.Connect(ctx)
	return brokenConnection{connection, transport.broken}, err
}

type brokenConnection struct {
	sdk.Connection
	broken *atomic.Bool
}

func (connection brokenConnection) Write(ctx context.Context, message jsonrpc.Message) error {
	if connection.broken.Load() {
		return errors.New("fixture transport failure")
	}
	return connection.Connection.Write(ctx, message)
}

// lifecycleFixture opens in-memory servers with one tool and keeps their
// sessions, so a case can end one or see that the pool closed it.
type lifecycleFixture struct {
	servers []*sdk.ServerSession
	fail    atomic.Bool
	hold    chan struct{}
}

func (fixture *lifecycleFixture) open(t *testing.T) func(context.Context, Launch, func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error) {
	return func(ctx context.Context, _ Launch, elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error) {
		if fixture.hold != nil {
			<-ctx.Done()
			return nil, ctx.Err()
		}
		if fixture.fail.Load() {
			return nil, unavailable()
		}
		server := sdk.NewServer(&sdk.Implementation{Name: "fixture", Version: "1"}, nil)
		sdk.AddTool(server, &sdk.Tool{Name: "echo"}, func(context.Context, *sdk.CallToolRequest, struct{}) (*sdk.CallToolResult, any, error) {
			return &sdk.CallToolResult{}, nil, nil
		})
		clientWire, serverWire := sdk.NewInMemoryTransports()
		session, err := server.Connect(t.Context(), serverWire, nil)
		if err != nil {
			return nil, err
		}
		fixture.servers = append(fixture.servers, session)
		return sdk.NewClient(&sdk.Implementation{Name: "fixture", Version: "1"}, &sdk.ClientOptions{ElicitationHandler: elicit}).Connect(ctx, clientWire, nil)
	}
}

func TestMCPLifecycle(t *testing.T) {
	summary := protocol.PluginSummary{Name: "lifecycle", Digest: "capture", SettingsRevision: 1, Revision: 1, Enabled: true}
	summary.Compatibility.Kind = "compatible"
	reference := protocol.PluginReference{Name: summary.Name, Digest: summary.Digest, SettingsRevision: summary.SettingsRevision}
	options := protocol.DefaultPluginMcpOptions()
	options.Reuse = true
	config := Configuration{Package: reference, Server: "echo", Options: options, Authorization: 1}
	list := func(t *testing.T, pool *Pool, ctx context.Context) error {
		lease, err := pool.Acquire(ctx, config)
		if err != nil {
			return err
		}
		_, err = lease.List(ctx)
		return errors.Join(err, lease.Close())
	}
	state := func(pool *Pool) string {
		runtime := pool.Status(reference, "echo")
		if runtime == nil {
			return "none"
		}
		return string(runtime.State) + ":" + string(rune('0'+runtime.Tools))
	}
	changed := func(pool *Pool) bool {
		select {
		case <-pool.Changed():
			names := pool.Changes()
			return len(names) == 1 && names[0] == summary.Name
		default:
			return false
		}
	}
	t.Run("an idle server leaves on the pool's own timer", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			clock := clockwork.NewFakeClockAt(time.Unix(100, 0))
			fixture := &lifecycleFixture{}
			pool := New(t.Context(), Options{Clock: clock, Open: fixture.open(t)})
			defer pool.Close()
			if err := pool.Observe(summary); err != nil {
				t.Fatal(err)
			}
			if err := list(t, pool, t.Context()); err != nil {
				t.Fatal(err)
			}
			ended := make(chan struct{})
			go func() { _ = fixture.servers[0].Wait(); close(ended) }()
			clock.Advance(idleTTL - time.Second)
			synctest.Wait()
			select {
			case <-ended:
				t.Fatal("a server was closed before its idle time passed")
			default:
			}
			clock.Advance(expiryInterval)
			synctest.Wait()
			select {
			case <-ended:
			default:
				t.Fatal("an idle server kept its connection with no later acquisition")
			}
			pool.mu.Lock()
			entries, owners := len(pool.entries), len(pool.owners)
			pool.mu.Unlock()
			if entries != 0 || owners != 0 {
				t.Fatalf("expired server stayed owned: entries=%d owners=%d", entries, owners)
			}
			// The server that was closed for being idle did not fail.
			if state(pool) != "connected:1" {
				t.Fatalf("expired server reports %s", state(pool))
			}
			if err := list(t, pool, t.Context()); err != nil || len(fixture.servers) != 2 {
				t.Fatalf("expired server was not reopened: %v, opened=%d", err, len(fixture.servers))
			}
		})
	})
	t.Run("a server reports each change of its runtime once", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			fixture := &lifecycleFixture{}
			pool := New(t.Context(), Options{Open: fixture.open(t)})
			defer pool.Close()
			if err := pool.Observe(summary); err != nil {
				t.Fatal(err)
			}
			if state(pool) != "none" || changed(pool) {
				t.Fatal("a server that was never asked to connect reported a runtime")
			}
			lease, err := pool.Acquire(t.Context(), config)
			if err != nil {
				t.Fatal(err)
			}
			if state(pool) != "connecting:0" || !changed(pool) {
				t.Fatalf("opened server reports %s", state(pool))
			}
			if _, err := lease.List(t.Context()); err != nil {
				t.Fatal(err)
			}
			if state(pool) != "connected:1" || !changed(pool) {
				t.Fatalf("listed server reports %s", state(pool))
			}
			if err := errors.Join(lease.Close(), list(t, pool, t.Context())); err != nil {
				t.Fatal(err)
			}
			if state(pool) != "connected:1" || changed(pool) {
				t.Fatal("an unchanged runtime was reported again")
			}
			// A caller that gives up leaves the runtime as it was.
			gone, cancel := context.WithCancel(t.Context())
			cancel()
			lease, err = pool.Acquire(t.Context(), config)
			if err != nil {
				t.Fatal(err)
			}
			if _, err := lease.List(gone); err == nil {
				t.Fatal("a cancelled listing passed")
			}
			if err := lease.Close(); err != nil {
				t.Fatal(err)
			}
			if state(pool) != "connected:1" || changed(pool) {
				t.Fatalf("cancelled listing reports %s", state(pool))
			}
			// The server ends its side: the runtime is failed until it reopens.
			_ = fixture.servers[0].Close()
			synctest.Wait()
			if state(pool) != "failed:0" || !changed(pool) {
				t.Fatalf("ended server reports %s", state(pool))
			}
			fixture.fail.Store(true)
			if err := list(t, pool, t.Context()); err == nil {
				t.Fatal("an unavailable server opened")
			}
			runtime := pool.Status(reference, "echo")
			if runtime == nil || runtime.State != protocol.PluginMcpRuntimeStateFailed || runtime.Fault == nil || runtime.Fault.Code != protocol.ErrorCodeUnavailable {
				t.Fatalf("failed server reports %+v", runtime)
			}
			// Connecting and failed again: two changes, one wake.
			if !changed(pool) || changed(pool) {
				t.Fatal("a failed reopening was not reported once")
			}
			fixture.fail.Store(false)
			if err := list(t, pool, t.Context()); err != nil || state(pool) != "connected:1" || !changed(pool) {
				t.Fatalf("recovered server reports %s: %v", state(pool), err)
			}
			// Another version of the package starts without a runtime, and a
			// connection of the old one no longer reports.
			old, err := pool.Acquire(t.Context(), config)
			if err != nil {
				t.Fatal(err)
			}
			next := summary
			next.Digest, next.Revision = "capture-two", 2
			if err := pool.Observe(next); err != nil {
				t.Fatal(err)
			}
			if _, err := old.List(t.Context()); err != nil {
				t.Fatal(err)
			}
			if err := old.Close(); err != nil {
				t.Fatal(err)
			}
			current := protocol.PluginReference{Name: next.Name, Digest: next.Digest, SettingsRevision: next.SettingsRevision}
			if pool.Status(current, "echo") != nil || pool.Status(reference, "echo") != nil || changed(pool) {
				t.Fatal("a replaced package version kept or reported a runtime")
			}
		})
	})
	t.Run("an opening that is closed is not a failure", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			fixture := &lifecycleFixture{hold: make(chan struct{})}
			pool := New(t.Context(), Options{Open: fixture.open(t)})
			defer pool.Close()
			if err := pool.Observe(summary); err != nil {
				t.Fatal(err)
			}
			// A server that is not shared is closed with its only caller; a
			// shared one keeps opening for the next.
			single := config
			single.Options.Reuse = false
			waiting, cancel := context.WithCancel(t.Context())
			result := make(chan error, 1)
			go func() {
				lease, err := pool.Acquire(waiting, single)
				if err == nil {
					err = errors.Join(errors.New("a cancelled acquisition passed"), lease.Close())
				}
				result <- err
			}()
			synctest.Wait()
			if state(pool) != "connecting:0" {
				t.Fatalf("opening server reports %s", state(pool))
			}
			cancel()
			var fault *protocol.Fault
			if err := <-result; !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeCancelled {
				t.Fatal(err)
			}
			synctest.Wait()
			if state(pool) != "none" {
				t.Fatalf("abandoned opening reports %s", state(pool))
			}
		})
	})
	t.Run("an opening that runs out of time fails", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			fixture := &lifecycleFixture{hold: make(chan struct{})}
			pool := New(t.Context(), Options{Open: fixture.open(t)})
			defer pool.Close()
			if err := pool.Observe(summary); err != nil {
				t.Fatal(err)
			}
			started := time.Now()
			if err := list(t, pool, t.Context()); err == nil {
				t.Fatal("a server that never answered opened")
			}
			if waited := time.Since(started); waited != 20*time.Second {
				t.Fatalf("opening waited %s", waited)
			}
			if state(pool) != "failed:0" {
				t.Fatalf("timed out opening reports %s", state(pool))
			}
		})
	})
}
