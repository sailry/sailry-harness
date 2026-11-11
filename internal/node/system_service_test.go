package node

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/store"
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
		var saved protocol.RequestID
		save := func(errors, metrics bool) {
			t.Helper()
			settings := commandFixture(t, node, protocol.Command{Kind: "read_telemetry_settings"}).Data.(protocol.TelemetrySettings)
			request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "save_telemetry_settings", Data: protocol.TelemetryUpdate{Revision: settings.Revision, Errors: errors, Metrics: metrics}})
			if fault := dispatchFixture(t, node, node.ID(), request).Fault; fault != nil {
				t.Fatal(fault)
			}
			saved = request.ID
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
		// Every change is recorded, a credential-bearing one by its kind
		// and typed identifiers alone, whether it is accepted or refused.
		changes := map[string]protocol.RequestID{}
		change := func(command protocol.Command) {
			request := protocol.NewRequest(node.ID(), command)
			changes[command.Kind] = request.ID
			if accepted, err := node.Dispatch(context.Background(), node.ID(), request); err == nil {
				<-accepted.Completion
			}
		}
		secret := protocol.NewSecret("SECRET-credential")
		provider := protocol.DefaultConversationProvider()
		provider.ID, provider.Name, provider.Api = protocol.NewProviderID(), "SECRET provider", protocol.ConversationModelApiChatCompletions
		provider.Authentication, provider.Endpoint = protocol.AuthenticationApiKey, "https://SECRET.test/v1"
		worktree := protocol.NewWorktreeID()
		change(protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
		change(protocol.Command{Kind: "put_credential", Data: protocol.CommandPutCredentialPayload{ID: protocol.NewCredentialID(), Provider: provider.ID, Secret: secret}})
		change(protocol.Command{Kind: "save_database", Data: protocol.CommandSaveDatabasePayload{Password: &secret, Profile: protocol.DatabaseProfile{
			ID: protocol.NewDatabaseID(), Name: "SECRET database", ReadOnly: true, Connection: protocol.DatabaseConnection{Kind: "network", Data: &protocol.DatabaseConnectionNetworkPayload{
				Engine: protocol.DatabaseEnginePostgres, Host: "SECRET.test", Port: 5432, Database: "SECRET", Username: "SECRET", Tls: protocol.DatabaseTlsRequire}}}}})
		change(protocol.Command{Kind: "save_ssh", Data: protocol.CommandSaveSshPayload{
			Profile:    protocol.SshProfile{ID: protocol.NewSshID(), Name: "SECRET host", Host: "SECRET.test", Port: 22, Username: "SECRET", Authentication: protocol.SshAuthenticationPassword},
			Credential: &protocol.SshCredential{Kind: "password", Data: &protocol.SshCredentialPasswordPayload{Password: secret}}}})
		backup := protocol.BackupUpdate{Settings: commandFixture(t, node, protocol.Command{Kind: "read_backup_settings"}).Data.(protocol.BackupSettings),
			Identity: backupFixtureKeep(), Secret: backupFixtureKeep(), SessionToken: backupFixtureKeep(), Passphrase: protocol.PluginSettingsSecretUpdate{Kind: "replace", Data: &secret}}
		change(protocol.Command{Kind: "save_backup_settings", Data: backup})
		change(protocol.Command{Kind: "write_file", Data: protocol.CommandWriteFilePayload{Worktree: worktree, Path: "SECRET/notes.txt", Text: "SECRET contents"}})
		// A forwarded configuration is recorded like any other credential.
		forwarded := protocol.DefaultSessionImport()
		forwarded.Source, forwarded.Provider, forwarded.Secret = node.ID(), provider, &secret
		forwarded.Config.Provider, forwarded.Config.Model = provider.ID, "SECRET-model"
		forwarded.Config.Mode, forwarded.Config.Permission = protocol.WorkModeCode, protocol.PermissionProject
		forwarded.RoleCredentials = []protocol.RoleProviderSecret{{Provider: provider.ID, Secret: secret}}
		change(protocol.Command{Kind: "import_session", Data: forwarded})
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
			!bytes.Contains(written, []byte(" INFO desktop area=command phase=completed node="+node.ID().String()+" caller="+node.ID().String()+" request="+saved.String()+" command=save_telemetry_settings operation=save_telemetry_settings duration_us=")) ||
			bytes.Contains(written, []byte("read_telemetry_settings")) || bytes.Contains(written, []byte("read_backup_settings")) {
			t.Fatalf("log = %s", written)
		}
		for kind, id := range changes {
			begun, finished := false, false
			for _, line := range bytes.Split(written, []byte("\n")) {
				if !bytes.Contains(line, []byte(" request="+id.String()+" ")) {
					continue
				}
				if !bytes.Contains(line, []byte(" command="+kind+" operation="+kind)) || !bytes.Contains(line, []byte(" node="+node.ID().String()+" caller="+node.ID().String()+" ")) {
					t.Fatalf("%s logged as %s", kind, line)
				}
				begun = begun || bytes.Contains(line, []byte(" phase=begin "))
				finished = finished || bytes.Contains(line, []byte(" duration_us=")) && (bytes.Contains(line, []byte(" phase=completed ")) || bytes.Contains(line, []byte(" phase=failed ")))
			}
			if !begun || !finished {
				t.Fatalf("%s has no complete record (begin %v, outcome %v): %s", kind, begun, finished, written)
			}
		}
		if !bytes.Contains(written, []byte(" request="+changes["put_credential"].String()+" provider="+provider.ID.String()+" command=put_credential ")) ||
			!bytes.Contains(written, []byte(" request="+changes["write_file"].String()+" worktree="+worktree.String()+" command=write_file ")) {
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
		tool, counted := telemetry.BuiltinTool("read_session")
		if !counted || capture == nil {
			t.Fatalf("a built-in tool is not counted (%v) or no consent was captured", !counted)
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
	t.Run("feature counters take the local controller's closed list only", func(t *testing.T) {
		t.Parallel()
		usage := &usageFixture{}
		node := systemNodeFixture(t, Options{Telemetry: telemetry.Options{Transport: usage, Timer: usage.timer,
			Lookup: environment(map[string]string{"SAILRY_TELEMETRY_KEY": "usage-key"})}})
		count := func(caller protocol.NodeID, feature protocol.FeatureUse) *protocol.Fault {
			t.Helper()
			accepted, err := node.Dispatch(context.Background(), caller, protocol.NewRequest(node.ID(), protocol.Command{Kind: "count_feature", Data: protocol.CommandCountFeaturePayload{Feature: feature}}))
			if err != nil {
				var fault *protocol.Fault
				if !errors.As(err, &fault) {
					t.Fatal(err)
				}
				return fault
			}
			response := <-accepted.Completion
			if response.Fault == nil && response.Output.Kind != "feature_counted" {
				t.Fatalf("output = %+v", response.Output)
			}
			return response.Fault
		}
		shipped, err := operations.OfficialPackages()
		if err != nil || len(shipped) == 0 {
			t.Fatal(shipped, err)
		}
		official := protocol.FeatureUsePagePluginOfficial + protocol.FeatureUse(shipped[0])

		save := func(metrics bool) {
			t.Helper()
			settings := commandFixture(t, node, protocol.Command{Kind: "read_telemetry_settings"}).Data.(protocol.TelemetrySettings)
			commandFixture(t, node, protocol.Command{Kind: "save_telemetry_settings", Data: protocol.TelemetryUpdate{Revision: settings.Revision, Metrics: metrics}})
		}
		// Off: the count is accepted, and nothing is kept or scheduled.
		save(false)
		windows := usage.started()
		if fault := count(node.ID(), protocol.FeatureUsePageSettings); fault != nil {
			t.Fatal(fault)
		}
		if usage.started() != windows || node.telemetry.Capture() != nil {
			t.Fatalf("usage off started %d windows", usage.started()-windows)
		}
		save(true)
		for _, feature := range []protocol.FeatureUse{protocol.FeatureUsePageSettings, protocol.FeatureUsePageSettings, protocol.FeatureUseSaveTelemetry, official} {
			if fault := count(node.ID(), feature); fault != nil {
				t.Fatalf("%s: %v", feature, fault)
			}
		}
		// Outside the closed list, a package the application does not ship
		// under a planted name, or a controller that is not this device.
		for _, feature := range []protocol.FeatureUse{"", "page.unknown", "settings.save./Users/someone/private", protocol.FeatureUsePagePluginOfficial + "my-private-package",
			protocol.FeatureUsePagePluginOfficial + protocol.FeatureUse(shipped[0]) + " copy"} {
			if fault := count(node.ID(), feature); fault == nil || fault.Code != protocol.ErrorCodeInvalidRequest {
				t.Errorf("feature %q: %v", feature, fault)
			}
		}
		if fault := count(protocol.NodeID{1}, protocol.FeatureUsePageSettings); fault == nil {
			t.Error("another controller counted a feature")
		}
		usage.fire()
		rows := map[string]float64{}
		for _, batch := range usage.batches(t) {
			if batch.Table != "feature_uses" {
				continue
			}
			for _, row := range batch.Data {
				rows[row["feature"].(string)] = row["count"].(float64)
				for key := range row {
					if !slices.Contains([]string{"event_id", "window_start", "window_end", "count", "installation_id", "channel", "application", "os", "arch", "release", "feature"}, key) {
						t.Errorf("feature row carries %q", key)
					}
				}
			}
		}
		if want := map[string]float64{"page.settings": 2, "settings.save.telemetry": 1, string(official): 1}; !reflect.DeepEqual(rows, want) {
			t.Fatalf("feature rows = %v, want %v", rows, want)
		}

		// Every tool the Node registers itself is counted under its own name
		// or left out with a stated reason; none is silently uncounted.
		project := protocol.NewProjectID()
		invocation := &store.AgentInvocation{}
		invocation.Worktree.Project = &project
		registered, err := node.agentTools(invocation)
		if err != nil || len(registered) < 13 {
			t.Fatalf("%d registered tools, %v", len(registered), err)
		}
		for _, tool := range registered {
			name := tool.Definition.Name
			counted, ok := engine.CountedTool(registered, name)
			if reason := telemetry.UncountedBuiltins[name]; ok == (reason != "") || ok && counted != (telemetry.Tool{Source: "builtin", Name: name}) {
				t.Errorf("registered tool %q is counted as %+v (%v) with exclusion %q", name, counted, ok, reason)
			}
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
	refuse, stuck      int64
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
	if f.stuck != 0 {
		return f.stuck
	}
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

		// A short supervised command is not work: it never takes the request.
		started, finish := make(chan struct{}), make(chan struct{})
		if err := node.tasks.Submit(func(context.Context) { close(started); <-finish }); err != nil {
			t.Fatal(err)
		}
		<-started
		if held, acquired, _ := backend.counts(); held || acquired != 2 {
			t.Fatalf("supervised command = %v %d", held, acquired)
		}
		close(finish)

		// An installation is counted for as long as it runs.
		working := node.plugins.installations.Working()
		if held, acquired, _ := backend.counts(); !held || acquired != 3 {
			t.Fatalf("running installation = %v %d", held, acquired)
		}
		working()
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
	t.Run("a refusal is one notice and the request is asked again", func(t *testing.T) {
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
		// Work that starts while the system still refuses is not a second notice.
		node.power.Hold()()
		if status := node.power.Status(); status.Acquired || status.Error != 7 || len(snapshotFixture(t, node).Notifications) != 1 {
			t.Fatalf("second refusal = %#v", status)
		}
		// The saved setting takes effect as soon as the system allows it.
		backend.mu.Lock()
		backend.refuse = 0
		backend.mu.Unlock()
		save(t, node, false)
		save(t, node, true)
		if held, acquired, _ := backend.counts(); !held || acquired != 1 || len(snapshotFixture(t, node).Notifications) != 1 {
			t.Fatalf("allowed again = %v %d", held, acquired)
		}
		// A release the system refuses is asked again too: off means released.
		backend.mu.Lock()
		backend.stuck = 9
		backend.mu.Unlock()
		save(t, node, false)
		if held, _, released := backend.counts(); !held || released != 0 {
			t.Fatalf("refused release = %v %d", held, released)
		}
		backend.mu.Lock()
		backend.stuck = 0
		backend.mu.Unlock()
		node.power.Hold()()
		if held, _, released := backend.counts(); held || released != 1 || len(snapshotFixture(t, node).Notifications) != 1 {
			t.Fatalf("released = %v %d", held, released)
		}
	})
}
