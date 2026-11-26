package terminal

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWorkspaceTerminalRequiresSandbox(t *testing.T) {
	service := New(t.Context(), protocol.NodeID{1}, nil)
	t.Cleanup(func() { _ = service.Close() })
	launch := Launch{
		ID: protocol.TerminalID{1}, Root: t.TempDir(), Access: protocol.ProcessAccessWorkspace,
		Command: []string{"fixture-not-started"}, Viewport: protocol.TerminalViewport{Columns: 80, Rows: 24},
	}
	if info, err := service.Start(t.Context(), launch); info.ID != (protocol.TerminalID{}) || !isTerminalFault(err, protocol.ErrorCodeNotConfigured) {
		t.Fatalf("workspace launch without a sandbox = %+v, %v", info, err)
	}
	denied := fault(protocol.ErrorCodeNotConfigured, "fixture sandbox is unavailable")
	launch.Sandbox = func(root string, command, environment []string) ([]string, []string, func(), error) {
		return nil, nil, nil, denied
	}
	if info, err := service.Start(t.Context(), launch); info.ID != (protocol.TerminalID{}) || !errors.Is(err, denied) {
		t.Fatalf("workspace launch refused by its sandbox = %+v, %v", info, err)
	}
	if _, err := service.Info(launch.ID); !isTerminalFault(err, protocol.ErrorCodeNotFound) {
		t.Fatalf("refused workspace launch appeared in the inventory: %v", err)
	}
}

func isTerminalFault(err error, code protocol.ErrorCode) bool {
	var failure *protocol.Fault
	return errors.As(err, &failure) && failure.Code == code
}

func TestTerminalPollIntervalBackoff(t *testing.T) {
	if got := terminalPollInterval(0, false); got != terminalActiveInterval {
		t.Fatalf("active local interval = %v, want %v", got, terminalActiveInterval)
	}
	if got := terminalPollInterval(29, true); got != terminalActiveInterval {
		t.Fatalf("active conn interval = %v, want %v", got, terminalActiveInterval)
	}
	if got := terminalPollInterval(30, false); got != terminalWarmInterval {
		t.Fatalf("warm interval = %v, want %v", got, terminalWarmInterval)
	}
	if got := terminalPollInterval(45, false); got != terminalCoolInterval {
		t.Fatalf("cool interval = %v, want %v", got, terminalCoolInterval)
	}
	if got := terminalPollInterval(60, false); got != terminalLocalIdle {
		t.Fatalf("local idle interval = %v, want %v", got, terminalLocalIdle)
	}
	if got := terminalPollInterval(60, true); got != terminalConnIdle {
		t.Fatalf("conn idle interval = %v, want %v", got, terminalConnIdle)
	}
}

func TestSubscriptionWakeResetsBackoff(t *testing.T) {
	service := New(t.Context(), protocol.NodeID{1}, nil)
	t.Cleanup(func() { _ = service.Close() })
	title := "initial"
	h := &handle{
		service: service,
		hasConn: true,
		streams: map[*subscription]struct{}{},
		info: protocol.TerminalInfo{
			ID:     protocol.TerminalID{2},
			Status: protocol.TerminalStatus{Kind: "running"},
			Title:  &title,
		},
		viewport: protocol.TerminalViewport{Columns: 80, Rows: 24},
	}
	stream := &subscription{
		handle:     h,
		closed:     make(chan struct{}),
		finished:   make(chan struct{}),
		wake:       make(chan struct{}, 1),
		hasConn:    true,
		emptyTicks: 100, // already backed off to terminalConnIdle (1s)
		last:       h.info,
	}
	h.streams[stream] = struct{}{}
	t.Cleanup(func() { _ = stream.Close() })

	ctx, cancel := context.WithTimeout(t.Context(), 500*time.Millisecond)
	defer cancel()
	done := make(chan protocol.Update, 1)
	errs := make(chan error, 1)
	go func() {
		update, err := stream.Next(ctx)
		if err != nil {
			errs <- err
			return
		}
		done <- update
	}()

	time.Sleep(15 * time.Millisecond)
	updated := "updated"
	h.mu.Lock()
	h.info.Title = &updated
	h.wakeStreamsLocked()
	h.mu.Unlock()

	select {
	case err := <-errs:
		t.Fatalf("Next failed before wake: %v", err)
	case update := <-done:
		frame, ok := update.Data.(protocol.TerminalFrame)
		if !ok || frame.Info.Title == nil || *frame.Info.Title != updated {
			t.Fatalf("unexpected wake update: %+v", update)
		}
	case <-time.After(250 * time.Millisecond):
		t.Fatal("wake signal did not unblock backed-off subscription immediately")
	}
}
