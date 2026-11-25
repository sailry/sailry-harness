package telemetry

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type heldTelemetryEndpoint struct {
	*operationEndpoint
	first   atomic.Bool
	started chan context.Context
	resume  chan struct{}
	once    sync.Once
}

func newHeldTelemetryControl(t *testing.T, dir string, status int) (*Control, *heldTelemetryEndpoint) {
	t.Helper()
	remote := &heldTelemetryEndpoint{
		operationEndpoint: newOperationEndpoint(status),
		started:           make(chan context.Context, 1),
		resume:            make(chan struct{}),
	}
	values := map[string]string{"SAILRY_SENTRY_DSN": retryDSN, "SAILRY_TELEMETRY_KEY": "test-key"}
	c := New(Options{RetryDirectory: dir, Transport: remote, Timer: remote.timer, Lookup: func(key string) string { return values[key] }})
	t.Cleanup(c.Close)
	t.Cleanup(remote.release)
	return c, remote
}

func (e *heldTelemetryEndpoint) RoundTrip(request *http.Request) (*http.Response, error) {
	if e.first.CompareAndSwap(false, true) {
		ctx := request.Context()
		e.started <- ctx
		select {
		case <-e.resume:
		case <-ctx.Done():
			return nil, ctx.Err()
		}
	}
	return e.operationEndpoint.RoundTrip(request)
}

func (e *heldTelemetryEndpoint) release() { e.once.Do(func() { close(e.resume) }) }

func (e *heldTelemetryEndpoint) runTimer(t *testing.T) <-chan struct{} {
	t.Helper()
	fire := receiveOperation(t, e.timers)
	done := make(chan struct{})
	go func() { defer close(done); fire() }()
	t.Cleanup(func() { e.release(); receiveOperation(t, done) })
	return done
}

func TestInterfaceReportsUseErrorConsent(t *testing.T) {
	for _, kind := range []string{"crash", "hang"} {
		t.Run(kind, func(t *testing.T) {
			c, remote := newHeldTelemetryControl(t, "", http.StatusOK)
			settings := protocol.TelemetrySettings{InstallationID: uuid.New(), Errors: true, Metrics: true}
			c.Configure(settings)
			c.report(kind, reduce([]byte(crashTrace)))
			request := receiveOperation(t, remote.started)

			settings.Metrics = false
			c.Configure(settings)
			if err := request.Err(); err != nil {
				t.Fatalf("disabling usage metrics canceled the %s report: %v", kind, err)
			}
			settings.Errors = false
			c.Configure(settings)
			if err := request.Err(); !errors.Is(err, context.Canceled) {
				t.Fatalf("withdrawing error consent did not cancel the %s report: %v", kind, err)
			}
			c.reports.Wait()
			if len(remote.calls) != 0 {
				t.Fatalf("sent a %s report after error consent was withdrawn", kind)
			}
		})
	}
}

func TestUsageMetricsSurviveErrorConsentChanges(t *testing.T) {
	c, remote := newHeldTelemetryControl(t, "", http.StatusOK)
	settings := protocol.TelemetrySettings{InstallationID: uuid.New(), Errors: true, Metrics: true}
	c.Configure(settings)
	capture := c.Capture()
	if capture == nil {
		t.Fatal("missing usage consent capture")
	}
	settings.Errors = false
	c.Configure(settings)
	capture.Record(Turn("completed", protocol.WorkModeCode, false, nil))
	remote.runTimer(t)
	request := receiveOperation(t, remote.started)

	settings.Errors = true
	c.Configure(settings)
	if err := request.Err(); err != nil {
		t.Fatalf("enabling error reports canceled the metrics send: %v", err)
	}
	remote.release()
	sent := make(map[string]bool)
	for range 2 {
		var body batch
		if err := json.Unmarshal(receiveOperation(t, remote.calls), &body); err != nil {
			t.Fatal(err)
		}
		if len(body.Data) != 1 || body.Data[0]["count"] != float64(1) {
			t.Fatalf("invalid usage batch for %s: %v", body.Table, body.Data)
		}
		sent[body.Table] = true
	}
	if !sent["app_launches"] || !sent["turns"] {
		t.Fatalf("error consent changes lost a pending or captured count: %v", sent)
	}
}

func TestUsageMetricsCancelOnWithdrawalOrIdentityChange(t *testing.T) {
	for _, change := range []string{"metrics", "identity"} {
		t.Run(change, func(t *testing.T) {
			c, remote := newHeldTelemetryControl(t, "", http.StatusOK)
			settings := protocol.TelemetrySettings{InstallationID: uuid.New(), Metrics: true}
			c.Configure(settings)
			capture := c.Capture()
			if capture == nil {
				t.Fatal("missing usage consent capture")
			}
			done := remote.runTimer(t)
			request := receiveOperation(t, remote.started)
			if change == "metrics" {
				settings.Metrics = false
			} else {
				settings.InstallationID = uuid.New()
			}
			c.Configure(settings)
			if err := request.Err(); !errors.Is(err, context.Canceled) {
				t.Fatalf("changing %s did not cancel the metrics send: %v", change, err)
			}
			receiveOperation(t, done)
			settings.Metrics = true
			c.Configure(settings)
			capture.Record(Launch())
			if len(remote.calls) != 0 || len(remote.timers) != 0 {
				t.Fatalf("changing %s let the previous metrics consent send or record counts", change)
			}
		})
	}
}

func TestDiagnosticsFollowsInstallationConsent(t *testing.T) {
	for _, change := range []string{"switches", "identity"} {
		t.Run(change, func(t *testing.T) {
			c, remote := newHeldTelemetryControl(t, "", http.StatusOK)
			settings := protocol.TelemetrySettings{InstallationID: uuid.New()}
			c.Configure(settings)
			result, done := make(chan error, 1), make(chan struct{})
			go func() { defer close(done); result <- c.Diagnostics(t.Context()) }()
			t.Cleanup(func() { remote.release(); receiveOperation(t, done) })
			request := receiveOperation(t, remote.started)
			if change == "identity" {
				settings.InstallationID = uuid.New()
				c.Configure(settings)
				var fault *protocol.Fault
				if err := receiveOperation(t, result); !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeUnavailable {
					t.Fatalf("changing identity did not cancel diagnostics: %v", err)
				}
				return
			}
			settings.Errors, settings.Metrics = true, true
			c.Configure(settings)
			if err := request.Err(); err != nil {
				t.Fatalf("changing the automatic telemetry switches canceled diagnostics: %v", err)
			}
			remote.release()
			if err := receiveOperation(t, result); err != nil {
				t.Fatal(err)
			}
		})
	}
}
