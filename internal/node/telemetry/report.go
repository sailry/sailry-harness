package telemetry

import (
	"context"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"log/slog"
	"net/http"
	"path"
	"path/filepath"
	"strings"
	"time"

	"github.com/getsentry/sentry-go"
	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"lukechampine.com/blake3"
)

const (
	module          = "github.com/sailry/sailry-harness"
	reportTimeout   = 10 * time.Second
	reportsInFlight = 32
	keptBreadcrumbs = 50
	keptFrames      = 128
)

type breadcrumb struct {
	at     time.Time
	fields logging.Fields
}

// limit is one fingerprint's reports in the current hour.
type limit struct {
	start time.Time
	count int
}

// admitLocked allows ten reports of one fingerprint an hour, over at most
// 1024 fingerprints.
func (c *Control) admitLocked(fingerprint string, now time.Time) bool {
	for key, entry := range c.limits {
		if now.Sub(entry.start) >= time.Hour {
			delete(c.limits, key)
		}
	}
	entry := c.limits[fingerprint]
	if entry == nil {
		if len(c.limits) >= 1024 {
			return false
		}
		entry = &limit{start: now}
		c.limits[fingerprint] = entry
	}
	if entry.count >= 10 {
		return false
	}
	entry.count++
	return true
}

// recorder keeps the one delivery's status.
type recorder struct {
	next   http.RoundTripper
	status int
}

func (r *recorder) RoundTrip(request *http.Request) (*http.Response, error) {
	response, err := r.next.RoundTrip(request)
	if err == nil {
		r.status = response.StatusCode
	}
	return response, err
}

// send delivers one event within ten seconds, without following redirects.
// No request URL, response body or HTTP error text reaches diagnostics.
func (c *Control) send(ctx context.Context, event *sentry.Event) bool {
	ctx, cancel := context.WithTimeout(ctx, reportTimeout)
	defer cancel()
	record := &recorder{next: c.transport}
	transport := sentry.NewHTTPSyncTransport()
	transport.Configure(sentry.ClientOptions{Dsn: c.dsn, HTTPClient: &http.Client{Transport: record, Timeout: reportTimeout,
		CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}})
	transport.SendEventWithContext(ctx, event)
	return record.status >= 200 && record.status <= 299
}

// stack is the reporting goroutine's own frames inside this module: package,
// function, module-relative file and line. Paths, source and variables of
// the machine never enter it.
func stack(trace *sentry.Stacktrace) *sentry.Stacktrace {
	if trace == nil {
		return nil
	}
	frames := trace.Frames
	if len(frames) > keptFrames {
		frames = frames[len(frames)-keptFrames:]
	}
	safe := &sentry.Stacktrace{Frames: []sentry.Frame{}}
	for _, frame := range frames {
		kept := sentry.Frame{Lineno: frame.Lineno}
		if symbol(frame.Module) {
			kept.Module = frame.Module
		}
		if symbol(frame.Function) {
			kept.Function = frame.Function
		}
		if relative, inside := strings.CutPrefix(frame.Module, module); inside && (relative == "" || relative[0] == '/') && frame.AbsPath != "" {
			if name := path.Join(strings.TrimPrefix(relative, "/"), filepath.Base(frame.AbsPath)); len(name) <= 512 && !strings.ContainsAny(name, "\\:") {
				kept.Filename = name
			}
		}
		safe.Frames = append(safe.Frames, kept)
	}
	return safe
}

func symbol(value string) bool {
	if value == "" || len(value) > 1024 {
		return false
	}
	for _, b := range []byte(value) {
		if !(b >= 'a' && b <= 'z' || b >= 'A' && b <= 'Z' || b >= '0' && b <= '9' || strings.IndexByte(" _.:$<>{},()&;[]?!'*+-/", b) >= 0) {
			return false
		}
	}
	return true
}

// event is the one shape every report has. kind is "diagnostic" for the
// report a controller asked for, "operation" for a failed operation, "hang"
// for an interface thread that stopped answering and "crash" for the fatal
// panic that ended the previous run.
// Only fields that pass logging.Remote are included.
func (c *Control) eventLocked(kind string, fields logging.Fields, trace *sentry.Stacktrace) *sentry.Event {
	text := func(key, fallback string) string {
		if value, found := fields[key].(string); found {
			return value
		}
		return fallback
	}
	message, level := "runtime.diagnostics:requested", sentry.LevelInfo
	if kind != "diagnostic" {
		message, level = text("area", "runtime")+"."+text("operation", "operation")+":"+text("code", "internal"), sentry.LevelError
	}
	if kind == "crash" {
		level = sentry.LevelFatal
	}
	hash := blake3.New(32, nil)
	hash.Write([]byte(kind))
	for _, key := range []string{"area", "code", "operation", "stage", "reason", "cause_code", "package", "plugin_version"} {
		if value, found := fields[key]; found {
			encoded, _ := json.Marshal(value)
			hash.Write([]byte(key))
			hash.Write(encoded)
		}
	}
	if trace != nil {
		for _, frame := range trace.Frames {
			hash.Write([]byte(frame.Module + frame.Function + frame.Filename))
			hash.Write(binary.LittleEndian.AppendUint64(nil, uint64(frame.Lineno)))
		}
	}
	event := &sentry.Event{
		EventID:     sentry.EventID(strings.ReplaceAll(uuid.NewString(), "-", "")),
		Timestamp:   time.Now().UTC(),
		Level:       level,
		Logger:      "sailry",
		Platform:    "native",
		Message:     message,
		Release:     c.release,
		Environment: c.channel,
		Fingerprint: []string{hex.EncodeToString(hash.Sum(nil))},
		Tags:        c.tagsLocked(),
		Sdk:         sentry.SdkInfo{Name: "Sailry", Version: "1"},
	}
	if len(fields) > 0 {
		// sentry-go's event has no free "extra" object; the public fields
		// travel as one context.
		event.Contexts = map[string]sentry.Context{"sailry": sentry.Context(fields)}
	}
	if kind != "diagnostic" {
		handled, mechanism := kind == "operation", "tracing"
		if !handled {
			mechanism = kind
		}
		event.Exception = []sentry.Exception{{Type: "SailryError", Value: message, Stacktrace: trace, Mechanism: &sentry.Mechanism{Type: mechanism, Handled: &handled}}}
		for _, crumb := range c.breadcrumbs {
			event.Breadcrumbs = append(event.Breadcrumbs, &sentry.Breadcrumb{Type: "log", Category: "sailry", Level: sentry.LevelWarning, Timestamp: crumb.at, Data: map[string]any(crumb.fields)})
		}
	}
	return event
}

// Record receives the log's warnings and errors. While error reports are
// on, a warning is kept for the next report and a failed operation is
// reported, at most ten times an hour for one fingerprint. While they are
// off it does nothing.
func (c *Control) Record(level slog.Level, fields logging.Fields) {
	c.mu.Lock()
	if !c.errorsLocked() || c.closing {
		c.mu.Unlock()
		return
	}
	safe := logging.Remote(fields)
	if level < slog.LevelError {
		if len(safe) > 0 {
			c.breadcrumbs = append(c.breadcrumbs, breadcrumb{at: time.Now().UTC(), fields: safe})
			if len(c.breadcrumbs) > keptBreadcrumbs {
				c.breadcrumbs = c.breadcrumbs[len(c.breadcrumbs)-keptBreadcrumbs:]
			}
		}
		c.mu.Unlock()
		return
	}
	if phase, _ := fields["phase"].(string); phase != logging.Failed {
		c.mu.Unlock()
		return
	}
	event := c.eventLocked("operation", safe, stack(sentry.NewStacktrace()))
	if !c.admitLocked(event.Fingerprint[0], time.Now()) || c.inFlight >= reportsInFlight {
		c.mu.Unlock()
		return
	}
	c.inFlight++
	c.reports.Add(1)
	scope, epoch := c.scope, c.epoch
	c.mu.Unlock()
	go func() {
		defer c.reports.Done()
		c.mu.Lock()
		allowed := c.errorsLocked() && c.epoch == epoch
		c.mu.Unlock()
		if allowed {
			c.send(scope, event)
		}
		c.mu.Lock()
		c.inFlight--
		c.mu.Unlock()
	}()
}

// Diagnostics sends the one report a controller asked for: the anonymous
// identity, release, channel, application, system and architecture, with the
// active log's recent records reduced to their public fields. It never
// carries requests, responses, prompts or file contents. The two switches do
// not gate it: only the environment's opt-outs and a missing endpoint do.
func (c *Control) Diagnostics(ctx context.Context) error {
	c.mu.Lock()
	permitted, epoch, scope := c.permittedLocked() && c.dsn != "" && !c.closing, c.epoch, c.scope
	c.mu.Unlock()
	if !permitted {
		return unavailable()
	}
	current, err := logging.CurrentFile()
	if err != nil {
		return unavailable()
	}
	tail, err := logging.Tail(current)
	if err != nil {
		return unavailable()
	}
	c.mu.Lock()
	event := c.eventLocked("diagnostic", nil, nil)
	changed := c.epoch != epoch
	c.mu.Unlock()
	if changed {
		return unavailable()
	}
	event.Attachments = []*sentry.Attachment{{Filename: "diagnostics.jsonl", ContentType: "application/x-ndjson", Payload: tail}}
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()
	defer context.AfterFunc(scope, cancel)()
	if !c.send(ctx, event) {
		return unavailable()
	}
	return nil
}
