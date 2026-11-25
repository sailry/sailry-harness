package process

import (
	"context"
	"errors"
	"log/slog"
	"maps"
	"path/filepath"
	"slices"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func processFailureReports(t *testing.T) func() []logging.Fields {
	t.Helper()
	stop, err := logging.Install(filepath.Join(t.TempDir(), "profile"), "host")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(stop)
	var mu sync.Mutex
	var reports []logging.Fields
	logging.SetReport(func(level slog.Level, fields logging.Fields) {
		if level >= slog.LevelError {
			mu.Lock()
			reports = append(reports, fields)
			mu.Unlock()
		}
	})
	t.Cleanup(func() { logging.SetReport(nil) })
	return func() []logging.Fields {
		mu.Lock()
		defer mu.Unlock()
		return slices.Clone(reports)
	}
}

func checkProcessFailureReport(t *testing.T, reports []logging.Fields, launch Launch, operation, cause string) {
	t.Helper()
	if operation == "" {
		if len(reports) != 0 {
			t.Fatalf("unexpected process reports: %v", reports)
		}
		return
	}
	want := logging.Fields{"area": "process", "operation": operation, "phase": logging.Failed, "code": "outcome_unknown", "cause_code": cause}
	if len(reports) != 1 || !maps.Equal(logging.Remote(reports[0]), want) {
		t.Fatalf("process reports: got %v, want one %v", reports, want)
	}
	want["request"], want["session"], want["turn"] = launch.ID.String(), launch.Session.String(), launch.Turn.String()
	if !maps.Equal(reports[0], want) {
		t.Fatalf("process log contains fields besides the classification and local IDs: %v", reports[0])
	}
}

func TestProcessPanicReportOwnership(t *testing.T) {
	for _, tc := range []struct {
		name      string
		mode      string
		finishErr error
		report    bool
	}{
		{name: "foreground", mode: "execute"},
		{name: "not promoted", mode: "promotable"},
		{name: "background", mode: "start", report: true},
		{name: "promoted", mode: "promote", report: true},
		{name: "one report even if persistence also failed", mode: "start", finishErr: errors.New("private persistence text"), report: true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			reports := processFailureReports(t)
			exit, promote := make(chan struct{}), make(chan struct{})
			if tc.mode != "promote" {
				close(exit)
			} else {
				close(promote)
			}
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			defer cancel()
			service, err := New(Options{Context: ctx, After: func(time.Duration) <-chan time.Time { return nil }, Run: func(ctx context.Context, _ Launch, emit func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
				select {
				case <-ctx.Done():
					return protocol.ProcessCompletion{}, ctx.Err()
				case <-exit:
				}
				emit(protocol.ProcessOutputOutputStreamStdout, []byte("private output"))
				panic("private panic payload")
			}, Finished: func(_ context.Context, finished Finished) error {
				if finished.Snapshot.Info.Status.Kind != "failed" || finished.Snapshot.Info.Status.Data != "outcome_unknown: command execution failed; side effects were not replayed" || finished.Snapshot.Stdout.Text != "private output" {
					t.Errorf("panic did not preserve safe failure evidence: %+v", finished.Snapshot)
				}
				return tc.finishErr
			}})
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = service.Close() })
			launch := Launch{ID: protocol.NewRequestID(), Session: protocol.NewSessionID(), Turn: protocol.NewTurnID(), Root: t.TempDir(), Command: "private command", TimeoutMs: 1000}
			switch tc.mode {
			case "execute":
				_, err = service.Execute(ctx, launch)
			case "promotable":
				var output protocol.Output
				output, _, err = service.ExecutePromotable(ctx, launch, promote)
				if output.Kind != "command_result" {
					t.Fatal(output.Kind)
				}
			case "start":
				if _, err := service.Start(launch); err != nil {
					t.Fatal(err)
				}
			case "promote":
				if output, _, err := service.ExecutePromotable(ctx, launch, promote); err != nil || output.Kind != "command_started" {
					t.Fatal(output, err)
				}
				close(exit)
			}
			if tc.mode == "execute" || tc.mode == "promotable" {
				if fault, ok := errors.AsType[*protocol.Fault](err); !ok || fault.Code != protocol.ErrorCodeOutcomeUnknown || fault.Message != "command execution failed; side effects were not replayed" {
					t.Fatalf("synchronous panic did not return the safe fault: %v", err)
				}
			} else if wait, err := service.Wait(ctx, launch.Session, launch.ID, 1000, "missing"); err != nil || wait != protocol.ProcessWaitExited {
				t.Fatalf("background panic did not settle: %v, %v", wait, err)
			}
			operation, cause := "", ""
			if tc.report {
				operation, cause = "run_background", "panic"
			}
			checkProcessFailureReport(t, reports(), launch, operation, cause)
		})
	}
}
