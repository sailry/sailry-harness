package node

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func systemNodeFixture(t *testing.T, options Options) *Node {
	t.Helper()
	value, err := StartWithOptions(context.Background(), filepath.Join(t.TempDir(), "profile"), options)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := value.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	return value
}

// sentryFixture records every request a report endpoint receives.
type sentryFixture struct {
	mu       sync.Mutex
	requests []*http.Request
	bodies   [][]byte
	status   int
	server   *httptest.Server
}

func newSentryFixture(t *testing.T) *sentryFixture {
	fixture := &sentryFixture{status: http.StatusOK}
	fixture.server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		fixture.mu.Lock()
		fixture.requests, fixture.bodies = append(fixture.requests, r), append(fixture.bodies, body)
		status := fixture.status
		fixture.mu.Unlock()
		w.WriteHeader(status)
	}))
	t.Cleanup(fixture.server.Close)
	return fixture
}

func (f *sentryFixture) dsn() string {
	return strings.Replace(f.server.URL, "http://", "http://public@", 1) + "/42"
}

func (f *sentryFixture) count() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.requests)
}

func environment(values map[string]string) func(string) string {
	return func(key string) string { return values[key] }
}

// Over 1 s: each Node start installs and verifies the bundled packages.
func TestTelemetryOffSendsNothing(t *testing.T) {
	send := func(t *testing.T, node *Node) *protocol.Fault {
		t.Helper()
		return dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "send_diagnostics"})).Fault
	}
	for name, values := range map[string]map[string]string{
		"SAILRY_TELEMETRY=off": {"SAILRY_TELEMETRY": "off"},
		"DO_NOT_TRACK=1":       {"DO_NOT_TRACK": "1"},
	} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			server := newSentryFixture(t)
			values["SAILRY_SENTRY_DSN"] = server.dsn()
			node := systemNodeFixture(t, Options{Telemetry: telemetry.Options{Lookup: environment(values)}})
			if node.telemetry.Permitted() || node.telemetry.Errors() {
				t.Fatal("an opted-out process is permitted to report")
			}
			if fault := send(t, node); fault == nil || fault.Code != protocol.ErrorCodeUnavailable {
				t.Fatalf("diagnostics fault = %#v", fault)
			}
			// Saving the switches on never overrides the environment.
			settings := commandFixture(t, node, protocol.Command{Kind: "read_telemetry_settings"}).Data.(protocol.TelemetrySettings)
			commandFixture(t, node, protocol.Command{Kind: "save_telemetry_settings", Data: protocol.TelemetryUpdate{Revision: settings.Revision, Errors: true, Metrics: true}})
			if fault := send(t, node); fault == nil || fault.Code != protocol.ErrorCodeUnavailable {
				t.Fatalf("diagnostics fault after save = %#v", fault)
			}
			if server.count() != 0 {
				t.Fatalf("an opted-out process sent %d requests", server.count())
			}
		})
	}
	t.Run("no endpoint", func(t *testing.T) {
		t.Parallel()
		node := systemNodeFixture(t, Options{Telemetry: telemetry.Options{Lookup: environment(nil)}})
		if fault := send(t, node); fault == nil || fault.Code != protocol.ErrorCodeUnavailable {
			t.Fatalf("diagnostics fault = %#v", fault)
		}
	})
	t.Run("switches off send nothing unasked", func(t *testing.T) {
		t.Parallel()
		server := newSentryFixture(t)
		node := systemNodeFixture(t, Options{Telemetry: telemetry.Options{Lookup: environment(map[string]string{"SAILRY_SENTRY_DSN": server.dsn()})}})
		settings := commandFixture(t, node, protocol.Command{Kind: "read_telemetry_settings"}).Data.(protocol.TelemetrySettings)
		commandFixture(t, node, protocol.Command{Kind: "save_telemetry_settings", Data: protocol.TelemetryUpdate{Revision: settings.Revision}})
		if node.telemetry.Errors() {
			t.Fatal("automatic error reports stayed on")
		}
		// Ordinary work, a failing command included, reports nothing.
		sessionFixture(t, node)
		dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "save_power_settings", Data: protocol.PowerSettings{Revision: 99}}))
		if server.count() != 0 {
			t.Fatalf("sent %d requests without a request for diagnostics", server.count())
		}
	})
	t.Run("plugin cannot send", func(t *testing.T) {
		t.Parallel()
		server := newSentryFixture(t)
		node := systemNodeFixture(t, Options{Telemetry: telemetry.Options{Lookup: environment(map[string]string{"SAILRY_SENTRY_DSN": server.dsn()})}})
		if _, err := node.dispatchDiagnostics(context.Background(), protocol.Request{Plugin: &protocol.PluginContext{}}); err == nil {
			t.Fatal("a plugin sent diagnostics")
		}
		if server.count() != 0 {
			t.Fatal("a plugin request reached the endpoint")
		}
	})
	t.Run("requested report carries only reviewed fields", func(t *testing.T) {
		t.Parallel()
		server := newSentryFixture(t)
		node := systemNodeFixture(t, Options{Telemetry: telemetry.Options{Lookup: environment(map[string]string{"SAILRY_SENTRY_DSN": server.dsn()})}})
		settings := commandFixture(t, node, protocol.Command{Kind: "read_telemetry_settings"}).Data.(protocol.TelemetrySettings)
		if output := commandFixture(t, node, protocol.Command{Kind: "send_diagnostics"}); output.Kind != "diagnostics_sent" {
			t.Fatalf("output = %#v", output)
		}
		if server.count() != 1 {
			t.Fatalf("requests = %d", server.count())
		}
		request, body := server.requests[0], server.bodies[0]
		if request.Method != http.MethodPost || request.URL.Path != "/api/42/envelope/" || request.Header.Get("User-Agent") != "Sailry/1" ||
			request.Header.Get("Content-Type") != "application/x-sentry-envelope" || !strings.Contains(request.Header.Get("X-Sentry-Auth"), "sentry_key=public") {
			t.Fatalf("request = %s %s %v", request.Method, request.URL, request.Header)
		}
		lines := bytes.Split(bytes.TrimSuffix(body, []byte("\n")), []byte("\n"))
		if len(lines) != 5 {
			t.Fatalf("envelope has %d lines: %s", len(lines), body)
		}
		var event map[string]json.RawMessage
		if err := json.Unmarshal(lines[2], &event); err != nil {
			t.Fatal(err)
		}
		allowed := map[string]bool{"event_id": true, "timestamp": true, "level": true, "logger": true, "platform": true, "message": true,
			"release": true, "environment": true, "fingerprint": true, "tags": true, "sdk": true}
		for key := range event {
			// The library always writes its empty user object.
			if !allowed[key] && !(key == "user" && string(event[key]) == "{}") {
				t.Errorf("event carries %q: %s", key, event[key])
			}
		}
		var tags map[string]string
		if err := json.Unmarshal(event["tags"], &tags); err != nil {
			t.Fatal(err)
		}
		if len(tags) != 5 || tags["installation_id"] != settings.InstallationID.String() || tags["application"] != "desktop" ||
			tags["os"] != reportedSystem[runtime.GOOS] || tags["arch"] != reportedArch[runtime.GOARCH] || tags["channel"] != "dev" {
			t.Fatalf("tags = %v", tags)
		}
		if string(event["message"]) != `"runtime.diagnostics:requested"` || string(event["level"]) != `"info"` ||
			string(event["release"]) != `"sailry@`+link.ProductVersion+`+unknown"` || string(event["sdk"]) != `{"name":"Sailry","version":"1"}` {
			t.Fatalf("event = %s", lines[2])
		}
		var attachment struct {
			Type, Filename string
			Length         int
			ContentType    string `json:"content_type"`
		}
		if err := json.Unmarshal(lines[3], &attachment); err != nil {
			t.Fatal(err)
		}
		if attachment.Type != "attachment" || attachment.Filename != "diagnostics.jsonl" || attachment.Length != 0 || attachment.ContentType != "application/x-ndjson" || len(lines[4]) != 0 {
			t.Fatalf("attachment = %s / %q", lines[3], lines[4])
		}

		// A new identity replaces the old one in the next report.
		reset := commandFixture(t, node, protocol.Command{Kind: "reset_telemetry_identity", Data: protocol.CommandResetTelemetryIdentityPayload{ExpectedRevision: settings.Revision}}).Data.(protocol.TelemetrySettings)
		if reset.InstallationID == settings.InstallationID || reset.Revision != settings.Revision+1 || reset.Errors != settings.Errors || reset.Metrics != settings.Metrics {
			t.Fatalf("reset = %#v from %#v", reset, settings)
		}
		stale := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "reset_telemetry_identity", Data: protocol.CommandResetTelemetryIdentityPayload{ExpectedRevision: settings.Revision}}))
		if stale.Fault == nil || stale.Fault.Code != protocol.ErrorCodeRevisionConflict {
			t.Fatalf("stale reset = %#v", stale)
		}
		commandFixture(t, node, protocol.Command{Kind: "send_diagnostics"})
		if !bytes.Contains(server.bodies[1], []byte(reset.InstallationID.String())) || bytes.Contains(server.bodies[1], []byte(settings.InstallationID.String())) {
			t.Fatal("the report kept the previous identity")
		}

		// A refused delivery is a failure, never a success.
		server.mu.Lock()
		server.status = http.StatusFound
		server.mu.Unlock()
		if fault := send(t, node); fault == nil || fault.Code != protocol.ErrorCodeUnavailable {
			t.Fatalf("redirected delivery = %#v", fault)
		}
		if server.count() != 3 {
			t.Fatalf("a redirect was followed or retried: %d requests", server.count())
		}
	})
	t.Run("log-backed reports and metrics follow their switches", func(t *testing.T) {
		profile := filepath.Join(t.TempDir(), "profile")
		closeLog, err := logging.Install(profile, "desktop")
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(closeLog)
		server, usage := newSentryFixture(t), &usageFixture{}
		node, err := StartWithOptions(context.Background(), profile, Options{Telemetry: telemetry.Options{Transport: usage, Timer: usage.timer,
			Lookup: environment(map[string]string{"SAILRY_SENTRY_DSN": server.dsn(), "SAILRY_TELEMETRY_KEY": "usage-key"})}})
		if err != nil {
			t.Fatal(err)
		}
		stopped := false
		t.Cleanup(func() {
			if !stopped {
				_ = node.Stop(context.Background())
			}
		})
		save := func(errors, metrics bool) {
			t.Helper()
			settings := commandFixture(t, node, protocol.Command{Kind: "read_telemetry_settings"}).Data.(protocol.TelemetrySettings)
			commandFixture(t, node, protocol.Command{Kind: "save_telemetry_settings", Data: protocol.TelemetryUpdate{Revision: settings.Revision, Errors: errors, Metrics: metrics}})
		}
		failure := func() {
			logging.Emit(slog.LevelWarn, slog.String("area", "command"), slog.String("phase", "failed"), slog.String("operation", "read_session"), slog.String("code", "not_found"),
				slog.String("session", "local-session-value"))
			logging.Emit(slog.LevelError, slog.String("area", "command"), slog.String("phase", "failed"), slog.String("operation", "submit_turn"), slog.String("code", "internal"),
				slog.String("session", "local-session-value"), slog.String("detail", "/Users/someone/project"), slog.String("model", "private-model"), slog.String("prompt", "SECRET PROMPT"))
		}
		reached := func(count int) {
			t.Helper()
			for deadline := time.Now().Add(10 * time.Second); server.count() < count; time.Sleep(5 * time.Millisecond) {
				if time.Now().After(deadline) {
					t.Fatalf("requests = %d, want %d", server.count(), count)
				}
			}
		}
		// The saved switches start on: the launch opens the usage window.
		if !node.logs || !node.telemetry.Errors() || !node.telemetry.Metrics() || usage.started() != 1 {
			t.Fatalf("logs %v, errors %v, metrics %v, timers %d", node.logs, node.telemetry.Errors(), node.telemetry.Metrics(), usage.started())
		}

		// Off: no report, no counter, no timer.
		save(false, false)
		if usage.stopped() != 1 {
			t.Fatalf("the usage window's timer kept running: %d stopped", usage.stopped())
		}
		session := protocol.NewSessionID()
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
			Session: session, Message: protocol.ConversationInput{Text: "SECRET PROMPT"}}})
		refused := dispatchFixture(t, node, node.ID(), request)
		if refused.Fault == nil {
			t.Fatal("a turn was queued in a missing session")
		}
		// A read whose caller has gone is cancelled, not failed.
		gone, leave := context.WithCancel(context.Background())
		leave()
		abandoned := protocol.NewRequest(node.ID(), protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: session}})
		if accepted, err := node.Dispatch(gone, node.ID(), abandoned); err == nil {
			<-accepted.Completion
		}
		failure()
		node.telemetry.Observe(telemetry.Turn("completed", protocol.WorkModeCode, false, nil))
		if node.telemetry.Capture() != nil {
			t.Fatal("an operation captured consent with usage metrics off")
		}
		if server.count() != 0 || usage.count() != 0 || usage.started() != 1 {
			t.Fatalf("with the switches off: %d reports, %d usage requests, %d timers", server.count(), usage.count(), usage.started())
		}
		// The file names the command and its identifiers, never its payload.
		current, err := logging.CurrentFile()
		if err != nil || current != filepath.Join(logging.Directory(), "desktop", "current.log") {
			t.Fatalf("current log = %q, %v", current, err)
		}
		written, err := os.ReadFile(current)
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.Contains(written, []byte(" desktop area=command phase=begin node="+node.ID().String()+" caller="+node.ID().String()+" request="+request.ID.String()+" session="+session.String()+" command=submit_turn operation=submit_turn")) ||
			!bytes.Contains(written, []byte(" WARN desktop area=command phase=failed ")) || bytes.Contains(written, []byte("SECRET")) ||
			bytes.Contains(written, []byte("save_telemetry_settings")) {
			t.Fatalf("log = %s", written)
		}
		observed := 0
		for _, line := range bytes.Split(written, []byte("\n")) {
			if bytes.Contains(line, []byte(abandoned.ID.String())) {
				observed++
				if !bytes.Contains(line, []byte(" INFO desktop area=command phase=cancelled ")) || !bytes.Contains(line, []byte("code=cancelled")) {
					t.Fatalf("abandoned read logged as %s", line)
				}
			}
		}
		if observed != 1 {
			t.Fatalf("abandoned read has %d records", observed)
		}
		// A requested report is the controller's own act: the switches do
		// not gate it, and it attaches the log's public fields only.
		commandFixture(t, node, protocol.Command{Kind: "send_diagnostics"})
		if server.count() != 1 {
			t.Fatalf("requests = %d", server.count())
		}
		lines := bytes.Split(bytes.TrimSuffix(server.bodies[0], []byte("\n")), []byte("\n"))
		if len(lines) < 5 || bytes.Contains(server.bodies[0], []byte(session.String())) || bytes.Contains(server.bodies[0], []byte(node.ID().String())) ||
			bytes.Contains(server.bodies[0], []byte("local-session-value")) || bytes.Contains(server.bodies[0], []byte("/Users/someone")) || bytes.Contains(server.bodies[0], []byte("private-model")) {
			t.Fatalf("diagnostics = %s", server.bodies[0])
		}
		public := map[string]bool{"level": true, "timestamp": true, "area": true, "phase": true, "operation": true, "code": true, "duration_us": true}
		records := 0
		for _, entry := range lines[4:] {
			if len(entry) == 0 {
				continue
			}
			records++
			var record map[string]any
			if err := json.Unmarshal(entry, &record); err != nil {
				t.Fatalf("attachment line %q: %v", entry, err)
			}
			for key := range record {
				if !public[key] {
					t.Errorf("attachment carries %q: %s", key, entry)
				}
			}
		}
		if records < 4 {
			t.Fatalf("attachment has %d records: %s", records, server.bodies[0])
		}

		// On: a failed operation is reported with its public fields.
		save(true, true)
		failure()
		reached(2)
		lines = bytes.Split(server.bodies[1], []byte("\n"))
		var event struct {
			Message, Level string
			Contexts       map[string]map[string]any
			Breadcrumbs    []struct {
				Category string
				Data     map[string]any
			}
			Exception []struct {
				Type, Value string
				Mechanism   struct {
					Type    string
					Handled bool
				}
			}
		}
		if err := json.Unmarshal(lines[2], &event); err != nil {
			t.Fatal(err)
		}
		fields := map[string]any{"area": "command", "phase": "failed", "operation": "submit_turn", "code": "internal"}
		if event.Message != "command.submit_turn:internal" || event.Level != "error" || len(event.Contexts) != 1 || !reflect.DeepEqual(event.Contexts["sailry"], fields) ||
			len(event.Exception) != 1 || event.Exception[0].Type != "SailryError" || event.Exception[0].Mechanism.Type != "tracing" || !event.Exception[0].Mechanism.Handled ||
			len(event.Breadcrumbs) != 1 || event.Breadcrumbs[0].Category != "sailry" ||
			!reflect.DeepEqual(event.Breadcrumbs[0].Data, map[string]any{"area": "command", "phase": "failed", "operation": "read_session", "code": "not_found"}) {
			t.Fatalf("event = %s", lines[2])
		}
		home, _ := os.UserHomeDir()
		working, _ := os.Getwd()
		for _, private := range []string{"local-session-value", "/Users/someone", "private-model", "SECRET", profile, home, working, node.ID().String()} {
			if private != "" && bytes.Contains(server.bodies[1], []byte(private)) {
				t.Errorf("the report carries %q: %s", private, server.bodies[1])
			}
		}
		// One failure, here the loop's own call site, reports ten times an
		// hour at most.
		for range 15 {
			failure()
		}
		reached(12)

		// Usage counters leave once, by table, when the window's timer fires.
		if usage.count() != 0 || usage.started() != 1 {
			t.Fatalf("before any observation: %d usage requests, %d timers", usage.count(), usage.started())
		}
		denied := protocol.ErrorCodePermissionDenied
		node.telemetry.Observe(telemetry.Turn("completed", protocol.WorkModeCode, false, nil))
		node.telemetry.Observe(telemetry.Turn("completed", protocol.WorkModeCode, false, nil))
		capture := node.telemetry.Capture()
		tool, counted := telemetry.CaptureTool("read_session")
		if _, workspace := telemetry.CaptureTool("write_file"); workspace || !counted || capture == nil {
			t.Fatalf("a workspace tool is counted (%v), a listed tool is not (%v) or no consent was captured", workspace, !counted)
		}
		if owned, _ := telemetry.CaptureTool("plugin_0123456789abcdef_private_name"); owned != (telemetry.Tool{Source: "third_party"}) {
			t.Fatalf("a package's tool is named: %+v", owned)
		}
		capture.Record(telemetry.ToolCall(tool, "denied", &denied, 3*time.Millisecond))
		if usage.started() != 2 || usage.count() != 0 {
			t.Fatalf("timers = %d, usage requests = %d", usage.started(), usage.count())
		}
		usage.fire()
		batches := usage.batches(t)
		if len(batches) == 0 || batches[0].Table != "turns" || len(batches[0].Data) != 1 || batches[0].Data[0]["count"] != float64(2) ||
			batches[0].Data[0]["result"] != "completed" || batches[0].Data[0]["mode"] != "code" || batches[0].Data[0]["subagent"] != false {
			t.Fatalf("usage = %+v", batches)
		}
		if len(batches) != 2 || batches[1].Table != "tool_calls" || batches[1].Data[0]["elapsed_us_total"] != float64(3000) || batches[1].Data[0]["error_code"] != "permission_denied" ||
			!reflect.DeepEqual(batches[1].Data[0]["tool"], map[string]any{"source": "builtin", "name": "read_session"}) {
			t.Fatalf("usage = %+v", batches)
		}
		allowed := map[string]bool{"event_id": true, "window_start": true, "window_end": true, "count": true, "installation_id": true, "channel": true, "application": true,
			"os": true, "arch": true, "release": true, "result": true, "mode": true, "subagent": true, "error_code": true, "tool": true, "elapsed_us_total": true, "elapsed_us_max": true}
		for _, sent := range batches {
			for _, row := range sent.Data {
				for key := range row {
					if !allowed[key] {
						t.Errorf("%s row carries %q", sent.Table, key)
					}
				}
			}
		}
		for _, request := range usage.requests {
			if request.Method != http.MethodPost || request.URL.String() != "https://api.telemetry.sh/log" || request.Header.Get("Authorization") != "Bearer usage-key" {
				t.Fatalf("usage request = %s %s %v", request.Method, request.URL, request.Header)
			}
		}

		// Off again: what was counted is dropped and nothing new starts.
		node.telemetry.Observe(telemetry.Turn("failed", protocol.WorkModeCode, true, nil))
		sent, timers := usage.count(), usage.started()
		save(false, false)
		capture.Record(telemetry.Turn("completed", protocol.WorkModeCode, false, nil))
		failure()
		if err := node.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
		stopped = true
		if server.count() != 12 || usage.count() != sent || usage.started() != timers || usage.stopped() != 2 {
			t.Fatalf("after switching off: %d reports, %d usage requests, %d timers, %d stopped", server.count(), usage.count(), usage.started(), usage.stopped())
		}
	})
}

var (
	reportedSystem = map[string]string{"darwin": "macos", "linux": "linux", "windows": "windows"}
	reportedArch   = map[string]string{"arm64": "aarch64", "amd64": "x86_64"}
)

// usageFixture stands in for the usage endpoint and the window's timer.
// Requests for any other host go to the network's loopback fixtures.
type usageFixture struct {
	mu       sync.Mutex
	requests []*http.Request
	bodies   [][]byte
	pending  []func()
	timers   int
	stops    int
}

func (f *usageFixture) RoundTrip(request *http.Request) (*http.Response, error) {
	if request.URL.Host != "api.telemetry.sh" {
		return http.DefaultTransport.RoundTrip(request)
	}
	body, _ := io.ReadAll(request.Body)
	f.mu.Lock()
	defer f.mu.Unlock()
	f.requests, f.bodies = append(f.requests, request), append(f.bodies, body)
	return &http.Response{StatusCode: http.StatusOK, Body: http.NoBody, Request: request}, nil
}

func (f *usageFixture) timer(_ time.Duration, run func()) func() {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.timers, f.pending = f.timers+1, append(f.pending, run)
	return func() {
		f.mu.Lock()
		defer f.mu.Unlock()
		f.stops++
	}
}

// fire runs the latest timer.
func (f *usageFixture) fire() {
	f.mu.Lock()
	run := f.pending[len(f.pending)-1]
	f.mu.Unlock()
	run()
}

func (f *usageFixture) started() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.timers
}

func (f *usageFixture) stopped() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.stops
}

func (f *usageFixture) count() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.requests)
}

type usageBatch struct {
	Table string
	Data  []map[string]any
}

func (f *usageFixture) batches(t *testing.T) []usageBatch {
	t.Helper()
	f.mu.Lock()
	defer f.mu.Unlock()
	var out []usageBatch
	for _, body := range f.bodies {
		var sent usageBatch
		if err := json.Unmarshal(body, &sent); err != nil {
			t.Fatal(err)
		}
		out = append(out, sent)
	}
	return out
}

// powerFixture counts the system requests the Node takes and returns.
type powerFixture struct {
	mu                 sync.Mutex
	held               bool
	acquired, released int
	refuse             int64
}

func (f *powerFixture) Supported() bool { return true }
func (f *powerFixture) Held() bool {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.held
}
func (f *powerFixture) Acquire() int64 {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.refuse != 0 {
		return f.refuse
	}
	f.held, f.acquired = true, f.acquired+1
	return 0
}
func (f *powerFixture) Release() int64 {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.held, f.released = false, f.released+1
	return 0
}
func (f *powerFixture) counts() (bool, int, int) {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.held, f.acquired, f.released
}

// Over 1 s: each Node start installs and verifies the bundled packages.
func TestPowerAssertionOwnership(t *testing.T) {
	t.Parallel()
	save := func(t *testing.T, node *Node, prevent bool) {
		t.Helper()
		settings := commandFixture(t, node, protocol.Command{Kind: "read_power_settings"}).Data.(protocol.PowerSettings)
		commandFixture(t, node, protocol.Command{Kind: "save_power_settings", Data: protocol.PowerSettings{Revision: settings.Revision, PreventSleep: prevent}})
	}
	t.Run("one request follows the setting and running work", func(t *testing.T) {
		t.Parallel()
		backend := &powerFixture{}
		path := filepath.Join(t.TempDir(), "profile")
		node, err := StartWithOptions(context.Background(), path, Options{Power: backend})
		if err != nil {
			t.Fatal(err)
		}
		// An idle Node lets the machine sleep whatever the setting says.
		if held, acquired, _ := backend.counts(); held || acquired != 0 {
			t.Fatalf("startup = %v %d", held, acquired)
		}
		first, second := node.power.Hold(), node.power.Hold()
		save(t, node, true)
		if held, acquired, _ := backend.counts(); !held || acquired != 1 {
			t.Fatalf("overlapping work = %v %d", held, acquired)
		}
		save(t, node, false)
		if held, _, released := backend.counts(); held || released != 1 {
			t.Fatalf("off = %v %d", held, released)
		}
		// A stale save changes nothing.
		stale := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "save_power_settings", Data: protocol.PowerSettings{PreventSleep: true}}))
		if held, _, _ := backend.counts(); stale.Fault == nil || held {
			t.Fatalf("stale save = %#v, held %v", stale, held)
		}
		save(t, node, true)
		first()
		first()
		if held, acquired, _ := backend.counts(); !held || acquired != 2 {
			t.Fatalf("on with one work left = %v %d", held, acquired)
		}
		second()
		if held, _, released := backend.counts(); held || released != 2 {
			t.Fatalf("idle = %v %d", held, released)
		}

		// A supervised task is counted for as long as it runs.
		started, finish := make(chan struct{}), make(chan struct{})
		if err := node.tasks.Submit(func(context.Context) { close(started); <-finish }); err != nil {
			t.Fatal(err)
		}
		<-started
		if held, acquired, _ := backend.counts(); !held || acquired != 3 {
			t.Fatalf("running task = %v %d", held, acquired)
		}
		close(finish)
		for deadline := time.Now().Add(10 * time.Second); ; time.Sleep(time.Millisecond) {
			if held, _, _ := backend.counts(); !held {
				break
			}
			if time.Now().After(deadline) {
				t.Fatal("the request outlived its task")
			}
		}

		// Stop releases the request under work that is still counted.
		abandoned := node.power.Hold()
		if err := node.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
		if held, acquired, released := backend.counts(); held || acquired != 4 || released != 4 {
			t.Fatalf("stopped = %v %d %d", held, acquired, released)
		}
		abandoned()
		if status := node.power.Status(); !status.Closed || status.Acquired {
			t.Fatalf("status = %#v", status)
		}

		// The saved setting is what the next start applies to its work.
		restarted, err := StartWithOptions(context.Background(), path, Options{Power: backend})
		if err != nil {
			t.Fatal(err)
		}
		if held, acquired, _ := backend.counts(); held || acquired != 4 {
			t.Fatalf("restart = %v %d", held, acquired)
		}
		work := restarted.power.Hold()
		if held, acquired, _ := backend.counts(); !held || acquired != 5 {
			t.Fatalf("work after restart = %v %d", held, acquired)
		}
		if err := restarted.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
		work()
		if held, _, _ := backend.counts(); held {
			t.Fatal("the request outlived the Node")
		}
	})
	t.Run("a refusal is one notice and no retry", func(t *testing.T) {
		t.Parallel()
		backend := &powerFixture{refuse: 7}
		node := systemNodeFixture(t, Options{Power: backend})
		if notices := snapshotFixture(t, node).Notifications; len(notices) != 0 {
			t.Fatalf("an idle Node asked for the request: %#v", notices)
		}
		defer node.power.Hold()()
		notices := snapshotFixture(t, node).Notifications
		if len(notices) != 1 || notices[0].Content.Kind != protocol.NotificationKindError || notices[0].Content.Details == nil || notices[0].Content.Details.Kind != "power" {
			t.Fatalf("notices = %#v", notices)
		}
		if details, ok := notices[0].Content.Details.Data.(*protocol.NotificationDetailsPowerPayload); !ok || details.Code != 7 {
			t.Fatalf("details = %#v", notices[0].Content.Details.Data)
		}
		backend.mu.Lock()
		backend.refuse = 0
		backend.mu.Unlock()
		save(t, node, false)
		save(t, node, true)
		defer node.power.Hold()()
		if status := node.power.Status(); status.Acquired || status.Error != 7 || len(snapshotFixture(t, node).Notifications) != 1 {
			t.Fatalf("status = %#v", status)
		}
	})
}
