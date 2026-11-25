// Package telemetry owns the process's reporting consent, its anonymous
// identity, the diagnostics report a controller asks for, automatic error
// reports (failed operations, interface hangs and crashes) and anonymous
// usage counters. Nothing is sent and nothing runs in the background while
// the matching switch is off.
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
	// BuildProduct is a stable lowercase kebab-case ID, at most 64 bytes.
	// Unset or invalid values use "harness"; runtime input cannot override it.
	BuildProduct string
)

// Options names the application and replaces the process boundaries in tests.
type Options struct {
	Product     string // Compiled product entry point, never supplied by a controller.
	Version     string
	Application string              // "desktop" (default) or "host"
	Lookup      func(string) string // environment; os.Getenv by default
	Transport   http.RoundTripper   // http.DefaultTransport by default
	// RetryDirectory is the owned profile's private temporary directory.
	RetryDirectory string
	// Timer schedules usage flushes and operation retries; time.AfterFunc by default.
	Timer func(time.Duration, func()) (stop func())
	// Models are the built-in presets' static model identifiers by preset.
	// Any other model is counted as "custom".
	Models map[string][]string
	// Packages are the names of the packages the application ships. Only
	// these may be named by a feature counter.
	Packages []string
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
	installation string
	policy       sync.Mutex // serializes queue replacement and profile cleanup
	retryDir     string
	retry        *operationQueue

	identityScope  context.Context
	cancelIdentity context.CancelFunc
	errorScope     context.Context
	cancelErrors   context.CancelFunc
	metricsScope   context.Context
	cancelMetrics  context.CancelFunc
	metricsEpoch   uint64

	queueRaces  map[string]limit // one counter per allowlisted cause
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
	product     string
	channel     string
	release     string
	transport   http.RoundTripper
	timer       func(time.Duration, func()) func()
	models      map[string][]string
	packages    []string
}

// New reads the environment once. SAILRY_TELEMETRY=off and DO_NOT_TRACK=1
// opt the process out whatever the saved settings say.
func New(options Options) *Control {
	lookup := options.Lookup
	if lookup == nil {
		lookup = os.Getenv
	}
	product := options.Product
	if product == "" {
		product = BuildProduct
	}
	c := &Control{application: options.Application, product: productID(product), transport: options.Transport, timer: options.Timer, models: options.Models, packages: options.Packages, channel: "dev", limits: map[string]*limit{}}
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
	version := options.Version
	if version == "" {
		version = link.ProductVersion
	}
	c.release = "sailry@" + version + "+" + revision
	c.identityScope, c.cancelIdentity = context.WithCancel(context.Background())
	c.errorScope, c.cancelErrors = context.WithCancel(c.identityScope)
	c.metricsScope, c.cancelMetrics = context.WithCancel(c.identityScope)
	c.retryDir = options.RetryDirectory
	return c
}

func productID(value string) string {
	if !identifier(value, '-') || strings.HasSuffix(value, "-") || strings.Contains(value, "--") {
		return "harness"
	}
	return value
}

func (c *Control) advanceMetricsLocked() {
	c.metricsEpoch++
	c.cancelMetrics()
	if c.ready {
		c.metricsScope, c.cancelMetrics = context.WithCancel(c.identityScope)
	}
	c.clearWindowLocked()
}

func (c *Control) advanceErrorsLocked() {
	c.cancelErrors()
	if c.ready {
		c.errorScope, c.cancelErrors = context.WithCancel(c.identityScope)
	}
	c.breadcrumbs = nil
	clear(c.queueRaces)
}

// Configure takes the saved settings. The first one counts the launch when
// usage metrics are on.
func (c *Control) Configure(settings protocol.TelemetrySettings) {
	c.policy.Lock()
	defer c.policy.Unlock()
	c.mu.Lock()
	id := settings.InstallationID.String()
	identityChanged := !c.ready || c.installation != id
	errorsChanged := identityChanged || c.errors != settings.Errors
	metricsChanged := identityChanged || c.metrics != settings.Metrics
	rotate := !c.closing && c.retryDir != "" && errorsChanged
	previous := c.retry
	if !c.closing {
		c.ready, c.errors, c.metrics, c.installation = true, settings.Errors, settings.Metrics, id
		if identityChanged {
			c.cancelIdentity()
			c.identityScope, c.cancelIdentity = context.WithCancel(context.Background())
		}
		if errorsChanged {
			c.advanceErrorsLocked()
		}
		if metricsChanged {
			c.advanceMetricsLocked()
		}
	}
	first := !c.configured && !c.closing
	c.configured = true
	if rotate {
		c.retry = nil
		if previous != nil {
			previous.cancel()
		}
		if c.errorsLocked() {
			c.retry = newOperationQueue(c)
		}
	}
	next := c.retry
	c.mu.Unlock()
	if rotate {
		if previous != nil {
			<-previous.done
		}
		if next != nil {
			go next.run(c)
		} else if previous == nil {
			purgeOperations(c.retryDir)
		}
	}
	if first {
		c.Observe(Launch())
	}
}

// Close sends the pending usage window and waits for reports in flight, for
// two seconds at most, then withdraws consent for the rest of the process.
func (c *Control) Close() {
	c.policy.Lock()
	defer c.policy.Unlock()
	c.mu.Lock()
	if c.closing {
		c.mu.Unlock()
		return
	}
	c.closing = true
	queue := c.retry
	c.retry = nil
	if queue != nil {
		queue.cancel()
	}
	pending, epoch := c.takeWindowLocked()
	c.mu.Unlock()
	if queue != nil {
		<-queue.done
	}
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
	c.cancelIdentity()
	c.advanceErrorsLocked()
	c.advanceMetricsLocked()
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
	return map[string]string{"installation_id": c.installation, "channel": c.channel, "application": c.application, "product": c.product, "os": system, "arch": arch}
}

func unavailable() error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "diagnostic reporting is unavailable"}
}
