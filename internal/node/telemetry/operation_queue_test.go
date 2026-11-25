package telemetry

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync/atomic"
	"syscall"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const retryDSN = "http://public@reports.invalid/42"

type operationEndpoint struct {
	status atomic.Int32
	calls  chan []byte
	timers chan func()
}

func newOperationEndpoint(status int) *operationEndpoint {
	e := &operationEndpoint{calls: make(chan []byte, 100), timers: make(chan func(), 100)}
	e.status.Store(int32(status))
	return e
}

func (e *operationEndpoint) timer(_ time.Duration, fn func()) func() {
	e.timers <- fn
	return func() {}
}

func (e *operationEndpoint) RoundTrip(r *http.Request) (*http.Response, error) {
	body, err := io.ReadAll(r.Body)
	if err != nil {
		return nil, err
	}
	status := int(e.status.Load())
	e.calls <- body
	return &http.Response{StatusCode: status, Body: http.NoBody, Request: r}, nil
}

func operationControl(t *testing.T, dir, dsn string, optOut bool, settings protocol.TelemetrySettings, status int) (*Control, *operationEndpoint) {
	t.Helper()
	e := newOperationEndpoint(status)
	c := New(Options{RetryDirectory: dir, Transport: e, Timer: e.timer, Lookup: func(key string) string {
		if key == "SAILRY_SENTRY_DSN" {
			return dsn
		}
		if key == "DO_NOT_TRACK" && optOut {
			return "1"
		}
		return ""
	}})
	c.Configure(settings)
	t.Cleanup(c.Close)
	return c, e
}

func receiveOperation[T any](t *testing.T, ch <-chan T) T {
	t.Helper()
	select {
	case value := <-ch:
		return value
	case <-time.After(5 * time.Second):
		t.Fatal("timeout waiting for telemetry test event")
		var zero T
		return zero
	}
}

func recordFailure(c *Control) {
	c.Record(slog.LevelWarn, logging.Fields{"area": "warning", "operation": "breadcrumb_only"})
	c.Record(slog.LevelError, logging.Fields{"area": "command", "operation": "submit_turn", "phase": logging.Failed, "code": "unavailable", "request": "private-request-id", "detail": plantedSecret, "session": plantedName})
}

// Capture an actual failed operation. The returned bytes simulate precisely
// what a hard interruption (not a clean shutdown) could leave on disk.
func savedOperation(t *testing.T, settings protocol.TelemetrySettings) ([]byte, pendingOperation) {
	t.Helper()
	dir := t.TempDir()
	c, e := operationControl(t, dir, retryDSN, false, settings, 503)
	recordFailure(c)
	first := receiveOperation(t, e.calls)
	receiveOperation(t, e.timers)
	name := filepath.Join(dir, operationName(0))
	b, err := os.ReadFile(name)
	if err != nil {
		t.Fatal(err)
	}
	if info, err := os.Stat(name); err != nil || runtime.GOOS != "windows" && info.Mode().Perm() != 0600 {
		t.Fatalf("operation file permissions: %v, %v", info, err)
	}
	var item pendingOperation
	if err := json.Unmarshal(b, &item); err != nil {
		t.Fatal(err)
	}
	for _, id := range []string{string(item.Event), item.Fingerprint} {
		if id == "" || !bytes.Contains(first, []byte(id)) {
			t.Fatal("persisted reporting identity does not match first submission")
		}
	}
	assertClean(t, "pending operation", b)
	for _, denied := range []string{"stacktrace", "breadcrumb_only", "private-request-id", retryDSN, settings.InstallationID.String()} {
		if bytes.Contains(b, []byte(denied)) {
			t.Errorf("pending operation contains %q", denied)
		}
	}
	c.Close()
	if _, err := os.Lstat(name); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("clean close retained operation: %v", err)
	}
	return b, item
}

func TestOperationRetryRecovery(t *testing.T) {
	setBuildProduct(t, "sailry")
	settings := protocol.TelemetrySettings{InstallationID: uuid.New(), Errors: true}
	body, item := savedOperation(t, settings)
	fresh := func(t *testing.T, b []byte) (string, string) {
		t.Helper()
		dir := t.TempDir()
		name := filepath.Join(dir, operationName(0))
		if err := os.WriteFile(name, b, 0600); err != nil {
			t.Fatal(err)
		}
		return dir, name
	}
	t.Run("same consent and endpoint", func(t *testing.T) {
		dir, name := fresh(t, body)
		_, e := operationControl(t, dir, retryDSN, false, settings, 200)
		receiveOperation(t, e.timers)()
		report := receiveOperation(t, e.calls)
		for _, want := range []string{string(item.Event), item.Fingerprint, "command.submit_turn:unavailable"} {
			if !bytes.Contains(report, []byte(want)) {
				t.Fatalf("replay lacks %q", want)
			}
		}
		assertClean(t, "replayed operation", report)
		if event := productReportEvent(t, report); event.Tags["product"] != "sailry" || event.Tags["application"] != "desktop" {
			t.Fatalf("replayed operation identity = %v", event.Tags)
		}
		if _, err := os.Stat(name); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("successful replay remained: %v", err)
		}
	})
	for _, reason := range []string{"opt out", "errors off", "new identity", "new endpoint", "new product", "no endpoint", "expired", "attempts exhausted", "oversized", "corrupt"} {
		t.Run(reason, func(t *testing.T) {
			b, s, dsn := body, settings, retryDSN
			switch reason {
			case "errors off":
				s.Errors = false
			case "new identity":
				s.InstallationID = uuid.New()
			case "new endpoint":
				dsn = "http://public@other.invalid/42"
			case "new product":
				setBuildProduct(t, "another-product")
			case "no endpoint":
				dsn = " "
			case "expired", "attempts exhausted":
				copy := item
				if reason == "expired" {
					copy.At = time.Now().Add(-operationTTL)
				} else {
					copy.Retries = operationRetries
				}
				b, _ = json.Marshal(copy)
			case "oversized":
				b = append(append([]byte(nil), b...), bytes.Repeat([]byte(" "), operationBytes)...)
			case "corrupt":
				b = []byte("{")
			}
			dir, name := fresh(t, b)
			_, e := operationControl(t, dir, dsn, reason == "opt out", s, 200)
			for deadline := time.Now().Add(5 * time.Second); ; {
				if _, err := os.Lstat(name); errors.Is(err, os.ErrNotExist) {
					break
				}
				if time.Now().After(deadline) {
					t.Fatal("ineligible operation not removed")
				}
				time.Sleep(time.Millisecond)
			}
			if len(e.timers) != 0 || len(e.calls) != 0 {
				t.Fatal("ineligible operation scheduled")
			}
		})
	}
}

func TestOperationRetryStops(t *testing.T) {
	for _, end := range []string{"budget", "permanent response", "opt out", "reset identity", "close"} {
		t.Run(end, func(t *testing.T) {
			s := protocol.TelemetrySettings{InstallationID: uuid.New(), Errors: true}
			dir := t.TempDir()
			name := filepath.Join(dir, operationName(0))
			c, e := operationControl(t, dir, retryDSN, false, s, 503)
			recordFailure(c)
			receiveOperation(t, e.calls)
			fire := receiveOperation(t, e.timers)
			switch end {
			case "budget":
				for i := 0; i < operationRetries; i++ {
					if i > 0 {
						fire = receiveOperation(t, e.timers)
					}
					fire()
					receiveOperation(t, e.calls)
				}
			case "permanent response":
				e.status.Store(403)
				fire()
				receiveOperation(t, e.calls)
			case "opt out":
				s.Errors = false
				c.Configure(s)
				fire()
			case "reset identity":
				s.InstallationID = uuid.New()
				c.Configure(s)
				fire()
			case "close":
				c.Close()
				fire()
			}
			// A second, distinct failure synchronizes with the queue owner: a
			// leftover timer/budget would precede the new item's timer.
			if end == "budget" || end == "permanent response" {
				e.status.Store(503)
				recordFailure(c)
				receiveOperation(t, e.calls)
				receiveOperation(t, e.timers)
				if _, err := os.Stat(filepath.Join(dir, operationName(1))); !errors.Is(err, os.ErrNotExist) {
					t.Fatal("exhausted item occupied a slot")
				}
			} else if _, err := os.Stat(name); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("withdrawal retained item: %v", err)
			}
			if len(e.calls) != 0 {
				t.Fatal("replayed after withdrawal")
			}
		})
	}
}

func TestOperationTransientDelivery(t *testing.T) {
	for _, code := range []int{200, 301, 400, 401, 403, 404, 413, 501, 505} {
		if retryHTTP(&http.Response{StatusCode: code}) != 0 {
			t.Errorf("HTTP %d queued", code)
		}
	}
	for _, code := range []int{408, 429, 500, 502, 503, 504} {
		if retryHTTP(&http.Response{StatusCode: code}) <= 0 {
			t.Errorf("HTTP %d not retried", code)
		}
	}
	if delay := retryHTTP(&http.Response{StatusCode: 429, Header: http.Header{"Retry-After": {"1800"}}}); delay != 30*time.Minute {
		t.Fatalf("cooldown = %v", delay)
	}
	if retryNetwork(context.Canceled) || retryNetwork(errors.New("permanent configuration error")) || !retryNetwork(context.DeadlineExceeded) || !retryNetwork(syscall.ENETUNREACH) {
		t.Fatal("unexpected network retry classification")
	}
}

func TestOperationQueueSize(t *testing.T) {
	s := protocol.TelemetrySettings{InstallationID: uuid.New(), Errors: true}
	dir := t.TempDir()
	c, e := operationControl(t, dir, retryDSN, false, s, 503)
	for i := range operationSlots + 5 {
		c.Record(slog.LevelError, logging.Fields{"phase": logging.Failed, "operation": fmt.Sprintf("operation_%d", i)})
		receiveOperation(t, e.calls)
		receiveOperation(t, e.timers)
	}
	entries, err := os.ReadDir(dir)
	if err != nil || len(entries) != operationSlots {
		t.Fatalf("retained %d files: %v", len(entries), err)
	}
	for _, entry := range entries {
		info, err := entry.Info()
		if err != nil || info.Size() > operationBytes {
			t.Fatalf("oversized queue file: %v, %v", info, err)
		}
	}
}

func TestOperationReportConsentInFlight(t *testing.T) {
	for _, changed := range []string{"metrics", "errors", "identity"} {
		t.Run(changed, func(t *testing.T) {
			dir := t.TempDir()
			c, remote := newHeldTelemetryControl(t, dir, http.StatusServiceUnavailable)
			settings := protocol.TelemetrySettings{InstallationID: uuid.New(), Errors: true, Metrics: changed == "metrics"}
			c.Configure(settings)
			if settings.Metrics {
				receiveOperation(t, remote.timers) // Launch's usage window, not an operation retry.
			}
			fields := modelRequestFields("recovered", "model_network", 2, 0)
			c.Record(slog.LevelWarn, fields)
			request := receiveOperation(t, remote.started)

			switch changed {
			case "metrics":
				settings.Metrics = false
			case "errors":
				settings.Errors = false
			case "identity":
				settings.InstallationID = uuid.New()
			}
			c.Configure(settings)
			if changed != "metrics" {
				if !errors.Is(request.Err(), context.Canceled) {
					t.Fatalf("changing %s did not cancel the admitted error report", changed)
				}
				c.reports.Wait()
				if files, err := os.ReadDir(dir); err != nil || len(files) != 0 || len(remote.calls) != 0 || len(remote.timers) != 0 {
					t.Fatalf("changing %s retained or delivered the old report: files=%v, err=%v", changed, files, err)
				}
				return
			}
			if err := request.Err(); err != nil {
				t.Fatalf("disabling usage metrics canceled the admitted error report: %v", err)
			}
			remote.release()
			initial := productReportEvent(t, receiveOperation(t, remote.calls))
			fire := receiveOperation(t, remote.timers)
			name := filepath.Join(dir, operationName(0))
			queued, err := os.ReadFile(name)
			if err != nil {
				t.Fatal(err)
			}

			settings.Metrics = true
			c.Configure(settings)
			if body, err := os.ReadFile(name); err != nil || !bytes.Equal(body, queued) {
				t.Fatalf("enabling usage metrics changed the queued error report: %v", err)
			}
			remote.status.Store(http.StatusOK)
			fire()
			replayed := productReportEvent(t, receiveOperation(t, remote.calls))
			if replayed.EventID == "" || replayed.EventID != initial.EventID {
				t.Fatalf("replayed event ID = %q, want %q", replayed.EventID, initial.EventID)
			}
		})
	}
}

type cancelReplay struct {
	attempts           atomic.Int32
	started, cancelled chan struct{}
}

func (r *cancelReplay) RoundTrip(request *http.Request) (*http.Response, error) {
	if r.attempts.Add(1) == 1 {
		return &http.Response{StatusCode: 503, Body: http.NoBody, Request: request}, nil
	}
	close(r.started)
	<-request.Context().Done()
	close(r.cancelled)
	return nil, request.Context().Err()
}

func TestOperationRetryCancelledInFlight(t *testing.T) {
	dir := t.TempDir()
	remote := &cancelReplay{started: make(chan struct{}), cancelled: make(chan struct{})}
	timer := make(chan func(), 1)
	c := New(Options{RetryDirectory: dir, Transport: remote, Lookup: func(name string) string {
		if name == "SAILRY_SENTRY_DSN" {
			return retryDSN
		}
		return ""
	}, Timer: func(_ time.Duration, run func()) func() { timer <- run; return func() {} }})
	t.Cleanup(c.Close)
	s := protocol.TelemetrySettings{InstallationID: uuid.New(), Errors: true}
	c.Configure(s)
	recordFailure(c)
	receiveOperation(t, timer)()
	receiveOperation(t, remote.started)
	s.Errors = false
	c.Configure(s)
	select {
	case <-remote.cancelled:
	default:
		t.Fatal("opt-out returned before cancellation")
	}
	if files, err := os.ReadDir(dir); err != nil || len(files) != 0 {
		t.Fatalf("operation survived opt-out: %v, %v", files, err)
	}
}

func TestOperationQueueIgnoresSymlinks(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("creating symbolic links requires Windows privileges")
	}
	outside := t.TempDir()
	name := filepath.Join(outside, operationName(0))
	sentinel := strings.Repeat("external", 16)
	if err := os.WriteFile(name, []byte(sentinel), 0600); err != nil {
		t.Fatal(err)
	}
	for _, dirLink := range []bool{false, true} {
		dir := t.TempDir()
		if dirLink {
			dir = filepath.Join(dir, "alias")
			if err := os.Symlink(outside, dir); err != nil {
				t.Fatal(err)
			}
		} else if err := os.Symlink(name, filepath.Join(dir, operationName(0))); err != nil {
			t.Fatal(err)
		}
		c, e := operationControl(t, dir, retryDSN, false, protocol.TelemetrySettings{InstallationID: uuid.New(), Errors: true}, 503)
		recordFailure(c)
		receiveOperation(t, e.calls)
		c.Close()
		if got, err := os.ReadFile(name); err != nil || string(got) != sentinel {
			t.Fatalf("queue affected symlink target: %v", err)
		}
	}
}
