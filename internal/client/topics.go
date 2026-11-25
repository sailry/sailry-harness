package client

import (
	"context"
	"errors"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (client *Client) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	client.lifecycle.Lock()
	if client.closed {
		client.lifecycle.Unlock()
		return nil, fault(protocol.ErrorCodeUnavailable, "Client is closed")
	}
	operation, cancel := context.WithCancel(ctx)
	// Close cancels closing before it waits for lifecycle, which interrupts
	// establishment. Once registered, the topic keeps operation until its own
	// Close cancels it.
	stop := context.AfterFunc(client.closing, cancel)
	defer stop()
	client.lifecycle.Unlock()
	// Establishment waits for the Node, so lifecycle is not held across it:
	// the interface thread takes lifecycle to observe a workspace, and a slow
	// subscription must not stall a frame.
	stream, err := client.transport.Subscribe(operation, copyValue(topic))
	if err != nil {
		cancel()
		return nil, err
	}
	client.lifecycle.Lock()
	defer client.lifecycle.Unlock()
	if client.closed {
		cancel()
		return nil, errors.Join(fault(protocol.ErrorCodeUnavailable, "Client is closed"), stream.Close())
	}
	value := &topicSubscription{client: client, topic: copyValue(topic), stream: stream, ctx: operation, cancel: cancel, changed: make(chan struct{})}
	client.topicsMu.Lock()
	if client.topics == nil {
		client.topics = map[*topicSubscription]struct{}{}
	}
	client.topics[value] = struct{}{}
	client.topicsMu.Unlock()
	return value, nil
}

type topicSubscription struct {
	client      *Client
	topic       protocol.Topic
	stream      link.Subscription
	ctx         context.Context
	cancel      context.CancelFunc
	once        sync.Once
	err         error
	next        sync.Mutex
	mu          sync.Mutex
	changed     chan struct{}
	generation  uint64
	bound       uint64
	routeCancel context.CancelFunc
}

func (stream *topicSubscription) Next(ctx context.Context) (protocol.Update, error) {
	stream.next.Lock()
	defer stream.next.Unlock()
	operation, cancel := context.WithCancel(ctx)
	defer cancel()
	stop := context.AfterFunc(stream.ctx, cancel)
	defer stop()
	delay := 250 * time.Millisecond
	wake := stream.client.retried()
	for {
		stream.mu.Lock()
		current := stream.stream
		stream.mu.Unlock()
		if current != nil {
			wake = stream.client.retried()
			update, err := current.Next(operation)
			if err != nil {
				if closed := stream.ctx.Err(); closed != nil {
					return protocol.Update{}, closed
				}
				if cancelled := operation.Err(); cancelled != nil {
					return protocol.Update{}, cancelled
				}
			}
			if stream.topic.Kind == "status" || stream.topic.Kind == "connector_login" || err == nil || !transient(err) {
				return update, err
			}
			// Keep the broken stream detached even if this particular read is
			// cancelled before a replacement can be established.
			stream.mu.Lock()
			retired, cancelRoute := stream.stream, stream.routeCancel
			stream.stream, stream.routeCancel = nil, nil
			stream.mu.Unlock()
			if cancelRoute != nil {
				cancelRoute()
			}
			if retired != nil {
				_ = retired.Close()
			}
		}

		stream.mu.Lock()
		changed, generation, fresh := stream.changed, stream.generation, stream.generation != stream.bound
		stream.mu.Unlock()
		woken := false
		if !fresh && operation.Err() == nil {
			timer := time.NewTimer(delay)
			select {
			case <-operation.Done():
			case <-changed:
			case <-wake:
				woken = true
			case <-timer.C:
			}
			timer.Stop()
		}
		if err := operation.Err(); err != nil {
			return protocol.Update{}, err
		}
		if woken {
			delay = 250 * time.Millisecond
		} else {
			delay = min(delay*2, 5*time.Second)
		}

		wake = stream.client.retried()
		routeContext, cancelRoute := context.WithCancel(stream.ctx)
		stopRoute := context.AfterFunc(operation, cancelRoute)
		reconnected, err := stream.client.transport.Subscribe(routeContext, copyValue(stream.topic))
		stopRoute()
		stream.mu.Lock()
		stream.bound = generation
		cancelled := operation.Err()
		if cancelled == nil {
			cancelled = stream.ctx.Err()
		}
		if err == nil && cancelled == nil {
			stream.stream = reconnected
			stream.routeCancel = cancelRoute
			stream.mu.Unlock()
			continue
		}
		stream.mu.Unlock()
		cancelRoute()
		if err == nil {
			_ = reconnected.Close()
			return protocol.Update{}, cancelled
		}
		if cancelled != nil {
			return protocol.Update{}, cancelled
		}
		if !transient(err) {
			return protocol.Update{}, err
		}
	}
}

func (stream *topicSubscription) Close() error {
	stream.once.Do(func() {
		stream.cancel()
		stream.mu.Lock()
		current := stream.stream
		stream.stream, stream.routeCancel = nil, nil
		stream.mu.Unlock()
		if current != nil {
			stream.err = current.Close()
		}
		stream.client.topicsMu.Lock()
		delete(stream.client.topics, stream)
		stream.client.topicsMu.Unlock()
	})
	return stream.err
}

func (client *Client) resumeTopics() {
	client.topicsMu.Lock()
	defer client.topicsMu.Unlock()
	for stream := range client.topics {
		stream.mu.Lock()
		stream.generation++
		close(stream.changed)
		stream.changed = make(chan struct{})
		stream.mu.Unlock()
	}
}

func (client *Client) closeTopics() {
	client.topicsMu.Lock()
	streams := make([]*topicSubscription, 0, len(client.topics))
	for stream := range client.topics {
		streams = append(streams, stream)
	}
	client.topicsMu.Unlock()
	for _, stream := range streams {
		_ = stream.Close()
	}
}
