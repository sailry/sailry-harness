package process

import (
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCommandDirectoryUsesAdmittedRoot(t *testing.T) {
	t.Parallel()
	root := t.TempDir()
	resolved, err := filepath.EvalSymlinks(root)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(filepath.Join(root, "subdir"), 0o700); err != nil {
		t.Fatal(err)
	}
	for _, cwd := range []string{"", "subdir"} {
		directory, err := commandDirectory(Launch{Root: root, Cwd: cwd, Access: protocol.ProcessAccessWorkspace})
		if want := filepath.Join(resolved, cwd); err != nil || directory != want {
			t.Errorf("command directory with cwd %q = %q, %v, want %q", cwd, directory, err, want)
		}
	}
}

func TestForegroundStdin(t *testing.T) {
	var received string
	var interactive bool
	service, err := New(Options{Profile: t.TempDir(), Run: func(_ context.Context, launch Launch, _ func(protocol.ProcessOutputOutputStream, []byte), input <-chan []byte) (protocol.ProcessCompletion, error) {
		received, interactive = launch.Stdin, input != nil
		return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
	}})
	if err != nil {
		t.Fatal(err)
	}
	defer service.Close()
	launch := Launch{ID: protocol.NewRequestID(), Root: t.TempDir(), Command: "fixture", TimeoutMs: 1000, Stdin: "{\"event\":\"fixture\"}\n"}
	if result, err := service.Execute(t.Context(), launch); err != nil || result.Outcome.Kind != "exited" || received != launch.Stdin || interactive {
		t.Fatalf("foreground runner received input %q, interactive=%v, outcome=%+v, error=%v", received, interactive, result.Outcome, err)
	}
	launch.ID, launch.Pty = protocol.NewRequestID(), true
	var fault *protocol.Fault
	if _, err := service.Start(launch); !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeInvalidRequest {
		t.Fatalf("background PTY accepted one-shot input: %v", err)
	}
}

func TestCompletedRunnerReleasesInputWhileReceiptIsPending(t *testing.T) {
	runnerDone, released, gate := make(chan struct{}), make(chan struct{}), make(chan struct{})
	service, err := New(Options{Profile: t.TempDir(), Run: func(ctx context.Context, _ Launch, emit func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
		select {
		case <-runnerDone:
		case <-ctx.Done():
		}
		emit(protocol.ProcessOutputOutputStreamStdout, []byte("done"))
		return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
	}})
	if err != nil {
		t.Fatal(err)
	}
	defer service.Close()
	launch := Launch{ID: protocol.NewRequestID(), Session: protocol.NewSessionID(), Root: t.TempDir(), Command: "fixture", TimeoutMs: 1000,
		Environment: map[string]string{"FIXTURE": "value"}, Release: func() { close(released) }, CompletionGate: gate}
	if _, err := service.Start(launch); err != nil {
		t.Fatal(err)
	}
	if _, err := service.Write(launch.Session, launch.ID, "unread", nil); err != nil {
		t.Fatal(err)
	}
	close(runnerDone)
	<-released
	if n, err := service.Write(launch.Session, launch.ID, "too late", nil); n != 0 || err == nil {
		t.Errorf("accepted input after the runner exited: %d bytes, %v", n, err)
	}
	service.mu.Lock()
	entry := service.entries[launch.ID]
	if entry.input != nil || entry.launch.Environment != nil || entry.launch.Release != nil {
		t.Errorf("stopped runner retains %d queued inputs, environment=%t, release callback=%t", len(entry.input), entry.launch.Environment != nil, entry.launch.Release != nil)
	}
	service.mu.Unlock()
	close(gate)
	<-entry.done
	snapshot, err := service.Read(launch.Session, launch.ID)
	if err != nil || snapshot.Info.Status.Kind != "finished" || snapshot.Stdout.Text != "done" {
		t.Fatalf("completed command no longer readable: %+v, %v", snapshot, err)
	}
}

func TestStopTransientKeepsServices(t *testing.T) {
	reports := processFailureReports(t)
	stopped := make(chan string, 2)
	service, err := New(Options{Profile: t.TempDir(), Run: func(ctx context.Context, launch Launch, _ func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
		<-ctx.Done()
		stopped <- launch.Command
		return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "cancelled"}}, nil
	}})
	if err != nil {
		t.Fatal(err)
	}
	defer service.Close()
	session := protocol.NewSessionID()
	for _, launch := range []Launch{{Command: "tests"}, {Command: "server", Service: true}} {
		launch.ID, launch.Session, launch.Root, launch.TimeoutMs = protocol.NewRequestID(), session, t.TempDir(), 1000
		if _, err := service.Start(launch); err != nil {
			t.Fatal(err)
		}
	}
	if err := (Launch{ID: protocol.NewRequestID(), Root: t.TempDir(), Command: "server", TimeoutMs: 1000, Service: true}).Validate(); err == nil {
		t.Fatal("a foreground service launch was accepted")
	}
	service.StopTransient(session)
	if command := <-stopped; command != "tests" {
		t.Fatalf("StopTransient stopped %s", command)
	}
	running := 0
	for _, info := range service.List(session) {
		if info.Status.Kind == "running" {
			if info.Command != "server" {
				t.Fatalf("%s is still running", info.Command)
			}
			running++
		}
	}
	if running != 1 {
		t.Fatalf("the service is not the only running command: %+v", service.List(session))
	}
	if err := service.Close(); err != nil {
		t.Fatal(err)
	}
	checkProcessFailureReport(t, reports(), Launch{}, "", "")
}

func TestSandboxedControlFollowsTheLiveProcess(t *testing.T) {
	if !SandboxSupported() {
		t.Skip("workspace sandbox is unavailable on this platform")
	}
	session, id := protocol.NewSessionID(), protocol.NewRequestID()
	info := protocol.ProcessInfo{ID: id, Session: session, Access: protocol.ProcessAccessWorkspace, Status: protocol.ProcessStatus{Kind: "running"}}
	service := &Service{options: Options{Now: time.Now}, entries: map[protocol.RequestID]*commandEntry{id: {info: info}}}
	target := service.SandboxedControl(session, id)
	if target == nil || service.SandboxedControl(protocol.NewSessionID(), id) != nil {
		t.Fatal("sandbox control lost the process's session binding")
	}
	info.Access = protocol.ProcessAccessUnrestricted
	service.entries[id] = &commandEntry{info: info}
	if service.SandboxedControl(session, id) != nil {
		t.Fatal("unrestricted command supplied sandbox evidence")
	}
	for _, err := range []error{
		func() error { _, err := service.Stop(session, id, target); return err }(),
		func() error { _, err := service.Write(session, id, "input", target); return err }(),
	} {
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeConflict {
			t.Fatalf("old sandbox approval controlled a new process: %v", err)
		}
	}
}

func TestOutputBudget(t *testing.T) {
	t.Parallel()
	t.Run("retained tool previews freeze ranges and background head", func(t *testing.T) {
		session, id := protocol.NewSessionID(), protocol.NewRequestID()
		budget := &outputBudget{limit: 2 * streamLimit}
		stdout, stderr := newRetained(t.TempDir(), budget, false, false), newRetained(t.TempDir(), budget, false, false)
		defer stdout.evict()
		defer stderr.evict()
		service := &Service{options: Options{Now: time.Now}, entries: map[protocol.RequestID]*commandEntry{id: {info: protocol.ProcessInfo{ID: id, Session: session}, stdout: stdout, stderr: stderr}}}
		stdout.append([]byte("small"))
		preview, err := service.Preview(session, id, false)
		if err != nil || len(preview) != 1 || preview["id"] != id {
			t.Fatal(preview, err)
		}
		stdout.append(bytes.Repeat([]byte{'x'}, streamLimit+prefixLimit))
		stdout.append([]byte("last failure\n"))
		preview, err = service.Preview(session, id, false)
		if err != nil {
			t.Fatal(err)
		}
		metadata, ok := preview["stdout"].(map[string]any)
		if !ok || preview["stderr"] != nil || metadata["total_bytes"] != stdout.total || metadata["retained_bytes"] != streamLimit || metadata["transcoded"] != false || metadata["invalid_utf8"] != false || !strings.HasSuffix(metadata["tail"].(string), "last failure\n") {
			t.Fatal(preview)
		}
		if _, present := metadata["head"]; present {
			t.Fatal("foreground preview duplicated its canonical head")
		}
		page, err := service.Page(session, id, protocol.ProcessOutputOutputStreamStdout, prefixLimit-1, 32)
		if err != nil || page.NextOffset == nil || metadata["retained_head_bytes"] != uint64(prefixLimit) || metadata["retained_tail_offset"] != *page.NextOffset || metadata["retained_end"] != stdout.total {
			t.Fatal(page, metadata, err)
		}
		background, err := service.Preview(session, id, true)
		if err != nil {
			t.Fatal(err)
		}
		head := background["stdout"].(map[string]any)
		if head["head_bytes"] != recentLimit || head["head_invalid_utf8"] != false || !strings.HasPrefix(head["head"].(string), "small") || len(head["head"].(string)) != recentLimit {
			t.Fatal(head)
		}
		frozen := metadata["tail"].(string)
		stdout.append([]byte("new output"))
		if metadata["tail"] != frozen {
			t.Fatal("later output changed the canonical preview")
		}
		if _, err := service.Preview(protocol.NewSessionID(), id, true); err == nil {
			t.Fatal("another session read command preview metadata")
		}
	})
	t.Run("prefix ring shared reservation and eviction", func(t *testing.T) {
		budget := &outputBudget{limit: streamLimit}
		stream := newRetained(t.TempDir(), budget, false, false)
		defer stream.evict()
		value := append(bytes.Repeat([]byte{'a'}, prefixLimit), bytes.Repeat([]byte{'b'}, streamLimit)...)
		value = append(value, []byte("end")...)
		stream.append(value)
		if budget.available() != 0 || stream.bytes() != streamLimit {
			t.Fatal("output exceeded its shared reservation")
		}
		second := newRetained(t.TempDir(), budget, true, false)
		defer second.evict()
		second.append([]byte("fallback"))
		second.finish()
		if second.bytes() != 0 || second.snapshot(false).Text != "fallback" {
			t.Fatal("capture must survive exhausted retention")
		}
		info := protocol.ProcessInfo{}
		page, err := stream.page(info, protocol.ProcessOutputOutputStreamStdout, prefixLimit-3, 32)
		if err != nil || page.Text != "aaa" || page.NextOffset == nil || *page.NextOffset <= prefixLimit || !page.Truncated {
			t.Fatal(page, err)
		}
		tail, err := stream.page(info, protocol.ProcessOutputOutputStreamStdout, stream.total-3, 32)
		if err != nil || tail.Text != "end" {
			t.Fatal(tail, err)
		}
		stream.finish()
		capture := stream.snapshot(false)
		if len(capture.Text) != captureLimit || !capture.Truncated {
			t.Fatal("foreground prefix capture changed")
		}
		stream.evict()
		if budget.available() != streamLimit {
			t.Fatal("eviction leaked output reservation")
		}
		if _, err := stream.page(info, protocol.ProcessOutputOutputStreamStdout, 0, 1); err == nil {
			t.Fatal("evicted output remained available")
		}
	})
	t.Run("UTF-8 splits invalid sequences and complete lines", func(t *testing.T) {
		budget := &outputBudget{limit: streamLimit}
		stream := newRetained(t.TempDir(), budget, true, false)
		defer stream.evict()
		stream.append([]byte{0xe4})
		if stream.snapshot(false).Text != "" {
			t.Fatal("partial UTF-8 escaped")
		}
		stream.append([]byte{0xb8, 0xad})
		if stream.snapshot(false).Text != "中" {
			t.Fatal("split UTF-8 changed")
		}
		if _, err := stream.page(protocol.ProcessInfo{}, protocol.ProcessOutputOutputStreamStdout, 1, 32); err == nil {
			t.Fatal("split offset accepted")
		}
		stream.append([]byte("\nLocal: \x1b[36mhttp://localhost:5173/app\x1b[0m"))
		if len(stream.services()) != 0 {
			t.Fatal("partial service line published")
		}
		stream.append([]byte("\nhttp://0.0.0.0:5173/ http://127.0.0.1:8000/\n"))
		if services := stream.services(); len(services) != 2 || services[0].Url != "http://127.0.0.1:5173/app" || services[1].Port != 8000 {
			t.Fatal(services)
		}
		stream.append([]byte{0xff, 0xfe, 0xe4})
		stream.finish()
		capture := stream.snapshot(false)
		if !capture.InvalidUtf8 || !strings.HasSuffix(capture.Text, "���") || !utf8.ValidString(capture.Text) {
			t.Fatal(capture)
		}
	})
	t.Run("tail capture and unused reservation", func(t *testing.T) {
		budget := &outputBudget{limit: streamLimit}
		stream := newRetained(t.TempDir(), budget, true, false)
		defer stream.evict()
		stream.append([]byte(strings.Repeat("x", captureLimit) + "last"))
		stream.finish()
		capture := stream.snapshot(false)
		if len(capture.Text) != captureLimit || !strings.HasSuffix(capture.Text, "last") || !capture.Truncated {
			t.Fatal(capture)
		}
		if budget.used.Load() != int64(captureLimit+4) {
			t.Fatal("unused output reservation was not returned")
		}
	})
}

func TestBackgroundCompletionEvidence(t *testing.T) {
	t.Parallel()
	t.Run("promotion keeps one process and gates durable completion", func(t *testing.T) {
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		started, runDone := make(chan struct{}), make(chan struct{})
		gate, entered, persisted := make(chan struct{}), make(chan struct{}), make(chan struct{})
		var runOnce, gateOnce, persistOnce sync.Once
		launches := 0
		service, err := New(Options{Profile: t.TempDir(), After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }, Run: func(ctx context.Context, _ Launch, emit func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
			launches++
			emit(protocol.ProcessOutputOutputStreamStdout, bytes.Repeat([]byte{'x'}, captureLimit))
			emit(protocol.ProcessOutputOutputStreamStdout, []byte("before promotion\n"))
			close(started)
			select {
			case <-runDone:
			case <-ctx.Done():
				t.Error("promoted process inherited caller cancellation")
			}
			emit(protocol.ProcessOutputOutputStreamStdout, []byte("promoted output\n"))
			emit(protocol.ProcessOutputOutputStreamStderr, bytes.Repeat([]byte{'x'}, streamLimit+100))
			emit(protocol.ProcessOutputOutputStreamStderr, []byte("final failure\n"))
			return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
		}, Finished: func(_ context.Context, finished Finished) error {
			close(entered)
			<-persisted
			stdout, stderr := finished.Snapshot.Stdout, finished.Snapshot.Stderr
			if !strings.HasSuffix(stdout.Text, "before promotion\npromoted output\n") || len(stdout.Text) != captureLimit || !stdout.Truncated {
				t.Error("promotion lost the already received tail")
			}
			if !strings.HasSuffix(stderr.Text, "final failure\n") || len(stderr.Text) != captureLimit || !stderr.Truncated {
				t.Error("completion persisted the beginning of stderr")
			}
			if finished.StdoutBytes == nil || *finished.StdoutBytes != uint64(captureLimit+len("before promotion\npromoted output\n")) ||
				finished.StderrBytes == nil || *finished.StderrBytes != uint64(streamLimit+100+len("final failure\n")) {
				t.Error("completion lost exact stream lengths")
			}
			return nil
		}})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() {
			runOnce.Do(func() { close(runDone) })
			gateOnce.Do(func() { close(gate) })
			persistOnce.Do(func() { close(persisted) })
			_ = service.Close()
		})
		launch := Launch{ID: protocol.NewRequestID(), Session: protocol.NewSessionID(), Turn: protocol.NewTurnID(), Root: t.TempDir(), Command: "fixture", Access: protocol.ProcessAccessWorkspace, TimeoutMs: 1, CompletionGate: gate}
		promotion := make(chan struct{})
		type result struct {
			output protocol.Output
			err    error
		}
		returned := make(chan result, 1)
		go func() {
			output, _, err := service.ExecutePromotable(ctx, launch, promotion)
			returned <- result{output, err}
		}()
		<-started
		if len(service.List(launch.Session)) != 0 {
			t.Fatal("foreground command appeared as background before promotion")
		}
		close(promotion)
		got := <-returned
		if got.err != nil || got.output.Kind != "command_started" || len(service.List(launch.Session)) != 1 {
			t.Fatal(got.output, got.err)
		}
		cancel()
		runOnce.Do(func() { close(runDone) })
		select {
		case <-entered:
			t.Fatal("completion callback passed a closed admission gate")
		default:
		}
		gateOnce.Do(func() { close(gate) })
		<-entered
		snapshot, err := service.Read(launch.Session, launch.ID)
		if err != nil || snapshot.Info.Status.Kind != "running" {
			t.Fatal("promoted completion published before durable evidence", snapshot, err)
		}
		persistOnce.Do(func() { close(persisted) })
		if _, err := service.Wait(context.Background(), launch.Session, launch.ID, 30000, "missing"); err != nil {
			t.Fatal(err)
		}
		if launches != 1 {
			t.Fatal("promotion relaunched the native process", launches)
		}
	})
	for _, uncertain := range []bool{false, true} {
		t.Run(map[bool]string{false: "persist before publication", true: "unknown persistence never relaunches"}[uncertain], func(t *testing.T) {
			ctx := context.Background()
			runDone, entered, release := make(chan struct{}), make(chan struct{}), make(chan struct{})
			var runOnce, releaseOnce sync.Once
			launches := 0
			now := time.Unix(100, 0)
			service, err := New(Options{Profile: t.TempDir(), Now: func() time.Time { return now }, After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }, Run: func(ctx context.Context, _ Launch, emit func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
				launches++
				emit(protocol.ProcessOutputOutputStreamStdout, []byte("done"))
				select {
				case <-runDone:
				case <-ctx.Done():
				}
				return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}, ElapsedMs: 7}, nil
			}, Finished: func(_ context.Context, finished Finished) error {
				close(entered)
				<-release
				if finished.Snapshot.Stdout.Text != "done" || finished.ElapsedMs == nil || *finished.ElapsedMs != 7 {
					t.Error("completion evidence lost output or timing")
				}
				if uncertain {
					return errors.New("fixture writer failed")
				}
				return nil
			}})
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() {
				runOnce.Do(func() { close(runDone) })
				releaseOnce.Do(func() { close(release) })
				_ = service.Close()
			})
			launch := Launch{ID: protocol.NewRequestID(), Session: protocol.NewSessionID(), Turn: protocol.NewTurnID(), Root: t.TempDir(), Command: "fixture", Access: protocol.ProcessAccessWorkspace, TimeoutMs: 1}
			initial, err := service.Start(launch)
			if err != nil || initial.Status.Kind != "running" {
				t.Fatal(initial, err)
			}
			runOnce.Do(func() { close(runDone) })
			<-entered
			snapshot, err := service.Read(launch.Session, launch.ID)
			if err != nil || snapshot.Info.Status.Kind != "running" {
				t.Fatal("completion published before durable evidence", snapshot, err)
			}
			releaseOnce.Do(func() { close(release) })
			if _, err := service.Wait(ctx, launch.Session, launch.ID, 30000, "missing"); err != nil {
				t.Fatal(err)
			}
			snapshot, err = service.Read(launch.Session, launch.ID)
			want := "finished"
			if uncertain {
				want = "failed"
			}
			if err != nil || snapshot.Info.Status.Kind != want || launches != 1 {
				t.Fatal(snapshot, err, launches)
			}
			if _, err := service.Start(launch); err == nil {
				t.Fatal("command identity was replayed")
			}
		})
	}
}

func TestBackgroundCompletionRetriesUncommittedWrite(t *testing.T) {
	finishes := make(chan Finished, 2)
	retry := make(chan time.Time, 1)
	armed := make(chan struct{})
	attempts, launches := 0, 0
	service, err := New(Options{Profile: t.TempDir(), After: func(delay time.Duration) <-chan time.Time {
		if delay == time.Second {
			close(armed)
			return retry
		}
		return make(chan time.Time)
	}, Run: func(_ context.Context, _ Launch, emit func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
		launches++
		emit(protocol.ProcessOutputOutputStreamStdout, []byte("completed once"))
		return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
	}, Finished: func(_ context.Context, finished Finished) error {
		attempts++
		finishes <- finished
		if attempts == 1 {
			return &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "transaction did not commit"}
		}
		return nil
	}})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = service.Close() })
	launch := Launch{ID: protocol.NewRequestID(), Session: protocol.NewSessionID(), Turn: protocol.NewTurnID(), Root: t.TempDir(), Command: "fixture", TimeoutMs: 1000}
	if _, err := service.Start(launch); err != nil {
		t.Fatal(err)
	}
	<-armed
	before, err := service.Read(launch.Session, launch.ID)
	if err != nil || before.Info.Status.Kind != "running" {
		t.Fatal("failed settlement was published as durable", before, err)
	}
	retry <- time.Now()
	if _, err := service.Wait(t.Context(), launch.Session, launch.ID, 30000, "missing"); err != nil {
		t.Fatal(err)
	}
	after, err := service.Read(launch.Session, launch.ID)
	if err != nil || after.Info.Status.Kind != "finished" || launches != 1 || len(finishes) != 2 {
		t.Fatal("settlement did not recover without repeating the command", after, err, launches)
	}
	for range 2 {
		evidence := <-finishes
		if evidence.Snapshot.Info.ID != launch.ID || evidence.Snapshot.Stdout.Text != "completed once" {
			t.Fatal("settlement retried different evidence", evidence)
		}
	}
}

func TestWaitReportsItsOutcome(t *testing.T) {
	lines, release, deadline := make(chan string), make(chan struct{}), make(chan time.Time)
	waitArmed := make(chan struct{}, 1)
	service, err := New(Options{Profile: t.TempDir(), After: func(delay time.Duration) <-chan time.Time {
		// Only the read deadline is scripted; output coalescing keeps real time.
		if delay == 5*time.Second {
			waitArmed <- struct{}{}
			return deadline
		}
		return time.After(delay)
	}, Run: func(ctx context.Context, _ Launch, emit func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
		for {
			select {
			case line := <-lines:
				emit(protocol.ProcessOutputOutputStreamStdout, []byte(line))
			case <-release:
				return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
			case <-ctx.Done():
				return protocol.ProcessCompletion{}, ctx.Err()
			}
		}
	}})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = service.Close() })
	launch := Launch{ID: protocol.NewRequestID(), Session: protocol.NewSessionID(), Turn: protocol.NewTurnID(), Root: t.TempDir(), Command: "fixture", TimeoutMs: 60000}
	if _, err := service.Start(launch); err != nil {
		t.Fatal(err)
	}
	if _, err := service.Wait(t.Context(), launch.Session, launch.ID, 0, "ready"); err == nil {
		t.Fatal("until without a wait was accepted")
	}
	if wait, err := service.Wait(t.Context(), launch.Session, launch.ID, 0, ""); err != nil || wait != "" {
		t.Fatal("a read without a wait reported an outcome", wait, err)
	}
	go func() { <-waitArmed; lines <- "booting\n"; lines <- "ready on :3000\n" }()
	if wait, err := service.Wait(t.Context(), launch.Session, launch.ID, 5000, "ready"); err != nil || wait != protocol.ProcessWaitMatched {
		t.Fatal(wait, err)
	}
	go func() { <-waitArmed; lines <- "request\n" }()
	if wait, err := service.Wait(t.Context(), launch.Session, launch.ID, 5000, ""); err != nil || wait != protocol.ProcessWaitOutput {
		t.Fatal(wait, err)
	}
	go func() { <-waitArmed; deadline <- time.Now() }()
	if wait, err := service.Wait(t.Context(), launch.Session, launch.ID, 5000, "missing"); err != nil || wait != protocol.ProcessWaitDeadline {
		t.Fatal(wait, err)
	}
	close(release)
	if wait, err := service.Wait(t.Context(), launch.Session, launch.ID, 5000, "missing"); err != nil || wait != protocol.ProcessWaitExited {
		t.Fatal(wait, err)
	}
}

func TestBackgroundCompletionStopsRetrying(t *testing.T) {
	for _, row := range []struct {
		name     string
		code     protocol.ErrorCode
		attempts int
		full     bool
	}{
		{"refused write", protocol.ErrorCodeBusy, 3, false},
		{"storage full", protocol.ErrorCodeBusy, 1, true},
		{"unconfirmed write", protocol.ErrorCodeOutcomeUnknown, 1, false},
		{"cancelled write", protocol.ErrorCodeCancelled, 1, false},
		{"failed launch receipt", "", 1, false},
	} {
		t.Run(row.name, func(t *testing.T) {
			reports := processFailureReports(t)
			attempts := 0
			var recovered Finished
			retry := make(chan time.Time)
			close(retry)
			service, err := New(Options{Profile: t.TempDir(), After: func(delay time.Duration) <-chan time.Time {
				if delay == time.Second {
					return retry
				}
				return time.After(delay)
			}, Run: func(_ context.Context, _ Launch, emit func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
				emit(protocol.ProcessOutputOutputStreamStdout, []byte("retained result"))
				return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(7)}, ElapsedMs: 12}, nil
			}, Finished: func(_ context.Context, finished Finished) error {
				attempts++
				if attempts > row.attempts {
					recovered = finished
					return nil
				}
				if row.code == "" {
					return ErrCompletionNotRegistered
				}
				fault := &protocol.Fault{Code: row.code, Message: "fixture completion write failed"}
				if row.full {
					return errors.Join(fault, ErrCompletionStorageFull)
				}
				return fault
			}})
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = service.Close() })
			launch := Launch{ID: protocol.NewRequestID(), Session: protocol.NewSessionID(), Turn: protocol.NewTurnID(), Root: t.TempDir(), Command: "fixture", TimeoutMs: 1000}
			if _, err := service.Start(launch); err != nil {
				t.Fatal(err)
			}
			ctx, cancel := context.WithTimeout(t.Context(), 3*time.Second)
			defer cancel()
			if _, err := service.Wait(ctx, launch.Session, launch.ID, 30000, "missing"); err != nil {
				t.Fatal("completion failure kept the process running", err)
			}
			snapshot, err := service.Read(launch.Session, launch.ID)
			message, _ := snapshot.Info.Status.Data.(string)
			if err != nil || snapshot.Info.Status.Kind != "failed" || !strings.Contains(message, "command completion could not be recorded") ||
				strings.Contains(message, "fixture completion write failed") || strings.Contains(message, "free disk space") != row.full ||
				snapshot.Stdout.Text != "retained result" || attempts != row.attempts {
				t.Fatal("completion failure was not visible with command output", snapshot, err, attempts)
			}
			service.ReconcileFinished(ctx, launch.Session, launch.ID)
			service.ReconcileFinished(ctx, launch.Session, launch.ID)
			operation, cause := "", ""
			if row.code == protocol.ErrorCodeOutcomeUnknown {
				operation, cause = "record_completion", "outcome_unknown"
			}
			checkProcessFailureReport(t, reports(), launch, operation, cause)
			if row.code != protocol.ErrorCodeBusy {
				if attempts != row.attempts {
					t.Fatal("inspection retried a non-retryable completion write")
				}
				return
			}
			snapshot, err = service.Read(launch.Session, launch.ID)
			outcome, _ := snapshot.Info.Status.Data.(protocol.ProcessOutcome)
			if err != nil || attempts != row.attempts+1 || snapshot.Info.Status.Kind != "finished" || outcome.Data != int32(7) ||
				recovered.Snapshot.Stdout.Text != "retained result" || recovered.ElapsedMs == nil || *recovered.ElapsedMs != 12 {
				t.Fatalf("inspection lost the original completion: %+v, %+v, attempts %d, error %v", snapshot, recovered, attempts, err)
			}
		})
	}
}

func TestRefusedCompletionSettlesWithoutInspection(t *testing.T) {
	for _, tc := range []struct {
		name, cause  string
		wantAttempts int
	}{
		{name: "recovers", wantAttempts: 5},
		{name: "exhausted", cause: "busy", wantAttempts: 3 + len(reconcileDelays)},
		{name: "later commit is uncertain", cause: "outcome_unknown", wantAttempts: 4},
	} {
		t.Run(tc.name, func(t *testing.T) {
			reports := processFailureReports(t)
			attempts := 0
			ticks := make(chan time.Time, len(reconcileDelays))
			retry := make(chan time.Time)
			close(retry)
			service, err := New(Options{Profile: t.TempDir(), After: func(delay time.Duration) <-chan time.Time {
				if delay == time.Second {
					return retry
				}
				if slices.Contains(reconcileDelays[:], delay) {
					return ticks
				}
				return nil
			}, Run: func(context.Context, Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error) {
				return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
			}, Finished: func(context.Context, Finished) error {
				attempts++
				switch {
				case tc.cause == "" && attempts == tc.wantAttempts:
					return nil
				case tc.cause == "outcome_unknown" && attempts == tc.wantAttempts:
					return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "fixture commit is uncertain"}
				default:
					return &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "fixture storage is busy"}
				}
			}})
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = service.Close() })
			launch := Launch{ID: protocol.NewRequestID(), Session: protocol.NewSessionID(), Turn: protocol.NewTurnID(), Root: t.TempDir(), Command: "fixture", TimeoutMs: 1000}
			if _, err := service.Start(launch); err != nil {
				t.Fatal(err)
			}
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			defer cancel()
			if wait, err := service.Wait(ctx, launch.Session, launch.ID, 5000, "missing"); err != nil || wait != protocol.ProcessWaitExited {
				t.Fatalf("completion did not reach reconciliation: %v, %v", wait, err)
			}
			checkProcessFailureReport(t, reports(), launch, "", "")
			for range reconcileDelays {
				ticks <- time.Time{}
			}
			done := make(chan struct{})
			go func() { service.workers.Wait(); close(done) }()
			select {
			case <-done:
			case <-ctx.Done():
				t.Fatal("automatic reconciliation did not stop")
			}
			kind := "failed"
			if tc.cause == "" {
				kind = "finished"
			}
			if snapshot, err := service.Read(launch.Session, launch.ID); err != nil || snapshot.Info.Status.Kind != kind || attempts != tc.wantAttempts {
				t.Fatalf("completion after %d attempts: %+v, %v", attempts, snapshot, err)
			}
			if tc.cause == "" {
				checkProcessFailureReport(t, reports(), launch, "", "")
			} else {
				checkProcessFailureReport(t, reports(), launch, "record_completion", tc.cause)
				service.ReconcileFinished(ctx, launch.Session, launch.ID)
				checkProcessFailureReport(t, reports(), launch, "record_completion", tc.cause)
				if tc.cause == "outcome_unknown" && attempts != tc.wantAttempts {
					t.Fatal("inspection replayed an uncertain completion write")
				}
			}
		})
	}
}

func TestCommandOutputMatchesAfterWaitBoundary(t *testing.T) {
	stream := newRetained(t.TempDir(), &outputBudget{}, true, false)
	defer stream.evict()
	stream.append([]byte("Compiled\nNext: Comp"))
	position := stream.position()
	if stream.matchesAfter(position, "Compiled") {
		t.Fatal("a match before the wait woke it")
	}
	stream.append([]byte("iled "))
	if !stream.matchesAfter(position, "Compiled") {
		t.Fatal("a match spanning the wait boundary was missed")
	}
	position = stream.position()
	stream.append([]byte("and running\n"))
	if stream.matchesAfter(position, "Compiled") {
		t.Fatal("unrelated new output replayed an earlier match")
	}
	stream.append([]byte(strings.Repeat("x", captureLimit) + "Compiled again"))
	if !stream.matchesAfter(position, "Compiled") {
		t.Fatal("a new match was lost after retention overflow")
	}
}

func TestEnvironmentSnapshot(t *testing.T) {
	t.Parallel()
	t.Run("workspace sandbox support matches the compiled platform", func(t *testing.T) {
		if want := runtime.GOOS == "darwin" || runtime.GOOS == "linux"; SandboxSupported() != want {
			t.Fatalf("sandbox support on %s = %v, want %v", runtime.GOOS, SandboxSupported(), want)
		}
	})
	environment := NewEnvironment([]string{"HOME=/fixture", "PATH=/usr/bin", "USER=fixture", "API_TOKEN=private", "EDITOR_KEY=private", "AWS_REGION=private", "CUSTOM=value", "HTTPS_PROXY=http://proxy.invalid", "LANG=C"})
	for _, key := range []string{"API_TOKEN", "EDITOR_KEY", "AWS_REGION"} {
		if _, ok := environment.unrestricted[key]; ok {
			t.Fatal("credential variable escaped", key)
		}
	}
	if environment.scoped["CUSTOM"] != "" || environment.unrestricted["CUSTOM"] != "value" || environment.scoped["HTTPS_PROXY"] != "http://proxy.invalid" {
		t.Fatal("captured command access changed")
	}
	if within(filepath.Join("root", "a"), filepath.Join("root", "ab")) {
		t.Fatal("workspace prefix comparison ignored path components")
	}
}

func TestForegroundTimeoutPromotes(t *testing.T) {
	t.Parallel()
	const timeoutMs = 5000
	type fixture struct {
		service *Service
		launch  Launch
		timer   chan time.Time
		started chan struct{}
		finish  chan struct{}
		// settled receives once the runner returned; gate then holds completion.
		settled chan struct{}
		gate    chan struct{}
	}
	// start holds each runner's completion on gate, or not at all when it is nil.
	start := func(t *testing.T, gate chan struct{}) fixture {
		if gate == nil {
			gate = make(chan struct{})
			close(gate)
		}
		value := fixture{timer: make(chan time.Time), started: make(chan struct{}, 64), finish: make(chan struct{}), settled: make(chan struct{}, 64), gate: gate}
		var once sync.Once
		service, err := New(Options{Profile: t.TempDir(),
			After: func(delay time.Duration) <-chan time.Time {
				if delay == timeoutMs*time.Millisecond {
					return value.timer
				}
				return make(chan time.Time)
			},
			Working: func() func() {
				return func() {
					value.settled <- struct{}{}
					<-value.gate
				}
			},
			Run: func(ctx context.Context, launch Launch, _ func(protocol.ProcessOutputOutputStream, []byte), _ <-chan []byte) (protocol.ProcessCompletion, error) {
				value.started <- struct{}{}
				select {
				case <-value.finish:
					return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}}, nil
				case <-launch.expired:
					return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "timed_out"}}, nil
				case <-ctx.Done():
					return protocol.ProcessCompletion{Outcome: protocol.ProcessOutcome{Kind: "cancelled"}}, nil
				}
			}})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() {
			once.Do(func() { close(value.finish) })
			_ = service.Close()
		})
		value.service = service
		value.launch = Launch{ID: protocol.NewRequestID(), Session: protocol.NewSessionID(), Turn: protocol.NewTurnID(), Root: t.TempDir(), Command: "fixture", Access: protocol.ProcessAccessWorkspace, TimeoutMs: timeoutMs}
		return value
	}
	type result struct {
		output   protocol.Output
		timedOut bool
		err      error
	}
	execute := func(ctx context.Context, value fixture) chan result {
		returned := make(chan result, 1)
		go func() {
			output, timedOut, err := value.service.ExecutePromotable(ctx, value.launch, nil)
			returned <- result{output, timedOut, err}
		}()
		<-value.started
		return returned
	}
	t.Run("the timeout moves the running command to the session", func(t *testing.T) {
		t.Parallel()
		value := start(t, nil)
		returned := execute(context.Background(), value)
		value.timer <- time.Now()
		got := <-returned
		if got.err != nil || got.output.Kind != "command_started" || !got.timedOut {
			t.Fatal(got)
		}
		if items := value.service.List(value.launch.Session); len(items) != 1 || items[0].Status.Kind != "running" {
			t.Fatal("timed-out command is not running in the background", items)
		}
		if _, err := value.service.Stop(value.launch.Session, value.launch.ID, nil); err != nil {
			t.Fatal(err)
		}
		if wait, err := value.service.Wait(context.Background(), value.launch.Session, value.launch.ID, 30000, ""); err != nil || wait != protocol.ProcessWaitExited {
			t.Fatal(wait, err)
		}
		snapshot, err := value.service.Read(value.launch.Session, value.launch.ID)
		if outcome, ok := snapshot.Info.Status.Data.(protocol.ProcessOutcome); err != nil || !ok || outcome.Kind != "cancelled" {
			t.Fatal("stopped command did not end", snapshot.Info.Status, err)
		}
	})
	t.Run("cancellation still stops the command", func(t *testing.T) {
		t.Parallel()
		value := start(t, nil)
		ctx, cancel := context.WithCancel(context.Background())
		returned := execute(ctx, value)
		cancel()
		got := <-returned
		completion, _ := got.output.Data.(protocol.ProcessCompletion)
		if got.err != nil || got.output.Kind != "command_result" || got.timedOut || completion.Outcome.Kind != "cancelled" {
			t.Fatal(got)
		}
	})
	t.Run("a completion racing the timeout returns its result", func(t *testing.T) {
		t.Parallel()
		value := start(t, make(chan struct{}))
		returned := execute(context.Background(), value)
		// The runner settles and holds completion until the timeout fired.
		value.finish <- struct{}{}
		<-value.settled
		value.timer <- time.Now()
		close(value.gate)
		got := <-returned
		completion, _ := got.output.Data.(protocol.ProcessCompletion)
		if got.err != nil || got.output.Kind != "command_result" || got.timedOut || completion.Outcome.Kind != "exited" {
			t.Fatal(got)
		}
		if len(value.service.List(value.launch.Session)) != 0 {
			t.Fatal("a finished command was promoted")
		}
	})
	t.Run("full background capacity stops the command at its timeout", func(t *testing.T) {
		t.Parallel()
		value := start(t, nil)
		for range 32 {
			background := value.launch
			background.ID = protocol.NewRequestID()
			if _, err := value.service.Start(background); err != nil {
				t.Fatal(err)
			}
			<-value.started
		}
		returned := execute(context.Background(), value)
		value.timer <- time.Now()
		got := <-returned
		completion, _ := got.output.Data.(protocol.ProcessCompletion)
		if got.err != nil || got.output.Kind != "command_result" || !got.timedOut || completion.Outcome.Kind != "timed_out" {
			t.Fatal(got)
		}
	})
}
