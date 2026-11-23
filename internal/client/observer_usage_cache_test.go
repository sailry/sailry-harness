package client

import (
	"context"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type usageCacheTransport struct {
	target    protocol.NodeID
	mu        sync.Mutex
	cursor    uint64
	reads     atomic.Int32
	blockRead chan struct{}
	entered   chan struct{}
	updates   chan protocol.Update
}

func (t *usageCacheTransport) Target() protocol.NodeID { return t.target }

func (t *usageCacheTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind != "node" {
		return nil, fault(protocol.ErrorCodeInvalidRequest, "unexpected topic")
	}
	return &usageCacheSubscription{ch: t.updates, closed: make(chan struct{})}, nil
}

func (t *usageCacheTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	completion := make(chan link.Response, 1)
	if request.Command.Kind != "read_usage" {
		completion <- link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "unexpected command"}}
		close(completion)
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
	}
	query := request.Command.Data.(protocol.UsageQuery)
	go func() {
		defer close(completion)
		t.reads.Add(1)
		if t.entered != nil {
			select {
			case t.entered <- struct{}{}:
			default:
			}
		}
		if t.blockRead != nil {
			select {
			case <-t.blockRead:
			case <-ctx.Done():
				completion <- link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeCancelled}}
				return
			}
		}
		t.mu.Lock()
		cursor := t.cursor
		t.mu.Unlock()
		report := protocol.DefaultUsageReport()
		report.Node = t.target
		report.Query = query
		report.Cursor = cursor
		report.Totals.Responses = cursor
		completion <- link.Response{Output: protocol.Output{Kind: "usage", Data: report}}
	}()
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
}

func (t *usageCacheTransport) Open(context.Context, protocol.StreamID) (link.Stream, error) {
	return nil, fault(protocol.ErrorCodeUnavailable, "unused")
}

func (t *usageCacheTransport) WatchConnection(ctx context.Context, _ func(link.ConnectionInfo)) error {
	<-ctx.Done()
	return ctx.Err()
}

type usageCacheSubscription struct {
	ch     <-chan protocol.Update
	closed chan struct{}
	once   sync.Once
}

func (s *usageCacheSubscription) Next(ctx context.Context) (protocol.Update, error) {
	select {
	case update, ok := <-s.ch:
		if !ok {
			return protocol.Update{}, fault(protocol.ErrorCodeUnavailable, "closed")
		}
		return update, nil
	case <-s.closed:
		return protocol.Update{}, context.Canceled
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	}
}

func (s *usageCacheSubscription) Close() error {
	s.once.Do(func() { close(s.closed) })
	return nil
}

func TestReadUsageSingleflightAndCursorCache(t *testing.T) {
	t.Parallel()
	node := protocol.NodeID{7}
	transport := &usageCacheTransport{
		target:    node,
		cursor:    10,
		blockRead: make(chan struct{}),
		entered:   make(chan struct{}, 4),
		updates:   make(chan protocol.Update, 4),
	}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Cursor = node, 10
	transport.updates <- protocol.Update{Kind: "snapshot", Data: snapshot}

	c := New(transport)
	defer c.Close()
	if err := c.Start(t.Context()); err != nil {
		t.Fatal(err)
	}

	query := protocol.DefaultUsageQuery()
	query.StartMs, query.EndMs, query.Dimension = 1000, 2000, protocol.UsageDimensionModel

	// Two concurrent identical ReadUsage calls must coalesce into one Node RPC.
	var wg sync.WaitGroup
	reports := make([]protocol.UsageReport, 2)
	errs := make([]error, 2)
	for i := range 2 {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			reports[idx], errs[idx] = c.ReadUsage(t.Context(), query)
		}(i)
	}
	select {
	case <-transport.entered:
	case <-time.After(2 * time.Second):
		t.Fatal("first ReadUsage never reached transport")
	}
	// Give the second goroutine a brief moment to join the inflight call.
	time.Sleep(20 * time.Millisecond)
	close(transport.blockRead)
	wg.Wait()
	for i := range 2 {
		if errs[i] != nil || reports[i].Totals.Responses != 10 {
			t.Fatalf("caller %d got report=%+v err=%v", i, reports[i], errs[i])
		}
	}
	if got := transport.reads.Load(); got != 1 {
		t.Fatalf("concurrent identical ReadUsage sent %d RPCs, want 1", got)
	}

	// Re-reading at the same cursor hits the cache with zero RPCs.
	cached, err := c.ReadUsage(t.Context(), query)
	if err != nil || cached.Totals.Responses != 10 || transport.reads.Load() != 1 {
		t.Fatalf("cached ReadUsage: report=%+v err=%v reads=%d", cached, err, transport.reads.Load())
	}

	// Advancing the Node cursor invalidates the cached report.
	transport.mu.Lock()
	transport.cursor = 11
	transport.mu.Unlock()
	snapshot.Cursor = 11
	applied := make(chan struct{}, 1)
	defer c.OnChange(func() {
		if c.Snapshot().Cursor == 11 {
			select {
			case applied <- struct{}{}:
			default:
			}
		}
	})()
	transport.updates <- protocol.Update{Kind: "snapshot", Data: snapshot}
	select {
	case <-applied:
	case <-time.After(2 * time.Second):
		t.Fatal("client did not apply updated snapshot cursor")
	}

	fresh, err := c.ReadUsage(t.Context(), query)
	if err != nil || fresh.Totals.Responses != 11 || transport.reads.Load() != 2 {
		t.Fatalf("post-cursor ReadUsage: report=%+v err=%v reads=%d", fresh, err, transport.reads.Load())
	}

	// Explicit refresh bypasses the cache even when cursor has not moved.
	refreshed, err := c.readUsage(t.Context(), query, true)
	if err != nil || refreshed.Totals.Responses != 11 || transport.reads.Load() != 3 {
		t.Fatalf("forced refresh ReadUsage: report=%+v err=%v reads=%d", refreshed, err, transport.reads.Load())
	}
}
