package client

import (
	"context"
	"encoding/json"
	"errors"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type browserTransport struct {
	*clientTransport
	calls *clientSubscription
}

func (transport browserTransport) Subscribe(_ context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind != "browser" {
		return nil, fault(protocol.ErrorCodeInvalidRequest, "browser topic expected")
	}
	return transport.calls, nil
}

// testServeBrowser covers the Client side of a browser call: the call the
// Node routes here reaches the handler, and its result goes back once with
// complete_browser under the call's ID.
func testServeBrowser(t *testing.T) {
	calls := &clientSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})}
	completed := make(chan protocol.CommandCompleteBrowserPayload, 1)
	transport := browserTransport{clientTransport: &clientTransport{dispatch: func(request protocol.Request) (link.Admission, error) {
		if request.Command.Kind != "complete_browser" {
			return link.Admission{}, fault(protocol.ErrorCodeInvalidRequest, "browser completion expected")
		}
		switch payload := request.Command.Data.(type) {
		case protocol.CommandCompleteBrowserPayload:
			completed <- payload
		case *protocol.CommandCompleteBrowserPayload:
			completed <- *payload
		}
		completion := make(chan link.Response, 1)
		completion <- link.Response{Output: protocol.Output{Kind: "browser_completed"}}
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
	}}, calls: calls}
	controller := New(transport)
	ctx, cancel := context.WithCancel(t.Context())
	call := protocol.BrowserCall{ID: protocol.NewRequestID(), Session: protocol.NewSessionID(), Action: protocol.BrowserAction{Kind: "tabs"}, ExpiresAtMs: 1}
	calls.updates <- protocol.Update{Kind: "browser_call", Data: call}
	handled := make(chan protocol.BrowserCall, 1)
	served := make(chan error, 1)
	go func() {
		served <- controller.ServeBrowser(ctx, func(_ context.Context, call protocol.BrowserCall) protocol.BrowserResult {
			handled <- call
			raw := json.RawMessage(`{"tabs":[]}`)
			return protocol.BrowserResult{Ok: &raw}
		})
	}()
	if got := <-handled; got.ID != call.ID || got.Session != call.Session || got.Action.Kind != "tabs" {
		t.Fatalf("handled call = %+v", got)
	}
	if got := <-completed; got.ID != call.ID || got.Result.Ok == nil || string(*got.Result.Ok) != `{"tabs":[]}` {
		t.Fatalf("completion = %+v", got)
	}
	cancel()
	if err := <-served; !errors.Is(err, context.Canceled) {
		t.Fatalf("serve ended with %v", err)
	}
}
