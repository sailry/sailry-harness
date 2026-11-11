package client

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"time"

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
	// The Node no longer waits for the slow call when its completion arrives.
	slow := protocol.BrowserCall{ID: protocol.NewRequestID(), Session: protocol.NewSessionID(), Action: protocol.BrowserAction{Kind: "wait"}}
	transport := browserTransport{clientTransport: &clientTransport{dispatch: func(request protocol.Request) (link.Admission, error) {
		if request.Command.Kind != "complete_browser" {
			return link.Admission{}, fault(protocol.ErrorCodeInvalidRequest, "browser completion expected")
		}
		switch payload := request.Command.Data.(type) {
		case protocol.CommandCompleteBrowserPayload:
			completed <- payload
			if payload.ID == slow.ID {
				return link.Admission{}, fault(protocol.ErrorCodeNotFound, "browser call is no longer pending")
			}
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
	type ending struct {
		deadline time.Time
		bounded  bool
		err      error
	}
	waited := make(chan ending, 1)
	go func() {
		served <- controller.ServeBrowser(ctx, func(ctx context.Context, call protocol.BrowserCall) protocol.BrowserResult {
			if call.ID == slow.ID {
				// A wait that outlasts its call ends with the call.
				<-ctx.Done()
				deadline, bounded := ctx.Deadline()
				waited <- ending{deadline, bounded, ctx.Err()}
				return protocol.BrowserResult{Err: &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "browser call expired"}}
			}
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
	// A call's handler runs under the call's own deadline, and the call
	// behind it is served on the same subscription once that passes.
	slow.ExpiresAtMs = uint64(time.Now().Add(50 * time.Millisecond).UnixMilli())
	calls.updates <- protocol.Update{Kind: "browser_call", Data: slow}
	select {
	case got := <-waited:
		if !got.bounded || got.deadline.UnixMilli() != int64(slow.ExpiresAtMs) || !errors.Is(got.err, context.DeadlineExceeded) {
			t.Fatalf("the slow call ended with %+v, want its deadline %d", got, slow.ExpiresAtMs)
		}
	case <-time.After(10 * time.Second):
		t.Fatal("a call's handler outlived the call's deadline")
	}
	if got := <-completed; got.ID != slow.ID || got.Result.Err == nil {
		t.Fatalf("expired completion = %+v", got)
	}
	next := protocol.BrowserCall{ID: protocol.NewRequestID(), Session: call.Session, Action: protocol.BrowserAction{Kind: "tabs"}, ExpiresAtMs: uint64(time.Now().Add(time.Minute).UnixMilli())}
	calls.updates <- protocol.Update{Kind: "browser_call", Data: next}
	select {
	case got := <-handled:
		if got.ID != next.ID {
			t.Fatalf("handled call after the expired one = %+v", got)
		}
	case <-time.After(10 * time.Second):
		t.Fatal("an expired call held the call behind it")
	}
	if got := <-completed; got.ID != next.ID {
		t.Fatalf("completion after the expired call = %+v", got)
	}
	select {
	case <-calls.closed:
		t.Fatal("an expired call's refused completion ended the subscription")
	default:
	}
	cancel()
	if err := <-served; !errors.Is(err, context.Canceled) {
		t.Fatalf("serve ended with %v", err)
	}
}
