package mcp

import (
	"context"
	"errors"
	"fmt"
	"sync"
	"sync/atomic"
	"testing"
	"testing/synctest"
	"time"

	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type mcpCallOutcome struct {
	value *sdk.CallToolResult
	err   error
}

type mcpHeldCall struct {
	id      string
	started chan struct{}
	release chan struct{}
	done    chan mcpCallOutcome
	once    sync.Once
	entered atomic.Int32
}

func (call *mcpHeldCall) finish() { call.once.Do(func() { close(call.release) }) }

type mcpCallFixture struct {
	t       *testing.T
	pool    *Pool
	version string
	pending sync.Map
	opened  atomic.Int32
	active  atomic.Int32
	peak    atomic.Int32
	mu      sync.Mutex
	servers []*sdk.ServerSession
	leases  []*Lease
}

func newMCPCallFixture(t *testing.T, version string) *mcpCallFixture {
	t.Helper()
	fixture := &mcpCallFixture{t: t, version: version}
	fixture.pool = New(t.Context(), Options{Open: fixture.open})
	t.Cleanup(func() {
		fixture.pending.Range(func(_, value any) bool {
			value.(*mcpHeldCall).finish()
			return true
		})
		for _, lease := range fixture.leases {
			if err := lease.Close(); err != nil {
				t.Errorf("close fixture lease: %v", err)
			}
		}
		if err := fixture.pool.Close(); err != nil {
			t.Errorf("close fixture pool: %v", err)
		}
		fixture.mu.Lock()
		servers := append([]*sdk.ServerSession(nil), fixture.servers...)
		fixture.mu.Unlock()
		for _, server := range servers {
			if err := server.Close(); err != nil {
				t.Errorf("close fixture server: %v", err)
			}
		}
	})
	return fixture
}

func (fixture *mcpCallFixture) config() Configuration {
	return Configuration{
		User: "fixture-user", Package: protocol.PluginReference{Name: "concurrency", Digest: "capture"},
		Server: "fixture", Options: protocol.DefaultPluginMcpOptions(),
	}
}

func (fixture *mcpCallFixture) open(ctx context.Context, _ Launch, elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)) (*sdk.ClientSession, error) {
	fixture.opened.Add(1)
	server := sdk.NewServer(&sdk.Implementation{Name: "concurrency-fixture", Version: "1"}, &sdk.ServerOptions{SupportedProtocolVersions: []string{fixture.version}})
	sdk.AddTool(server, &sdk.Tool{Name: "hold"}, func(ctx context.Context, _ *sdk.CallToolRequest, input struct {
		ID string `json:"id"`
	}) (*sdk.CallToolResult, any, error) {
		value, exists := fixture.pending.Load(input.ID)
		if !exists {
			return nil, nil, errors.New("unregistered fixture call")
		}
		call := value.(*mcpHeldCall)
		if call.entered.Add(1) != 1 {
			return nil, nil, errors.New("fixture call was replayed")
		}
		active := fixture.active.Add(1)
		defer fixture.active.Add(-1)
		for peak := fixture.peak.Load(); active > peak; peak = fixture.peak.Load() {
			if fixture.peak.CompareAndSwap(peak, active) {
				break
			}
		}
		close(call.started)
		select {
		case <-call.release:
			return &sdk.CallToolResult{Content: []sdk.Content{&sdk.TextContent{Text: input.ID}}}, nil, nil
		case <-ctx.Done():
			return nil, nil, ctx.Err()
		}
	})
	clientWire, serverWire := sdk.NewInMemoryTransports()
	session, err := server.Connect(fixture.t.Context(), serverWire, nil)
	if err != nil {
		return nil, err
	}
	fixture.mu.Lock()
	fixture.servers = append(fixture.servers, session)
	fixture.mu.Unlock()
	client := sdk.NewClient(&sdk.Implementation{Name: "concurrency-client", Version: "1"}, &sdk.ClientOptions{ElicitationHandler: elicit})
	return client.Connect(ctx, clientWire, &sdk.ClientSessionOptions{ProtocolVersion: fixture.version})
}

func (fixture *mcpCallFixture) acquire(config Configuration) *Lease {
	fixture.t.Helper()
	lease, err := fixture.pool.Acquire(fixture.t.Context(), config)
	if err != nil {
		fixture.t.Fatal(err)
	}
	fixture.leases = append(fixture.leases, lease)
	if got := lease.connection.session.InitializeResult().ProtocolVersion; got != fixture.version {
		fixture.t.Fatalf("negotiated protocol = %q, want %q", got, fixture.version)
	}
	return lease
}

func (fixture *mcpCallFixture) start(ctx context.Context, lease *Lease, id string, readOnly bool) *mcpHeldCall {
	fixture.t.Helper()
	call := &mcpHeldCall{id: id, started: make(chan struct{}), release: make(chan struct{}), done: make(chan mcpCallOutcome, 1)}
	if _, exists := fixture.pending.LoadOrStore(id, call); exists {
		fixture.t.Fatalf("duplicate fixture call %q", id)
	}
	go func() {
		value, err := lease.Call(ctx, "hold", map[string]any{"id": id}, readOnly, nil)
		call.done <- mcpCallOutcome{value, err}
	}()
	return call
}

func (fixture *mcpCallFixture) entered(call *mcpHeldCall, want bool) {
	fixture.t.Helper()
	synctest.Wait()
	select {
	case <-call.started:
		if !want {
			fixture.t.Fatalf("%s reached the server before its turn", call.id)
		}
	default:
		if want {
			fixture.t.Fatalf("%s did not reach the server", call.id)
		}
	}
}

func (fixture *mcpCallFixture) success(call *mcpHeldCall) {
	fixture.t.Helper()
	result := <-call.done
	if result.err != nil || result.value == nil || result.value.IsError || len(result.value.Content) != 1 {
		fixture.t.Fatalf("%s result = %+v, error = %v", call.id, result.value, result.err)
	}
	text, ok := result.value.Content[0].(*sdk.TextContent)
	if !ok || text.Text != call.id {
		fixture.t.Fatalf("%s received another call's result: %+v", call.id, result.value)
	}
}

func (fixture *mcpCallFixture) fault(call *mcpHeldCall, code protocol.ErrorCode) {
	fixture.t.Helper()
	result := <-call.done
	var failure *protocol.Fault
	if result.value != nil || !errors.As(result.err, &failure) || failure.Code != code {
		fixture.t.Fatalf("%s result = %+v, error = %v, want %s", call.id, result.value, result.err, code)
	}
}

func TestMCPConcurrentReadsRefillSlots(t *testing.T) {
	for _, version := range []string{"2026-07-28", "2025-11-25"} {
		t.Run(version, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				fixture := newMCPCallFixture(t, version)
				lease := fixture.acquire(fixture.config())
				calls := make([]*mcpHeldCall, 9)
				for i := range calls {
					calls[i] = fixture.start(t.Context(), lease, fmt.Sprintf("read-%d", i), true)
					fixture.entered(calls[i], i < 8)
				}
				if fixture.peak.Load() != 8 {
					t.Fatalf("concurrent reads = %d, want 8", fixture.peak.Load())
				}
				calls[0].finish()
				fixture.success(calls[0])
				fixture.entered(calls[8], true)
				// The ninth read starts while the other seven original reads still wait.
				for _, call := range calls[1:8] {
					select {
					case result := <-call.done:
						t.Fatalf("%s ended before release: %+v", call.id, result)
					default:
					}
				}
				for _, call := range calls[1:] {
					call.finish()
					fixture.success(call)
				}
				if fixture.peak.Load() != 8 {
					t.Fatalf("read limit exceeded: %d", fixture.peak.Load())
				}
			})
		})
	}
}

func TestMCPQueuedWriterPrecedesLaterReads(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		fixture := newMCPCallFixture(t, "2026-07-28")
		lease := fixture.acquire(fixture.config())
		first := fixture.start(t.Context(), lease, "first-read", true)
		second := fixture.start(t.Context(), lease, "second-read", true)
		fixture.entered(first, true)
		fixture.entered(second, true)
		writer := fixture.start(t.Context(), lease, "writer", false)
		fixture.entered(writer, false)
		later := fixture.start(t.Context(), lease, "later-read", true)
		fixture.entered(later, false)
		first.finish()
		fixture.success(first)
		fixture.entered(writer, false)
		fixture.entered(later, false)
		second.finish()
		fixture.success(second)
		fixture.entered(writer, true)
		fixture.entered(later, false)
		writer.finish()
		fixture.success(writer)
		fixture.entered(later, true)
		later.finish()
		fixture.success(later)
	})
}

func TestMCPExclusiveWritesAcrossConnections(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		fixture := newMCPCallFixture(t, "2026-07-28")
		config := fixture.config()
		config.Options.Reuse, config.Options.Exclusive = false, true
		first, second := fixture.acquire(config), fixture.acquire(config)
		if first.connection == second.connection {
			t.Fatal("nonreusable leases shared a connection")
		}
		one := fixture.start(t.Context(), first, "first-write", false)
		fixture.entered(one, true)
		two := fixture.start(t.Context(), second, "second-write", false)
		fixture.entered(two, false)
		one.finish()
		fixture.success(one)
		fixture.entered(two, true)
		two.finish()
		fixture.success(two)
		if fixture.peak.Load() != 1 {
			t.Fatalf("exclusive writes overlapped: %d", fixture.peak.Load())
		}
	})
}

func TestMCPReadInterruptionKeepsSharedConnection(t *testing.T) {
	for _, timedOut := range []bool{false, true} {
		t.Run(map[bool]string{false: "cancel", true: "timeout"}[timedOut], func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				fixture := newMCPCallFixture(t, "2026-07-28")
				lease := fixture.acquire(fixture.config())
				ctx, cancel := context.WithCancel(t.Context())
				defer cancel()
				interrupted := fixture.start(ctx, lease, "interrupted-read", true)
				fixture.entered(interrupted, true)
				if timedOut {
					time.Sleep(time.Minute)
				}
				neighbor := fixture.start(t.Context(), lease, "neighbor-read", true)
				fixture.entered(neighbor, true)
				if timedOut {
					time.Sleep(time.Minute)
					fixture.fault(interrupted, protocol.ErrorCodeUnavailable)
				} else {
					cancel()
					fixture.fault(interrupted, protocol.ErrorCodeCancelled)
				}
				neighbor.finish()
				fixture.success(neighbor)
				reused := fixture.acquire(fixture.config())
				if reused.connection != lease.connection || fixture.opened.Load() != 1 {
					t.Fatal("one interrupted read retired the shared connection")
				}
				next := fixture.start(t.Context(), reused, "next-read", true)
				fixture.entered(next, true)
				next.finish()
				fixture.success(next)
			})
		})
	}
}

func TestMCPQueuedCancellationDoesNotDispatch(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		fixture := newMCPCallFixture(t, "2026-07-28")
		lease := fixture.acquire(fixture.config())
		var occupied []*mcpHeldCall
		for i := range 8 {
			call := fixture.start(t.Context(), lease, fmt.Sprintf("occupied-%d", i), true)
			fixture.entered(call, true)
			occupied = append(occupied, call)
		}
		ctx, cancel := context.WithCancel(t.Context())
		defer cancel()
		queued := fixture.start(ctx, lease, "cancelled-queue", true)
		fixture.entered(queued, false)
		cancel()
		fixture.fault(queued, protocol.ErrorCodeCancelled)
		occupied[0].finish()
		fixture.success(occupied[0])
		next := fixture.start(t.Context(), lease, "after-queue", true)
		fixture.entered(next, true)
		fixture.entered(queued, false)
		next.finish()
		fixture.success(next)
		for _, call := range occupied[1:] {
			call.finish()
			fixture.success(call)
		}
		fixture.entered(queued, false)
	})
}

func TestMCPWriteCancellationRetiresWithoutReplay(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		fixture := newMCPCallFixture(t, "2026-07-28")
		lease := fixture.acquire(fixture.config())
		ctx, cancel := context.WithCancel(t.Context())
		defer cancel()
		writer := fixture.start(ctx, lease, "uncertain-write", false)
		fixture.entered(writer, true)
		cancel()
		fixture.fault(writer, protocol.ErrorCodeOutcomeUnknown)
		next := fixture.acquire(fixture.config())
		if next.connection == lease.connection || fixture.opened.Load() != 2 {
			t.Fatal("uncertain write left its connection reusable")
		}
		read := fixture.start(t.Context(), next, "fresh-read", true)
		fixture.entered(read, true)
		read.finish()
		fixture.success(read)
		synctest.Wait()
		if writer.entered.Load() != 1 {
			t.Fatalf("uncertain write was replayed %d times", writer.entered.Load())
		}
	})
}
