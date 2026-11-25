package client

import (
	"context"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A subscription waiting for the Node does not hold the Client's lifecycle,
// which the interface thread takes to observe a workspace; one that completes
// after Close is closed instead of registered.
func TestTopicEstablishmentDoesNotHoldTheClient(t *testing.T) {
	t.Parallel()
	entered, gate := make(chan struct{}), make(chan struct{})
	stream := &topicFixture{closed: make(chan struct{})}
	transport := &topicTransport{clientTransport: &clientTransport{}, subscribe: func(context.Context, protocol.Topic) (link.Subscription, error) {
		// Completes only when the test allows it, as a Node behind a slow
		// store write would.
		close(entered)
		<-gate
		return stream, nil
	}}
	controller := New(transport)
	establishing := make(chan error, 1)
	go func() {
		_, err := controller.Subscribe(t.Context(), protocol.Topic{Kind: "connections"})
		establishing <- err
	}()
	<-entered
	if !controller.lifecycle.TryLock() {
		close(gate)
		<-establishing
		_ = controller.Close()
		t.Fatal("an unfinished subscription held the Client's lifecycle")
	}
	controller.lifecycle.Unlock()
	if err := controller.Close(); err != nil {
		t.Fatal(err)
	}
	close(gate)
	if err := <-establishing; faultCode(err) != protocol.ErrorCodeUnavailable {
		t.Fatalf("subscription completed after Close = %v, want unavailable", err)
	}
	<-stream.closed
}
