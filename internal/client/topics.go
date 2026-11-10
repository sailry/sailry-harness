package client

import (
	"context"
	"sync"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (client *Client) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	client.lifecycle.Lock()
	defer client.lifecycle.Unlock()
	if client.closed {
		return nil, fault(protocol.ErrorCodeUnavailable, "Client is closed")
	}
	operation, cancel := context.WithCancel(ctx)
	stream, err := client.transport.Subscribe(operation, copyValue(topic))
	if err != nil {
		cancel()
		return nil, err
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
	for {
		stream.mu.Lock()
		current := stream.stream
		routeCancel := stream.routeCancel
		stream.mu.Unlock()
		update, err := current.Next(operation)
		if err == nil || !transient(err) || operation.Err() != nil {
			return update, err
		}
		_ = current.Close()
		if routeCancel != nil {
			routeCancel()
		}
		for {
			stream.mu.Lock()
			changed := stream.changed
			generation := stream.generation
			fresh := generation != stream.bound
			stream.mu.Unlock()
			if !fresh {
				select {
				case <-operation.Done():
					return protocol.Update{}, operation.Err()
				case <-changed:
				}
				continue
			}
			routeContext, cancelRoute := context.WithCancel(stream.ctx)
			stopRoute := context.AfterFunc(operation, cancelRoute)
			reconnected, err := stream.client.transport.Subscribe(routeContext, copyValue(stream.topic))
			stopRoute()
			if err == nil && operation.Err() != nil {
				_ = reconnected.Close()
				err = operation.Err()
			}
			stream.mu.Lock()
			stream.bound = generation
			if err == nil && stream.ctx.Err() != nil {
				_ = reconnected.Close()
				err = stream.ctx.Err()
			}
			if err == nil {
				stream.stream = reconnected
				stream.routeCancel = cancelRoute
			}
			stream.mu.Unlock()
			if err == nil {
				break
			}
			cancelRoute()
			if !transient(err) || operation.Err() != nil {
				return protocol.Update{}, err
			}
		}
	}
}

func (stream *topicSubscription) Close() error {
	stream.once.Do(func() {
		stream.cancel()
		stream.mu.Lock()
		current := stream.stream
		stream.mu.Unlock()
		stream.err = current.Close()
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
