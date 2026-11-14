package externalbrowser

import (
	"context"
	"errors"
	"os"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// The operation completes while Done is still open. Its first Err check then
// cancels it, deterministically modeling cancellation after receiving a result.
type cancelAtResult struct {
	context.Context
	cancel context.CancelFunc
}

func (ctx cancelAtResult) Err() error {
	ctx.cancel()
	return ctx.Context.Err()
}

func TestBrowserCompletedCancellation(t *testing.T) {
	for _, action := range []protocol.ExternalBrowserAction{protocol.ExternalBrowserActionPageInfo, protocol.ExternalBrowserActionClick} {
		t.Run(string(action), func(t *testing.T) {
			fixture := newLifetimeFixture(t)
			id := protocol.NewSessionID()
			current, started, err := fixture.browsers.acquire(t.Context(), id, "fixture-browser")
			if err != nil {
				t.Fatal(err)
			}
			released := false
			defer func() {
				if !released {
					fixture.browsers.release(id, current, true)
				}
			}()
			temporary, profile := current.session.temporary, current.session.profile
			original := operations[action]
			operations[action] = func(context.Context, *session, map[string]any) (map[string]any, error) {
				return map[string]any{"completed": true}, nil
			}
			t.Cleanup(func() { operations[action] = original })
			operation, cancel := context.WithCancel(current.session.ctx)
			defer cancel()
			done := make(chan error, 1)
			go func() {
				_, err := fixture.browsers.operate(cancelAtResult{operation, cancel}, current, Call{Session: id, Action: action}, nil, started)
				done <- err
			}()
			select {
			case err := <-done:
				code := protocol.ErrorCodeCancelled
				if !action.ReadOnly() {
					code = protocol.ErrorCodeOutcomeUnknown
				}
				var failure *protocol.Fault
				if !errors.As(err, &failure) || failure.Code != code {
					t.Fatalf("interrupted result = %v, want %s", err, code)
				}
			case <-time.After(5 * time.Second):
				t.Fatal("completed cancellation waited for a second operation result")
			}
			if current.session != nil || len(fixture.browsers.Sessions()) != 0 {
				t.Fatal("interrupted session was retained")
			}
			if _, err := os.Stat(temporary); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("interrupted browser kept temporary files: %v", err)
			}
			if _, err := os.Stat(profile); err != nil {
				t.Fatalf("interruption removed the browser profile: %v", err)
			}
			fixture.browsers.release(id, current, true)
			released = true
			fixture.use(id)
			if fixture.launches() != 2 {
				t.Fatal("the next call did not replace the interrupted browser")
			}
		})
	}
}

func TestBrowserCancellationJoinsWorker(t *testing.T) {
	fixture := newLifetimeFixture(t)
	id := protocol.NewSessionID()
	current, started, err := fixture.browsers.acquire(t.Context(), id, "fixture-browser")
	if err != nil {
		t.Fatal(err)
	}
	defer fixture.browsers.release(id, current, true)
	entered, finish := make(chan struct{}), make(chan struct{})
	original := operations[protocol.ExternalBrowserActionPageInfo]
	operations[protocol.ExternalBrowserActionPageInfo] = func(ctx context.Context, _ *session, _ map[string]any) (map[string]any, error) {
		close(entered)
		<-ctx.Done()
		<-finish
		return nil, ctx.Err()
	}
	t.Cleanup(func() { operations[protocol.ExternalBrowserActionPageInfo] = original })
	operation, cancel := context.WithCancel(current.session.ctx)
	defer cancel()
	done := make(chan error, 1)
	go func() {
		_, err := fixture.browsers.operate(operation, current, Call{Session: id, Action: protocol.ExternalBrowserActionPageInfo}, nil, started)
		done <- err
	}()
	<-entered
	cancel()
	select {
	case err := <-done:
		t.Fatalf("cancellation released a running worker: %v", err)
	default:
	}
	close(finish)
	select {
	case err := <-done:
		var failure *protocol.Fault
		if !errors.As(err, &failure) || failure.Code != protocol.ErrorCodeCancelled {
			t.Fatalf("cancelled operation = %v", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("cancelled operation did not join its worker")
	}
}
