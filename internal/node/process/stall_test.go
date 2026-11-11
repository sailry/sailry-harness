package process

import (
	"context"
	"testing"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestStallMonitor(t *testing.T) {
	t.Parallel()
	type fixture struct {
		service *Service
		clock   *clockwork.FakeClock
		stalls  chan Stalled
		output  chan string
		emitted chan struct{}
		launch  Launch
	}
	start := func(t *testing.T) fixture {
		value := fixture{clock: clockwork.NewFakeClock(), stalls: make(chan Stalled, 8), output: make(chan string), emitted: make(chan struct{})}
		service, err := New(Options{Profile: t.TempDir(), Now: value.clock.Now, After: value.clock.After,
			Stalled: func(_ context.Context, stalled Stalled) error { value.stalls <- stalled; return nil },
			Run: func(ctx context.Context, _ Launch, emit func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
				for {
					select {
					case text, open := <-value.output:
						if !open {
							return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
						}
						emit(protocol.ProcessOutputOutputStreamStdout, []byte(text))
						value.emitted <- struct{}{}
					case <-ctx.Done():
						return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "cancelled"}}, nil
					}
				}
			}})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = service.Close() })
		value.service = service
		value.launch = Launch{ID: protocol.NewRequestID(), Session: protocol.NewSessionID(), Turn: protocol.NewTurnID(), Root: t.TempDir(), Command: "fixture", Access: protocol.ProcessAccessWorkspace, TimeoutMs: 1}
		return value
	}
	// armed waits until the monitor sleeps on the clock again.
	armed := func(t *testing.T, value fixture) {
		t.Helper()
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		if err := value.clock.BlockUntilContext(ctx, 1); err != nil {
			t.Fatal("stall monitor is not waiting for the next silent period")
		}
	}
	// write returns once the service has retained the output.
	write := func(value fixture, text string) {
		value.output <- text
		<-value.emitted
	}
	stalled := func(t *testing.T, value fixture) Stalled {
		t.Helper()
		select {
		case report := <-value.stalls:
			return report
		case <-time.After(10 * time.Second):
			t.Fatal("silent command was not reported")
			return Stalled{}
		}
	}
	t.Run("each silent period of a background command is reported once", func(t *testing.T) {
		t.Parallel()
		value := start(t)
		info, err := value.service.Start(value.launch)
		if err != nil {
			t.Fatal(err)
		}
		armed(t, value)
		value.clock.Advance(StallWindow - time.Second)
		armed(t, value)
		if len(value.stalls) != 0 {
			t.Fatal("command was reported before a full silent window")
		}
		value.clock.Advance(time.Second)
		first := stalled(t, value)
		if first.Snapshot.Info.ID != info.ID || first.Snapshot.Info.Session != info.Session || first.Snapshot.Info.Status.Kind != "running" || first.Snapshot.Stdout.Text != "" || first.ElapsedMs != uint64(StallWindow.Milliseconds()) {
			t.Fatal("unexpected first stall", first)
		}
		// A reported period waits for output, however long the silence lasts.
		value.clock.Advance(4 * StallWindow)
		write(value, "resumed\n")
		armed(t, value)
		if len(value.stalls) != 0 {
			t.Fatal("one silent period was reported twice")
		}
		// Output inside a window restarts it.
		value.clock.Advance(10 * time.Minute)
		write(value, "more\n")
		value.clock.Advance(5 * time.Minute)
		armed(t, value)
		if len(value.stalls) != 0 {
			t.Fatal("output did not restart the silent window")
		}
		value.clock.Advance(10 * time.Minute)
		second := stalled(t, value)
		if second.Snapshot.Info.ID != info.ID || second.Snapshot.Stdout.Text != "resumed\nmore\n" || second.Snapshot.Info.Status.Kind != "running" {
			t.Fatal("unexpected second stall", second)
		}
		if snapshot, err := value.service.Read(info.Session, info.ID); err != nil || snapshot.Info.Status.Kind != "running" {
			t.Fatal("a stall changed the command", snapshot, err)
		}
	})
	t.Run("a finished command is never reported", func(t *testing.T) {
		t.Parallel()
		value := start(t)
		info, err := value.service.Start(value.launch)
		if err != nil {
			t.Fatal(err)
		}
		armed(t, value)
		close(value.output)
		if err := value.service.Wait(context.Background(), info.Session, info.ID, 30000, "missing"); err != nil {
			t.Fatal(err)
		}
		value.clock.Advance(2 * StallWindow)
		if err := value.service.Close(); err != nil {
			t.Fatal(err)
		}
		if len(value.stalls) != 0 {
			t.Fatal("finished command was reported as stalled")
		}
	})
	t.Run("a promoted command is watched from its promotion", func(t *testing.T) {
		t.Parallel()
		value := start(t)
		promotion := make(chan struct{})
		close(promotion)
		output, err := value.service.ExecutePromotable(context.Background(), value.launch, promotion)
		if err != nil || output.Kind != "command_started" {
			t.Fatal(output, err)
		}
		armed(t, value)
		value.clock.Advance(StallWindow)
		if report := stalled(t, value); report.Snapshot.Info.ID != value.launch.ID {
			t.Fatal("unexpected promoted stall", report)
		}
	})
}

func TestCutOutputKeepsOutcome(t *testing.T) {
	t.Parallel()
	service, err := New(Options{Profile: t.TempDir(), Run: func(_ context.Context, _ Launch, emit func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
		emit(protocol.ProcessOutputOutputStreamStdout, []byte("partial"))
		// The runner stopped reading stdout before it closed.
		return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}, Stdout: protocol.ProcessCapture{Truncated: true}}, nil
	}})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = service.Close() })
	result, err := service.Execute(context.Background(), Launch{ID: protocol.NewRequestID(), Session: protocol.NewSessionID(), Turn: protocol.NewTurnID(), Root: t.TempDir(), Command: "fixture", Access: protocol.ProcessAccessWorkspace, TimeoutMs: 1000})
	if err != nil || result.Outcome.Kind != "exited" || result.Stdout.Text != "partial" || !result.Stdout.Truncated || result.Stderr.Truncated {
		t.Fatal("cut output changed the outcome or lost its mark", result, err)
	}
}
