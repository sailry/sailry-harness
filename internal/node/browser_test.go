package node

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"testing/synctest"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestBrowserCallRouting covers the Node side of a browser call: it goes to
// the controller that started it, only that controller settles it, and a call
// without an answer ends as its action allows.
func TestBrowserCallRouting(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		var registry browserRegistry
		ctx := t.Context()
		owner, other := protocol.NodeID{1}, protocol.NodeID{2}
		session := protocol.NewSessionID()
		read := protocol.BrowserAction{Kind: "read", Data: protocol.DefaultBrowserActionReadPayload()}
		back := protocol.BrowserAction{Kind: "back", Data: protocol.DefaultBrowserActionBackPayload()}
		type outcome struct {
			value json.RawMessage
			err   error
		}
		start := func(ctx context.Context, caller protocol.NodeID, id protocol.RequestID, action protocol.BrowserAction) chan outcome {
			done := make(chan outcome, 1)
			go func() {
				value, err := registry.call(ctx, caller, session, id, action)
				done <- outcome{value, err}
			}()
			return done
		}
		fault := func(err error, code protocol.ErrorCode, message string) {
			t.Helper()
			var got *protocol.Fault
			if !errors.As(err, &got) || got.Code != code || got.Message != message {
				t.Fatalf("fault = %v, want %s: %s", err, code, message)
			}
		}
		value := func(text string) protocol.BrowserResult {
			raw := json.RawMessage(text)
			return protocol.BrowserResult{Ok: &raw}
		}

		if registry.available(owner) {
			t.Fatal("browser is available before a controller subscribes")
		}
		_, err := registry.call(ctx, owner, session, protocol.NewRequestID(), read)
		fault(err, protocol.ErrorCodeUnavailable, "browser controller is disconnected")

		stream, err := registry.subscribe(owner)
		if err != nil {
			t.Fatal(err)
		}
		_, err = registry.subscribe(owner)
		fault(err, protocol.ErrorCodeConflict, "browser controller is already connected")

		// The call reaches its own controller with a deadline, and that
		// controller's completion settles it.
		id := protocol.NewRequestID()
		expires := uint64(time.Now().Add(browserCallTimeout).UnixMilli())
		done := start(ctx, owner, id, read)
		update, err := stream.Next(ctx)
		if err != nil {
			t.Fatal(err)
		}
		call, routed := update.Data.(protocol.BrowserCall)
		if update.Kind != "browser_call" || !routed || call.ID != id || call.Session != session || call.Action.Kind != "read" || call.ExpiresAtMs != expires {
			t.Fatalf("update = %+v", update)
		}
		_, err = registry.call(ctx, owner, session, id, read)
		fault(err, protocol.ErrorCodeConflict, "browser call is already pending")
		fault(registry.complete(other, id, value(`{"url":"https://example.com/"}`)), protocol.ErrorCodeUnavailable, "browser controller is disconnected")
		fault(registry.complete(owner, id, value(`"`+strings.Repeat("a", browserMaxResultBytes)+`"`)), protocol.ErrorCodeInvalidRequest, "browser result is too large")
		if err := registry.complete(owner, id, value(`{"url":"https://example.com/"}`)); err != nil {
			t.Fatal(err)
		}
		if result := <-done; result.err != nil || string(result.value) != `{"url":"https://example.com/"}` {
			t.Fatalf("result = %s, %v", result.value, result.err)
		}
		fault(registry.complete(owner, id, value(`{}`)), protocol.ErrorCodeNotFound, "browser call is no longer pending")

		// A controller's fault is the call's.
		id = protocol.NewRequestID()
		done = start(ctx, owner, id, back)
		if _, err := stream.Next(ctx); err != nil {
			t.Fatal(err)
		}
		if err := registry.complete(owner, id, protocol.BrowserResult{Err: &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "browser tab is closed"}}); err != nil {
			t.Fatal(err)
		}
		fault((<-done).err, protocol.ErrorCodeNotFound, "browser tab is closed")

		// Without an answer in 30 seconds a read may be tried again; an
		// action's outcome is unknown.
		id = protocol.NewRequestID()
		done = start(ctx, owner, id, read)
		fault((<-done).err, protocol.ErrorCodeUnavailable, "browser response unavailable; read the page before trying another action")
		fault(registry.complete(owner, id, value(`{}`)), protocol.ErrorCodeNotFound, "browser call is no longer pending")
		done = start(ctx, owner, protocol.NewRequestID(), back)
		fault((<-done).err, protocol.ErrorCodeOutcomeUnknown, "browser response unavailable; read the page before trying another action")

		// A wait may ask for longer than the call lasts. The call's deadline
		// is the bound: it ends as an unanswered read at that deadline and
		// leaves nothing pending.
		wait := protocol.BrowserAction{Kind: "wait", Data: &protocol.BrowserActionWaitPayload{
			Condition: protocol.BrowserCondition{Kind: "text", Data: &protocol.BrowserConditionTextPayload{Text: "Ready"}}, TimeoutMs: 120_000}}
		if err := validateBrowserAction(wait); err != nil {
			t.Fatal(err)
		}
		id = protocol.NewRequestID()
		began := time.Now()
		done = start(ctx, owner, id, wait)
		// The unanswered calls above are still queued for the controller,
		// which drops them by their expiry.
		for call.ID != id {
			if update, err = stream.Next(ctx); err != nil {
				t.Fatal(err)
			}
			if call = update.Data.(protocol.BrowserCall); call.ID != id && call.ExpiresAtMs > uint64(began.UnixMilli()) {
				t.Fatalf("unanswered call %s is delivered unexpired", call.ID)
			}
		}
		if call.ExpiresAtMs != uint64(began.Add(browserCallTimeout).UnixMilli()) {
			t.Fatalf("long wait expires at %d", call.ExpiresAtMs)
		}
		fault((<-done).err, protocol.ErrorCodeUnavailable, "browser response unavailable; read the page before trying another action")
		if elapsed := time.Since(began); elapsed != browserCallTimeout {
			t.Fatalf("long wait ended after %s", elapsed)
		}
		registry.mu.Lock()
		pending := len(registry.connections[owner].replies)
		registry.mu.Unlock()
		if pending != 0 {
			t.Fatalf("%d timed-out calls are still pending", pending)
		}
		fault(registry.complete(owner, id, value(`{}`)), protocol.ErrorCodeNotFound, "browser call is no longer pending")

		// An interrupted call says so.
		interrupted, cancel := context.WithCancel(ctx)
		done = start(interrupted, owner, protocol.NewRequestID(), back)
		synctest.Wait()
		cancel()
		fault((<-done).err, protocol.ErrorCodeOutcomeUnknown, "browser call interrupted; read the page before trying another action")

		// A dropped subscription ends its pending calls and frees the
		// controller's place.
		done = start(ctx, owner, protocol.NewRequestID(), back)
		synctest.Wait()
		if err := stream.Close(); err != nil {
			t.Fatal(err)
		}
		fault((<-done).err, protocol.ErrorCodeOutcomeUnknown, "browser response unavailable; read the page before trying another action")
		if registry.available(owner) {
			t.Fatal("closed subscription still owns the browser")
		}
		again, err := registry.subscribe(owner)
		if err != nil {
			t.Fatal(err)
		}
		_ = again.Close()
	})
}
