package client

import (
	"context"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ServeBrowser makes this controller the owner of its Node's browser calls:
// every call the Node routes here is handed to handle, and its result settles
// the call with complete_browser. It subscribes again a second after the
// subscription or a completion fails, and returns when ctx ends. A lost
// completion is not permission to replay a browser action.
//
// The context handle gets ends when the Node stops waiting for the call
// (ExpiresAtMs), so that one call cannot hold the calls behind it. The Node
// answers the completion of a call it gave up on with a fault, which says
// nothing about the subscription.
func (client *Client) ServeBrowser(ctx context.Context, handle func(context.Context, protocol.BrowserCall) protocol.BrowserResult) error {
	for {
		if stream, err := client.Subscribe(ctx, protocol.Topic{Kind: "browser"}); err == nil {
			for {
				update, err := stream.Next(ctx)
				if err != nil {
					break
				}
				var call protocol.BrowserCall
				switch value := update.Data.(type) {
				case protocol.BrowserCall:
					call = value
				case *protocol.BrowserCall:
					call = *value
				default:
					continue
				}
				within, cancel := context.WithDeadline(ctx, time.UnixMilli(int64(call.ExpiresAtMs)))
				result := handle(within, call)
				expired := within.Err() != nil
				cancel()
				if _, err := client.Do(ctx, protocol.Command{Kind: "complete_browser", Data: protocol.CommandCompleteBrowserPayload{ID: call.ID, Result: result}}); err != nil && !expired {
					break
				}
			}
			_ = stream.Close()
		}
		timer := time.NewTimer(time.Second)
		select {
		case <-ctx.Done():
			timer.Stop()
			return ctx.Err()
		case <-timer.C:
		}
	}
}
