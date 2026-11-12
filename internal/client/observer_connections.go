package client

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// watchConnectionsView returns the Node's connector types and connections:
// each Next is one connections_changed payload, a full list every time, in
// the order the Node sent them. It ends on any other update or a failure;
// the subscription itself resumes after a transient loss, as every Client
// topic does. Observing never starts or repeats an authorization.
func (client *Client) watchConnectionsView(ctx context.Context) (*Observer, error) {
	observer := newObserver(ctx)
	stream, err := client.Subscribe(observer.ctx, protocol.Topic{Kind: "connections"})
	if err != nil {
		_ = observer.Close()
		return nil, err
	}
	observer.release = func() { _ = stream.Close() }
	type item struct {
		changes protocol.PluginConnectorChanges
		err     error
	}
	// One slot: the reader waits for Next to take a payload before the next.
	queue := make(chan item, 1)
	var ended error
	observer.read = func() (any, error) {
		observer.mu.Lock()
		defer observer.mu.Unlock()
		if ended == nil {
			select {
			case next := <-queue:
				if next.err == nil {
					return next.changes, nil
				}
				ended = next.err
			default:
				ended = fault(protocol.ErrorCodeUnavailable, "observation is closed")
			}
		}
		// Every later Next fails the same way.
		observer.notify()
		return nil, ended
	}
	observer.run(func() {
		for {
			var next item
			update, err := stream.Next(observer.ctx)
			switch {
			case err != nil:
				next.err = err
			case update.Kind != "connections_changed":
				next.err = fault(protocol.ErrorCodeInvalidRequest, "connection snapshot expected")
			default:
				next.changes, next.err = payload[protocol.PluginConnectorChanges](update.Data)
			}
			select {
			case <-observer.ctx.Done():
				return
			case queue <- next:
				observer.notify()
			}
			if next.err != nil {
				return
			}
		}
	})
	return observer, nil
}
