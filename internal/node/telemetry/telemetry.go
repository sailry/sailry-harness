// Package telemetry owns the process's reporting consent, its anonymous
// identity, the diagnostics report a controller asks for, automatic error
// reports and anonymous usage counters. Nothing is sent and nothing runs in
// the background while the matching switch is off.
package telemetry

import (
	"context"
	"net/http"
	"os"
	"runtime"
	"strings"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Release builds set these with -ldflags -X. A build without a revision
// reports the dev channel, as a debug build does in the reference.
var (
	BuildRevision string
	BuildDSN      string
	BuildKey      string
)

// Options names the application and replaces the process boundaries in tests.
type Options struct {
	Application string              // "desktop" (default) or "host"
	Lookup      func(string) string // environment; os.Getenv by default
	Transport   http.RoundTripper   // http.DefaultTransport by default
	// Timer schedules the usage window's flush; time.AfterFunc by default.
	Timer func(time.Duration, func()) (stop func())
	// Models are the built-in presets' static model identifiers by preset.
	// Any other model is counted as "custom".
	Models map[string][]string
}

// Control is the process's consent: the environment's opt-outs, the saved
// switches and the identity reports carry.
type Control struct {
	mu           sync.Mutex
	ready        bool
	errors       bool
	metrics      bool
	configured   bool
	closing      bool
	epoch        uint64
	scope        context.Context // ends when the epoch advances
	advance      context.CancelFunc
	installation string

	breadcrumbs []breadcrumb
	limits      map[string]*limit
	reports     sync.WaitGroup
	inFlight    int

	window *window
	stop   func()

	allowed     bool
	dsn         string
	key         string
	application string
	channel     string
	release     string
	transport   http.RoundTripper
	timer       func(time.Duration, func()) func()
	models      map[string][]string
}

// New reads the environment once. SAILRY_TELEMETRY=off and DO_NOT_TRACK=1
// opt the process out whatever the saved settings say.
func New(options Options) *Control {
	lookup := options.Lookup
	if lookup == nil {
		lookup = os.Getenv
	}
	c := &Control{application: options.Application, transport: options.Transport, timer: options.Timer, models: options.Models, channel: "dev", limits: map[string]*limit{}}
	if c.application == "" {
		c.application = "desktop"
	}
	if c.transport == nil {
		c.transport = http.DefaultTransport
	}
	if c.timer == nil {
		c.timer = func(after time.Duration, run func()) func() {
			timer := time.AfterFunc(after, run)
			return func() { timer.Stop() }
		}
	}
	value := func(name, compiled string) string {
		if text := lookup(name); text != "" {
			return strings.TrimSpace(text)
		}
		return strings.TrimSpace(compiled)
	}
	c.allowed = lookup("SAILRY_TELEMETRY") != "off" && lookup("DO_NOT_TRACK") != "1"
	c.dsn, c.key = value("SAILRY_SENTRY_DSN", BuildDSN), value("SAILRY_TELEMETRY_KEY", BuildKey)
	revision := BuildRevision
	if revision == "" {
		revision = "unknown"
	} else {
		c.channel = "beta"
	}
	c.release = "sailry@" + link.ProductVersion + "+" + revision
	c.scope, c.advance = context.WithCancel(context.Background())
	return c
}

// advanceLocked abandons everything prepared under the previous consent or
// identity: reports in flight, warnings kept for the next report and the
// usage window.
func (c *Control) advanceLocked() {
	c.epoch++
	c.advance()
	c.scope, c.advance = context.WithCancel(context.Background())
	c.breadcrumbs = nil
	c.clearWindowLocked()
}

// Configure takes the saved settings. The first one counts the launch when
// usage metrics are on.
func (c *Control) Configure(settings protocol.TelemetrySettings) {
	c.mu.Lock()
	id := settings.InstallationID.String()
	if !c.closing && !(c.ready && c.errors == settings.Errors && c.metrics == settings.Metrics && c.installation == id) {
		c.ready, c.errors, c.metrics, c.installation = true, settings.Errors, settings.Metrics, id
		c.advanceLocked()
	}
	first := !c.configured && !c.closing
	c.configured = true
	c.mu.Unlock()
	if first {
		c.Observe(Launch())
	}
}

// Close sends the pending usage window and waits for reports in flight, for
// two seconds at most, then withdraws consent for the rest of the process.
func (c *Control) Close() {
	c.mu.Lock()
	if c.closing {
		c.mu.Unlock()
		return
	}
	c.closing = true
	pending, epoch := c.takeWindowLocked()
	c.mu.Unlock()
	deadline, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	c.deliver(deadline, pending, epoch)
	done := make(chan struct{})
	go func() { c.reports.Wait(); close(done) }()
	select {
	case <-done:
	case <-deadline.Done():
	}
	c.mu.Lock()
	c.ready = false
	c.advanceLocked()
	c.mu.Unlock()
	<-done
}

func (c *Control) permittedLocked() bool { return c.allowed && c.ready }
func (c *Control) errorsLocked() bool    { return c.permittedLocked() && c.dsn != "" && c.errors }
func (c *Control) metricsLocked() bool {
	return c.permittedLocked() && c.key != "" && c.metrics && !c.closing
}

// Permitted reports whether this process may report at all.
func (c *Control) Permitted() bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.permittedLocked()
}

// Errors reports whether automatic error reports are allowed.
func (c *Control) Errors() bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.errorsLocked()
}

// Metrics reports whether usage counters are allowed.
func (c *Control) Metrics() bool {
	if c == nil {
		return false
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.metricsLocked()
}

func (c *Control) tagsLocked() map[string]string {
	// The reference's names for the system and architecture.
	system, arch := runtime.GOOS, runtime.GOARCH
	if system == "darwin" {
		system = "macos"
	}
	if name, renamed := map[string]string{"arm64": "aarch64", "amd64": "x86_64", "386": "x86"}[arch]; renamed {
		arch = name
	}
	return map[string]string{"installation_id": c.installation, "channel": c.channel, "application": c.application, "os": system, "arch": arch}
}

func unavailable() error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "diagnostic reporting is unavailable"}
}
