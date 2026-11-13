package client

import (
	"context"
	"errors"
	"sync"
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type topicFixture struct {
	ctx     context.Context
	first   chan struct{}
	updates chan protocol.Update
	fail    bool
	once    sync.Once
	closed  chan struct{}
}

func (stream *topicFixture) Next(ctx context.Context) (protocol.Update, error) {
	if stream.fail {
		stream.once.Do(func() { close(stream.first) })
		return protocol.Update{}, fault(protocol.ErrorCodeUnavailable, "fixture route disconnected")
	}
	select {
	case update := <-stream.updates:
		return update, nil
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	case <-stream.closed:
		return protocol.Update{}, context.Canceled
	}
}

func (stream *topicFixture) Close() error {
	if !stream.fail {
		stream.once.Do(func() { close(stream.closed) })
	}
	return nil
}

type topicTransport struct {
	*clientTransport
	topics    []*topicFixture
	subscribe func(context.Context, protocol.Topic) (link.Subscription, error)
}

func (transport *topicTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if transport.subscribe != nil {
		return transport.subscribe(ctx, topic)
	}
	if topic.Kind == "node" {
		return transport.clientTransport.Subscribe(ctx, topic)
	}
	transport.mu.Lock()
	defer transport.mu.Unlock()
	if len(transport.topics) == 0 {
		return nil, fault(protocol.ErrorCodeUnavailable, "fixture topic unavailable")
	}
	stream := transport.topics[0]
	transport.topics = transport.topics[1:]
	stream.ctx = ctx
	return stream, nil
}

func TestTopicEstablishment(t *testing.T) {
	t.Parallel()
	t.Run("Close interrupts an unfinished subscription", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			transport := &topicTransport{clientTransport: &clientTransport{}, subscribe: func(ctx context.Context, _ protocol.Topic) (link.Subscription, error) {
				<-ctx.Done()
				return nil, ctx.Err()
			}}
			controller := New(transport)
			establishing := make(chan error, 1)
			go func() {
				_, err := controller.Subscribe(ctx, protocol.Topic{Kind: "connections"})
				establishing <- err
			}()
			synctest.Wait()
			closed := make(chan error, 1)
			go func() { closed <- controller.Close() }()
			synctest.Wait()
			select {
			case err := <-closed:
				if err != nil {
					t.Error(err)
				}
			default:
				// Release the old implementation too, so the regression fails
				// without leaving blocked goroutines or waiting for a timeout.
				cancel()
				synctest.Wait()
				<-closed
				t.Error("Close waited for the subscription's caller to cancel")
			}
			if err := <-establishing; !errors.Is(err, context.Canceled) {
				t.Fatalf("unfinished subscription outlived Close: %v", err)
			}
		})
	})
	t.Run("a registered topic keeps its lifetime after establishment", func(t *testing.T) {
		stream := &topicFixture{closed: make(chan struct{})}
		transport := &topicTransport{clientTransport: &clientTransport{}, subscribe: func(ctx context.Context, _ protocol.Topic) (link.Subscription, error) {
			stream.ctx = ctx
			return stream, nil
		}}
		controller := New(transport)
		t.Cleanup(func() { _ = controller.Close() })
		if _, err := controller.Subscribe(t.Context(), protocol.Topic{Kind: "connections"}); err != nil {
			t.Fatal(err)
		}
		if stream.ctx.Err() != nil {
			t.Fatal("successful establishment cancelled the registered topic")
		}
		if err := controller.Close(); err != nil {
			t.Fatal(err)
		}
		if stream.ctx.Err() == nil {
			t.Fatal("Close retained the registered topic's lifetime")
		}
		<-stream.closed
	})
}

func TestTopicRecovery(t *testing.T) {
	t.Run("serve browser", testServeBrowser)
	t.Parallel()
	first := &topicFixture{fail: true, first: make(chan struct{})}
	second := &topicFixture{updates: make(chan protocol.Update, 1), closed: make(chan struct{})}
	transport := &topicTransport{clientTransport: &clientTransport{subscriptions: []*clientSubscription{testSubscription(projectionSnapshot(0)), testSubscription(projectionSnapshot(1))}}, topics: []*topicFixture{first, second}}
	controller := New(transport)
	t.Cleanup(func() { _ = controller.Close() })
	if err := controller.Start(context.Background()); err != nil {
		t.Fatal(err)
	}
	stream, err := controller.Subscribe(context.Background(), protocol.Topic{Kind: "connections"})
	if err != nil {
		t.Fatal(err)
	}
	done := make(chan error, 1)
	go func() {
		update, err := stream.Next(context.Background())
		if err == nil && update.Kind != "connections" {
			err = errors.New("unexpected recovered topic")
		}
		done <- err
	}()
	<-first.first
	second.updates <- protocol.Update{Kind: "connections"}
	if err := controller.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	if second.ctx.Err() != nil {
		t.Fatal("recovered subscription retained a per-read context")
	}
	go func() { _, err := stream.Next(context.Background()); done <- err }()
	if err := controller.Close(); err != nil {
		t.Fatal(err)
	}
	if err := <-done; !errors.Is(err, context.Canceled) {
		t.Fatalf("closed subscription remained active: %v", err)
	}
}
