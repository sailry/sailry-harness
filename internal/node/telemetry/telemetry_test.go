package telemetry

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"regexp"
	"runtime/debug"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// endpoints stands in for both remote endpoints: nothing leaves the process.
type endpoints struct {
	mu      sync.Mutex
	usage   [][]byte
	reports [][]byte
	timers  []func()
}

func (e *endpoints) RoundTrip(request *http.Request) (*http.Response, error) {
	body, _ := io.ReadAll(request.Body)
	e.mu.Lock()
	defer e.mu.Unlock()
	if request.URL.Host == "api.telemetry.sh" {
		e.usage = append(e.usage, body)
	} else {
		e.reports = append(e.reports, body)
	}
	return &http.Response{StatusCode: http.StatusOK, Body: http.NoBody, Request: request}, nil
}

func (e *endpoints) timer(_ time.Duration, run func()) func() {
	e.mu.Lock()
	defer e.mu.Unlock()
	e.timers = append(e.timers, run)
	return func() {}
}

func (e *endpoints) sent() (usage, reports [][]byte) {
	e.mu.Lock()
	defer e.mu.Unlock()
	return slices.Clone(e.usage), slices.Clone(e.reports)
}

func control(errors, metrics bool) (*Control, *endpoints) {
	remote := &endpoints{}
	values := map[string]string{"SAILRY_SENTRY_DSN": "http://public@reports.invalid/42", "SAILRY_TELEMETRY_KEY": "key"}
	c := New(Options{Lookup: func(name string) string { return values[name] }, Transport: remote, Timer: remote.timer,
		Models: map[string][]string{"preset": {"listed-model"}}, Packages: []string{"browser"}})
	c.Configure(protocol.TelemetrySettings{Errors: errors, Metrics: metrics, InstallationID: uuid.New()})
	return c, remote
}

// The planted values stand for what a payload must never carry.
const (
	plantedSecret = "sk-planted-secret-0123456789"
	plantedPath   = "/Users/planted/Private Project/notes.md"
	plantedName   = "Planted Custom Name"
)

// crashTrace is a runtime traceback as debug.SetCrashOutput receives it.
var crashTrace = "panic: open " + plantedPath + ": token " + plantedSecret + " [recovered]\n" +
	"\tpanic: runtime error: invalid memory address or nil pointer dereference\n" +
	"[signal SIGSEGV: segmentation violation code=0x2 addr=0x0 pc=0x1027a4c10]\n\n" +
	"goroutine 1 [running, locked to thread]:\n" +
	"github.com/sailry/sailry-harness/internal/desktop/shell.(*App).View(0x14000123456, {0x1400abcdef0, 0x1c})\n" +
	"\t" + plantedPath + "/internal/desktop/shell/shell.go:231 +0x1c4\n" +
	"main.run.func3({0x1, 0x2}, ...)\n" +
	"\t/Users/planted/go/src/cmd/sailry-desktop/main.go:112 +0x58\n" +
	"main.main()\n" +
	"\t/Users/planted/go/src/cmd/sailry-desktop/main.go:33 +0x20\n\n" +
	"goroutine 42 [chan receive, 3 minutes]:\n" +
	"github.com/sailry/sailry-harness/internal/node.(*Node).serve(0x14000222222)\n" +
	"\t/Users/planted/go/src/internal/node/node.go:300 +0x90\n" +
	"created by github.com/sailry/sailry-harness/internal/node.Start in goroutine 1\n" +
	"\t/Users/planted/go/src/internal/node/node.go:200 +0x44\n\n" +
	"rax    0x" + plantedSecret + "\n"

var reducedLine = regexp.MustCompile(`^(|[a-z_.]+|goroutine \d+ \[[A-Za-z0-9 ,]+\]|(created by )?[A-Za-z0-9_./*()\[\]{}<>$+-]+:\d+)$`)

func assertReduced(t *testing.T, reduced []byte) {
	t.Helper()
	for line := range strings.SplitSeq(string(reduced), "\n") {
		if !reducedLine.MatchString(line) {
			t.Errorf("a reduced trace keeps more than a class, a goroutine state or function:line: %q", line)
		}
	}
	assertClean(t, "reduced trace", reduced)
}

func assertClean(t *testing.T, what string, payload []byte) {
	t.Helper()
	directory, _ := os.Getwd()
	// ".go:" is a source file before its line; the compiler's own function
	// names hold ".go" (a go statement's "gowrap" wrapper).
	for _, planted := range []string{plantedSecret, plantedPath, plantedName, "planted", "/Users/", ".go:", ".go ", directory, os.TempDir()} {
		if planted != "" && bytes.Contains(payload, []byte(planted)) {
			t.Errorf("%s carries %q", what, planted)
		}
	}
}

// TestCountersAndInterfaceReports covers the usage payload's allow-list, the
// closed feature list, consent for every new kind, and the interface's crash
// and hang reports.
func TestCountersAndInterfaceReports(t *testing.T) {
	t.Run("a payload has only listed keys and values", func(t *testing.T) {
		c, remote := control(true, true)
		denied, status, retries := protocol.ErrorCodePermissionDenied, uint16(429), uint64(2)
		driver := protocol.SearchDriverExa
		official, valid := OfficialTool("browser", "open_page")
		builtin, counted := BuiltinTool("read_session")
		if !valid || !counted {
			t.Fatalf("official %v, built-in %v", valid, counted)
		}
		// Names a user or a package chose never become a tool or feature name.
		for _, name := range []string{plantedName, plantedPath, "Read", "a b", "", strings.Repeat("a", 65)} {
			if tool, ok := BuiltinTool(name); ok {
				t.Errorf("built-in %q counted as %+v", name, tool)
			}
			if tool, ok := OfficialTool(name, "tool"); ok {
				t.Errorf("package %q counted as %+v", name, tool)
			}
			if tool, ok := OfficialTool("browser", name); ok {
				t.Errorf("package tool %q counted as %+v", name, tool)
			}
		}
		for name, reason := range UncountedBuiltins {
			if _, ok := BuiltinTool(name); ok || strings.TrimSpace(reason) == "" {
				t.Errorf("excluded built-in %q is counted (%v) or has no reason", name, ok)
			}
		}
		c.Observe(Turn("failed", protocol.WorkModeCode, true, &denied))
		for _, tool := range []Tool{builtin, official, MCPTool(), ThirdPartyTool()} {
			c.Observe(ToolCall(tool, "denied", &denied, 3*time.Millisecond))
		}
		c.Observe(ModelRequest(c.Model(protocol.ConversationProvider{Name: plantedName, Endpoint: "https://planted.invalid" + plantedPath, Api: protocol.ConversationModelApiResponses, Authentication: protocol.AuthenticationApiKey}, plantedSecret), "error", &status, &denied, &retries, Tokens{Input: 1, Output: 2, CacheRead: 3, CacheWrite: 4}))
		c.Observe(Search(&driver, "error", &denied, time.Millisecond))
		features := append(protocol.FeatureUses(), protocol.FeatureUsePagePluginOfficial+"browser")
		for _, feature := range features {
			observation, listed := c.FeatureUse(feature)
			if !listed {
				t.Fatalf("listed feature %q refused", feature)
			}
			c.Observe(observation)
			c.Observe(observation)
		}
		// Anything outside the closed list is refused, an unshipped or
		// user-named package included.
		for _, feature := range []protocol.FeatureUse{"", "page", "page.conversation ", "PAGE.CONVERSATION", protocol.FeatureUse(plantedPath),
			"settings.save." + plantedName, protocol.FeatureUsePagePluginOfficial, protocol.FeatureUsePagePluginOfficial + plantedName,
			protocol.FeatureUsePagePluginOfficial + "not-shipped", "page.plugin_page.third_party:browser"} {
			if _, listed := c.FeatureUse(feature); listed {
				t.Errorf("feature %q outside the closed list is counted", feature)
			}
		}
		remote.timers[len(remote.timers)-1]()
		usage, _ := remote.sent()

		common := []string{"event_id", "window_start", "window_end", "count", "installation_id", "channel", "application", "os", "arch", "release"}
		allowed := map[string][]string{
			"app_launches":    nil,
			"turns":           {"result", "mode", "subagent", "error_code"},
			"tool_calls":      {"tool", "result", "error_code", "elapsed_us_total", "elapsed_us_max"},
			"model_requests":  {"api", "authentication", "model", "result", "http_status_class", "error_code", "retries", "input", "output", "cache_read", "cache_write"},
			"search_requests": {"driver", "result", "error_code", "elapsed_us_total"},
			"feature_uses":    {"feature"},
		}
		tools, counts, tables := map[string]bool{}, map[string]float64{}, map[string]bool{}
		for _, body := range usage {
			assertClean(t, "usage payload", body)
			var sent struct {
				Table string
				Data  []map[string]any
			}
			if err := json.Unmarshal(body, &sent); err != nil {
				t.Fatal(err)
			}
			keys, known := allowed[sent.Table]
			if !known || !slices.Contains(metricTables, sent.Table) {
				t.Fatalf("unknown table %q", sent.Table)
			}
			tables[sent.Table] = true
			for _, row := range sent.Data {
				for key := range row {
					if !slices.Contains(common, key) && !slices.Contains(keys, key) {
						t.Errorf("%s row has unlisted key %q", sent.Table, key)
					}
				}
				switch sent.Table {
				case "tool_calls":
					tool := row["tool"].(map[string]any)
					for key := range tool {
						if key != "source" && key != "name" {
							t.Errorf("tool has unlisted key %q", key)
						}
					}
					name, _ := tool["name"].(string)
					tools[tool["source"].(string)+"/"+name] = true
				case "feature_uses":
					feature := protocol.FeatureUse(row["feature"].(string))
					if !slices.Contains(features, feature) {
						t.Errorf("feature %q is outside the closed list", feature)
					}
					counts[string(feature)] = row["count"].(float64)
				}
			}
		}
		if len(tables) != len(allowed) {
			t.Errorf("tables sent = %v, want one batch of each of %d", tables, len(allowed))
		}
		if want := map[string]bool{"builtin/read_session": true, "official/browser.open_page": true, "mcp/": true, "third_party/": true}; len(tools) != len(want) ||
			!tools["builtin/read_session"] || !tools["official/browser.open_page"] || !tools["mcp/"] || !tools["third_party/"] {
			t.Errorf("tools = %v, want %v", tools, want)
		}
		if len(counts) != len(features) {
			t.Errorf("%d feature rows, want %d", len(counts), len(features))
		}
		for feature, count := range counts {
			if count != 2 {
				t.Errorf("feature %q counted %v times, want 2", feature, count)
			}
		}
		c.Close()
	})

	t.Run("feature counters cannot crowd out the other tables", func(t *testing.T) {
		c, remote := control(false, true)
		for _, feature := range protocol.FeatureUses() {
			observation, _ := c.FeatureUse(feature)
			c.Observe(observation)
		}
		if len(protocol.FeatureUses())+1 > featureGroups {
			t.Fatalf("the closed list has %d identifiers, over the window's %d feature groups", len(protocol.FeatureUses()), featureGroups)
		}
		c.mu.Lock()
		features, others := c.window.features, len(c.window.groups)-c.window.features
		c.mu.Unlock()
		if features != len(protocol.FeatureUses()) || others != 1 {
			t.Fatalf("window holds %d feature groups and %d others", features, others)
		}
		c.Close()
		if usage, _ := remote.sent(); len(usage) != 2 {
			t.Fatalf("%d usage batches at close, want launches and features", len(usage))
		}
	})

	t.Run("a crash is reduced, kept and reported once", func(t *testing.T) {
		// The runtime keeps its own duplicated descriptor until the next
		// SetCrashOutput call; release it before TempDir cleanup runs.
		defer func() {
			if err := debug.SetCrashOutput(nil, debug.CrashOptions{}); err != nil {
				t.Error(err)
			}
		}()
		reduced := reduce([]byte(crashTrace))
		assertReduced(t, reduced)
		want := "panic.nil_dereference\n" +
			"goroutine 1 [running, locked to thread]\n" +
			"github.com/sailry/sailry-harness/internal/desktop/shell.(*App).View:231\nmain.run.func3:112\nmain.main:33\n\n" +
			"goroutine 42 [chan receive, 3 minutes]\n" +
			"github.com/sailry/sailry-harness/internal/node.(*Node).serve:300\n" +
			"created by github.com/sailry/sailry-harness/internal/node.Start:200\n"
		if string(reduced) != want {
			t.Fatalf("reduced trace:\n%s\nwant:\n%s", reduced, want)
		}
		for raw, class := range map[string]string{
			"panic: " + plantedSecret + "\n\ngoroutine 1 [running]:\nmain.main()\n\t/x/main.go:1 +0x1\n":       "panic.other",
			"fatal error: concurrent map writes\n\ngoroutine 7 [running]:\nmain.f()\n\t/x/main.go:2 +0x1\n":    "fatal.concurrent_map",
			"SIGQUIT: quit\nPC=0x1 m=0 sigcode=0\n\ngoroutine 1 [select]:\nmain.main()\n\t/x/main.go:3 +0x1\n": "signal",
		} {
			if got, _, _ := strings.Cut(string(reduce([]byte(raw))), "\n"); got != class {
				t.Errorf("class %q, want %q", got, class)
			}
		}

		for _, errors := range []bool{true, false} {
			directory := t.TempDir()
			if crash, err := CatchCrashes(directory); crash != nil || err != nil {
				t.Fatalf("a first launch found a crash: %v, %v", crash, err)
			}
			if err := os.WriteFile(filepath.Join(directory, crashOutput), []byte(crashTrace), 0o600); err != nil {
				t.Fatal(err)
			}
			crash, err := CatchCrashes(directory)
			if err != nil || crash == nil {
				t.Fatalf("crash %v, error %v", crash, err)
			}
			kept, err := os.ReadFile(filepath.Join(directory, CrashFile))
			if err != nil || string(kept) != want {
				t.Fatalf("kept trace %q, %v", kept, err)
			}
			if raw, err := os.ReadFile(filepath.Join(directory, crashOutput)); err != nil || len(raw) != 0 {
				t.Fatalf("the raw traceback survives its launch: %d bytes, %v", len(raw), err)
			}
			if again, err := CatchCrashes(directory); again != nil || err != nil {
				t.Fatalf("the crash is found twice: %v, %v", again, err)
			}
			c, remote := control(errors, false)
			c.ReportCrash(crash)
			c.ReportCrash(nil)
			c.Close()
			_, reports := remote.sent()
			if errors && len(reports) != 1 || !errors && len(reports) != 0 {
				t.Fatalf("error reports %v: %d crash reports", errors, len(reports))
			}
			for _, report := range reports {
				assertClean(t, "crash report", report)
				for _, part := range []string{`"interface.crash:panic.nil_dereference"`, `"function":"(*App).View"`, `"lineno":231`, `"level":"fatal"`, `"handled":false`, "goroutines.txt"} {
					if !bytes.Contains(report, []byte(part)) {
						t.Errorf("crash report lacks %s", part)
					}
				}
			}
		}
	})

	t.Run("a blocked interface is one trace and one report", func(t *testing.T) {
		for _, test := range []struct {
			name             string
			errors           bool
			suspended        bool
			hidden           bool
			traces, reported int
		}{
			{name: "reporting on", errors: true, traces: 1, reported: 1},
			{name: "reporting off", traces: 1},
			{name: "suspended", errors: true, suspended: true},
			{name: "no visible window", errors: true, hidden: true},
		} {
			t.Run(test.name, func(t *testing.T) {
				ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
				defer cancel()
				c, remote := control(test.errors, false)
				directory, clock := t.TempDir(), clockwork.NewFakeClock()
				var blocked atomic.Bool
				var pings atomic.Int64
				held := make(chan func(), 64)
				watchdog := NewWatchdog(WatchdogOptions{Directory: directory, Control: c, Clock: clock, Ping: func(done func()) {
					pings.Add(1)
					if blocked.Load() {
						held <- done
						return
					}
					done()
				}})
				// settle lets an answering interface answer: the heartbeat is
				// asked for on another goroutine, which the fake clock outruns.
				settle := func() {
					for {
						select {
						case done := <-held:
							done()
							continue
						default:
						}
						watchdog.mu.Lock()
						waiting := watchdog.waiting
						watchdog.mu.Unlock()
						if !waiting {
							return
						}
						time.Sleep(50 * time.Microsecond)
					}
				}
				tick := func(step time.Duration) {
					t.Helper()
					if err := clock.BlockUntilContext(ctx, 1); err != nil {
						t.Fatal("the watchdog stopped ticking")
					}
					clock.Advance(step)
					if err := clock.BlockUntilContext(ctx, 1); err != nil {
						t.Fatal("the watchdog stopped ticking")
					}
					if !blocked.Load() {
						settle()
					}
				}
				traces := func() int {
					if _, err := os.Stat(filepath.Join(directory, HangFile)); err != nil {
						return 0
					}
					return 1
				}
				watchdog.Visible(!test.hidden)
				watchdog.Suspended(test.suspended)
				watchdog.Touch()
				// An answering interface is never a hang, however long it runs.
				for range 8 {
					tick(watchInterval)
				}
				if traces() != 0 {
					t.Fatal("an answering interface left a trace")
				}
				blocked.Store(true)
				tick(watchInterval) // the heartbeat that is not answered
				for range int(HangAfter/watchInterval) - 1 {
					tick(watchInterval)
				}
				if traces() != 0 {
					t.Fatalf("a trace before %v without a heartbeat", HangAfter)
				}
				// The process stood still (sleep, a debugger): not a hang.
				tick(time.Hour)
				if traces() != 0 {
					t.Fatal("a stopped process counted as a hang")
				}
				for range int(HangAfter / watchInterval) {
					tick(watchInterval)
				}
				if traces() != test.traces {
					t.Fatalf("%d traces after %v without a heartbeat, want %d", traces(), HangAfter, test.traces)
				}
				if test.traces == 1 {
					kept, err := os.ReadFile(filepath.Join(directory, HangFile))
					if err != nil || !bytes.Contains(kept, []byte("goroutine ")) || !bytes.Contains(kept, []byte("telemetry.(*Watchdog).hang:")) {
						t.Fatalf("trace %q, %v", kept, err)
					}
					assertReduced(t, kept)
					if err := os.Remove(filepath.Join(directory, HangFile)); err != nil {
						t.Fatal(err)
					}
				}
				// The same hang goes on: nothing more is written or sent.
				for range 20 {
					tick(watchInterval)
				}
				if traces() != 0 {
					t.Fatal("one hang left a second trace")
				}
				// The interface answers again and settles: the heartbeat ends.
				blocked.Store(false)
				settle()
				running := func() bool {
					watchdog.mu.Lock()
					defer watchdog.mu.Unlock()
					return watchdog.running
				}
				for deadline := time.Now().Add(20 * time.Second); running(); {
					if time.Now().After(deadline) {
						t.Fatal("an idle interface keeps its heartbeat")
					}
					brief, stop := context.WithTimeout(ctx, 5*time.Millisecond)
					if clock.BlockUntilContext(brief, 1) == nil {
						clock.Advance(watchInterval)
						settle()
					}
					stop()
				}
				idle := pings.Load()
				clock.Advance(time.Hour)
				if pings.Load() != idle {
					t.Fatal("an idle interface was asked for a heartbeat")
				}
				watchdog.Close()
				c.Close()
				_, reports := remote.sent()
				if len(reports) != test.reported || traces() != 0 {
					t.Fatalf("%d reports, want %d", len(reports), test.reported)
				}
				for _, report := range reports {
					assertClean(t, "hang report", report)
					for _, part := range []string{`"interface.hang:unresponsive"`, `"handled":false`, "goroutines.txt", "telemetry.(*Watchdog).hang:"} {
						if !bytes.Contains(report, []byte(part)) {
							t.Errorf("hang report lacks %s", part)
						}
					}
				}
			})
		}
	})

	t.Run("nothing of a kind is kept or sent while its switch is off", func(t *testing.T) {
		c, remote := control(false, false)
		for _, feature := range protocol.FeatureUses() {
			observation, _ := c.FeatureUse(feature)
			c.Observe(observation)
		}
		official, _ := OfficialTool("browser", "open_page")
		c.Observe(ToolCall(official, "success", nil, time.Millisecond))
		if capture := c.Capture(); capture != nil {
			t.Fatal("consent was captured with usage metrics off")
		}
		c.mu.Lock()
		kept, timers := c.window, len(remote.timers)
		c.mu.Unlock()
		if kept != nil || timers != 0 {
			t.Fatalf("usage off kept a window (%v) or started %d timers", kept != nil, timers)
		}
		c.report("hang", reduce([]byte(crashTrace)))
		c.ReportCrash(&Crash{reduced: reduce([]byte(crashTrace))})
		c.mu.Lock()
		limits, flying := len(c.limits), c.inFlight
		c.mu.Unlock()
		c.Close()
		if usage, reports := remote.sent(); len(usage) != 0 || len(reports) != 0 || limits != 0 || flying != 0 {
			t.Fatalf("switches off: %d usage requests, %d reports, %d fingerprints, %d in flight", len(usage), len(reports), limits, flying)
		}

		// Turning usage off drops the feature counts already in the window.
		c, remote = control(false, true)
		observation, _ := c.FeatureUse(protocol.FeatureUseSearchPalette)
		c.Observe(observation)
		c.Configure(protocol.TelemetrySettings{Errors: false, Metrics: false, InstallationID: uuid.New()})
		c.Close()
		if usage, _ := remote.sent(); len(usage) != 0 {
			t.Fatalf("%d usage requests after usage was turned off", len(usage))
		}
	})
}
