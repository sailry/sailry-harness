//go:build integration

package live

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// systemReports stands in for the error-report endpoint.
type systemReports struct {
	mu     sync.Mutex
	bodies [][]byte
	// refuse answers 500 and counts the request instead of keeping it.
	refuse  bool
	refused int
	server  *httptest.Server
}

func newSystemReports(t *testing.T) *systemReports {
	reports := &systemReports{}
	reports.server = httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		body, _ := io.ReadAll(request.Body)
		reports.mu.Lock()
		refuse := reports.refuse
		if refuse {
			reports.refused++
		} else {
			reports.bodies = append(reports.bodies, body)
		}
		reports.mu.Unlock()
		if refuse {
			writer.WriteHeader(http.StatusInternalServerError)
			return
		}
		writer.WriteHeader(http.StatusOK)
	}))
	t.Cleanup(reports.server.Close)
	return reports
}

func (r *systemReports) dsn() string {
	return strings.Replace(r.server.URL, "http://", "http://public@", 1) + "/42"
}

// refusing switches the endpoint's refusal and reports how many requests
// it has refused.
func (r *systemReports) refusing(refuse bool) int {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.refuse = refuse
	return r.refused
}

func (r *systemReports) sent() [][]byte {
	r.mu.Lock()
	defer r.mu.Unlock()
	return append([][]byte(nil), r.bodies...)
}

// systemUsage stands in for the usage endpoint and the usage window's timer.
// Requests for any other host go on to the loopback fixtures.
type systemUsage struct {
	mu      sync.Mutex
	bodies  [][]byte
	headers []http.Header
	urls    []string
	pending []func()
	stops   int
}

func (u *systemUsage) RoundTrip(request *http.Request) (*http.Response, error) {
	if request.URL.Host != "api.telemetry.sh" {
		return http.DefaultTransport.RoundTrip(request)
	}
	body, _ := io.ReadAll(request.Body)
	u.mu.Lock()
	defer u.mu.Unlock()
	u.bodies, u.headers, u.urls = append(u.bodies, body), append(u.headers, request.Header.Clone()), append(u.urls, request.Method+" "+request.URL.String())
	return &http.Response{StatusCode: http.StatusOK, Body: http.NoBody, Request: request}, nil
}

func (u *systemUsage) timer(_ time.Duration, run func()) func() {
	u.mu.Lock()
	defer u.mu.Unlock()
	u.pending = append(u.pending, run)
	return func() {
		u.mu.Lock()
		defer u.mu.Unlock()
		u.stops++
	}
}

// counts are the usage requests sent, the timers started and stopped.
func (u *systemUsage) counts() (int, int, int) {
	u.mu.Lock()
	defer u.mu.Unlock()
	return len(u.bodies), len(u.pending), u.stops
}

// fire runs the latest timer once.
func (u *systemUsage) fire() {
	u.mu.Lock()
	run := u.pending[len(u.pending)-1]
	u.mu.Unlock()
	run()
}

// systemPower counts the system requests the Node takes and returns. No
// real system assertion is held.
type systemPower struct {
	mu                 sync.Mutex
	held               bool
	acquired, released int
}

func (p *systemPower) Supported() bool { return true }
func (p *systemPower) Held() bool {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.held
}
func (p *systemPower) Acquire() int64 {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.held, p.acquired = true, p.acquired+1
	return 0
}
func (p *systemPower) Release() int64 {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.held, p.released = false, p.released+1
	return 0
}
func (p *systemPower) counts() (bool, int, int) {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.held, p.acquired, p.released
}

// systemReadTool is the name under which a turn is offered the bundled
// files package's read_file.
var systemReadTool = operations.Alias("files", "read_file")

// systemModel is a scripted chat-completions endpoint: a request that
// is first answered with tool_search for read_file, then with a call of the
// offered read_file for secret.txt, then with the final text. hold makes the
// next request wait, so a test can look at the Node while a turn runs.
type systemModel struct {
	mu      sync.Mutex
	bodies  []string
	turn    int
	gate    chan struct{}
	arrived chan struct{}
	server  *httptest.Server
}

// hold stops the next request before its answer. The request closes the
// first channel when it arrives; closing the second lets it answer.
func (m *systemModel) hold() (<-chan struct{}, chan<- struct{}) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.gate, m.arrived = make(chan struct{}), make(chan struct{})
	return m.arrived, m.gate
}

const (
	systemPrompt   = "SECRET PROMPT ABOUT THE HARBOUR"
	systemFile     = "SECRET FILE CONTENT OF THE LEDGER"
	systemResponse = "SECRET RESPONSE FROM THE MODEL"
	systemKey      = "fixture-provider-key"
)

func newSystemModel(t *testing.T) *systemModel {
	model := &systemModel{}
	model.server = httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		data, _ := io.ReadAll(request.Body)
		body := string(data)
		model.mu.Lock()
		model.bodies = append(model.bodies, body)
		call := fmt.Sprintf("call_fixture_%d", model.turn)
		gate, arrived := model.gate, model.arrived
		model.gate, model.arrived = nil, nil
		model.mu.Unlock()
		if gate != nil {
			close(arrived)
			select {
			case <-gate:
			case <-request.Context().Done():
				return
			}
		}
		writer.Header().Set("Content-Type", "text/event-stream")
		chunk := func(delta, finish string) {
			fmt.Fprintf(writer, "data: {\"id\":\"fixture\",\"object\":\"chat.completion.chunk\",\"model\":\"fixture\",\"created\":1,\"choices\":[{\"index\":0,\"delta\":%s,\"finish_reason\":%s}]}\n\n", delta, finish)
		}
		discover := fmt.Sprintf("discover_fixture_%d", model.turnNow())
		tool := func(id, name, arguments string) {
			quoted, _ := json.Marshal(arguments)
			chunk(`{"role":"assistant","tool_calls":[{"index":0,"id":"`+id+`","type":"function","function":{"name":"`+name+`","arguments":`+string(quoted)+`}}]}`, "null")
			chunk(`{}`, `"tool_calls"`)
		}
		if !strings.Contains(body, discover) {
			tool(discover, "tool_search", `{"query":"select:`+systemReadTool+`"}`)
		} else if strings.Contains(body, `"name":"`+systemReadTool+`"`) && !strings.Contains(body, call) {
			tool(call, systemReadTool, `{"path":"secret.txt"}`)
		} else {
			chunk(`{"role":"assistant","content":"`+systemResponse+`"}`, "null")
			chunk(`{}`, `"stop"`)
		}
		fmt.Fprint(writer, "data: {\"id\":\"fixture\",\"object\":\"chat.completion.chunk\",\"model\":\"fixture\",\"created\":1,\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":4,\"total_tokens\":16}}\n\ndata: [DONE]\n\n")
	}))
	t.Cleanup(model.server.Close)
	return model
}

func (m *systemModel) turnNow() int {
	m.mu.Lock()
	defer m.mu.Unlock()
	return m.turn
}

func (m *systemModel) next() {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.turn++
}

func (m *systemModel) requests() []string {
	m.mu.Lock()
	defer m.mu.Unlock()
	return append([]string(nil), m.bodies...)
}

// systemEventMessage finds an error report's message.
var systemEventMessage = regexp.MustCompile(`"message":"[^"]*"`)

// bring scrolls the page until the single element of a role named name is
// inside the window, and returns it there.
func bring(w *terminalUI, role ui.Role, name string) ui.AccessInfo {
	w.t.Helper()
	settle(w, fmt.Sprintf("one %q on the page", name), systemUIWait, func() bool { return len(w.named(role, name)) == 1 })
	direction := float32(1)
	for range 60 {
		target := w.one(role, name)
		if target.Bounds.Y >= 60 && target.Bounds.Y+target.Bounds.H <= 960 {
			return target
		}
		if target.Bounds.Y < 60 {
			direction = -1
		} else {
			direction = 1
		}
		x, _ := center(target.Bounds)
		w.tester.Scroll(x, 500, 0, 60*direction)
		w.frame()
		if moved := w.one(role, name); moved.Bounds.Y == target.Bounds.Y {
			w.tester.Scroll(x, 500, 0, -60*direction)
			w.frame()
			if again := w.one(role, name); again.Bounds.Y == target.Bounds.Y {
				break
			}
		}
	}
	w.t.Fatalf("the %q of the page cannot be scrolled into view; elements: %s", name, w.elements())
	return ui.AccessInfo{}
}

// menuItem is the application menu's item with an ID.
func menuItem(items []*mygo.MenuItem, id string) *mygo.MenuItem {
	for _, item := range items {
		if item == nil {
			continue
		}
		if item.ID == id {
			return item
		}
		if found := menuItem(item.Submenu, id); found != nil {
			return found
		}
	}
	return nil
}

// TestSystemGeneralUI is the B13 acceptance of Settings › General's power
// and diagnostics groups and of Help › Logs: SGEN-002 (power), SGEN-011,
// SGEN-012 and SHELL-068, with the telemetry switches.
//
// Substitutions: the error-report endpoint is a loopback httptest server
// named by SAILRY_SENTRY_DSN through telemetry.Options.Lookup; the usage
// endpoint https://api.telemetry.sh/log is intercepted by
// telemetry.Options.Transport and never reached; the usage window's
// 15-minute timer is telemetry.Options.Timer, fired once by the test; the
// system's sleep assertion is a counting power.Backend, so no real
// assertion is held; the model is a scripted loopback chat-completions
// server whose first answer of a turn waits while the test reads that
// backend; the turn and the failing commands are sent by a second controller
// of the same Node, not typed into the composer; the system viewer of
// Help › Logs is the injected NativeFileActions.OpenLocal, so Finder is
// never opened.
func TestSystemGeneralUI(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 240*time.Second)
	defer cancel()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	profile := filepath.Join(root, "profile")
	closeLog, err := logging.Install(profile, "desktop")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(closeLog)
	reports, usage, backend, model := newSystemReports(t), &systemUsage{}, &systemPower{}, newSystemModel(t)
	desk := newSystemDesk(t, ctx, profile, node.Options{Power: backend, Telemetry: telemetry.Options{Transport: usage, Timer: usage.timer,
		Lookup: func(name string) string {
			return map[string]string{"SAILRY_SENTRY_DSN": reports.dsn(), "SAILRY_TELEMETRY_KEY": "usage-fixture-key"}[name]
		}}})
	provider, chosen := desk.provider(model.server.URL + "/v1")
	project := desk.project("Harbour ledger workspace")
	if err := os.WriteFile(filepath.Join(project.Path, "secret.txt"), []byte(systemFile+"\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if !slices.ContainsFunc(desk.setup.Snapshot().Plugins, func(entry protocol.PluginSummary) bool { return entry.Name == "files" }) {
		desk.do(protocol.Command{Kind: "install_bundled_plugin", Data: protocol.CommandInstallBundledPluginPayload{Name: "files"}})
	}
	if info, valid := detail[protocol.PluginInfo](desk.do(protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: "files"}}).Data); !valid {
		t.Fatal("the bundled files package was not returned")
	} else if !info.Summary.Enabled {
		desk.do(protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{Name: "files", ExpectedRevision: info.Summary.Revision, Enabled: true}})
	}
	session := desk.session(project, provider, chosen, "Harbour ledger notes")

	consent := func() protocol.TelemetrySettings {
		t.Helper()
		value, valid := detail[protocol.TelemetrySettings](desk.do(protocol.Command{Kind: "read_telemetry_settings"}).Data)
		if !valid {
			t.Fatal("telemetry settings were not returned")
		}
		return value
	}
	sleep := func() protocol.PowerSettings {
		t.Helper()
		value, valid := detail[protocol.PowerSettings](desk.do(protocol.Command{Kind: "read_power_settings"}).Data)
		if !valid {
			t.Fatal("power settings were not returned")
		}
		return value
	}
	// private is everything that must never leave the process.
	home, _ := os.UserHomeDir()
	private := []string{systemPrompt, systemFile, systemResponse, systemKey, "secret.txt", "Harbour ledger", profile, root, project.Path,
		session.ID.String(), desk.owner.ID().String(), provider.ID.String(), model.server.URL, "System fixture"}
	if home != "" {
		private = append(private, home)
	}
	clean := func(label string, body []byte) {
		t.Helper()
		for _, text := range private {
			if bytes.Contains(body, []byte(text)) {
				t.Fatalf("%s carries %q: %s", label, text, body)
			}
		}
	}
	// idle waits for the Node's short work to end and returns the requests
	// taken so far. Package commands are supervised tasks, so a window that
	// reads its packages takes and returns the request many times: the test
	// asserts what is held, and counts only where nothing may be taken.
	idle := func(label string) int {
		t.Helper()
		deadline := time.Now().Add(systemUIWait)
		for {
			held, acquired, released := backend.counts()
			if !held && acquired == released {
				return acquired
			}
			if time.Now().After(deadline) {
				t.Fatalf("%s: the idle Node holds the power request: held %v acquired %d released %d", label, held, acquired, released)
			}
			time.Sleep(10 * time.Millisecond)
		}
	}
	// work is a read refused at warning level, a turn with a stale revision
	// refused at error level and one completed turn that reads the secret
	// file. awake is whether the turn holds the power request while it runs.
	var finished protocol.TurnID
	work := func(label string, awake bool) {
		t.Helper()
		baseline := idle(label)
		if _, err := desk.setup.Do(ctx, protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: protocol.NewSessionID()}}); faultCode(err) != protocol.ErrorCodeNotFound {
			t.Fatalf("%s: reading a missing session gave %v", label, err)
		}
		stale, found := desk.current(session.ID)
		if !found {
			t.Fatalf("%s: the session is gone", label)
		}
		if _, err := desk.setup.Do(ctx, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: stale.Revision + 99, Message: protocol.ConversationInput{Text: systemPrompt}}}); faultCode(err) != protocol.ErrorCodeRevisionConflict {
			t.Fatalf("%s: a stale turn gave %v", label, err)
		}
		model.next()
		before := len(model.requests())
		current, found := desk.current(session.ID)
		if !found {
			t.Fatalf("%s: the session is gone", label)
		}
		arrived, release := model.hold()
		desk.do(protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: current.Revision, Message: protocol.ConversationInput{Text: systemPrompt}}})
		select {
		case <-arrived:
		case <-time.After(systemUIWait):
			t.Fatalf("%s: the turn did not reach the model within %s", label, systemUIWait)
		}
		// The turn waits for the model: it runs. With the switch on the
		// request stays held without a break; with it off none is taken.
		held, taken, returned := backend.counts()
		time.Sleep(300 * time.Millisecond)
		still, later, back := backend.counts()
		if awake && (!held || !still || taken != returned+1 || later != taken || back != returned) {
			t.Fatalf("%s: while the turn runs with Prevent sleep on: held %v then %v, acquired %d then %d, released %d then %d", label, held, still, taken, later, returned, back)
		}
		if !awake && (held || still || later != baseline) {
			t.Fatalf("%s: while the turn runs with Prevent sleep off: held %v then %v, acquired %d, was %d", label, held, still, later, baseline)
		}
		close(release)
		deadline := time.Now().Add(systemUIWait)
		for {
			now, _ := desk.current(session.ID)
			if run := now.Activity.Run; run != nil && run.Turn != finished && run.FinishedMs != nil {
				if run.Status != protocol.ConversationStatusCompleted || len(model.requests()) != before+3 {
					t.Fatalf("%s: the turn ended %s (%+v) after %d model requests; tools offered: %v; search: %q", label, run.Status, run.Error, len(model.requests())-before, regexp.MustCompile(`"function":\{"name":"[A-Za-z_]+"`).FindAllString(model.requests()[len(model.requests())-1], -1), regexp.MustCompile(`.{200}discover_fixture.{900}`).FindAllString(model.requests()[len(model.requests())-1], -1))
				}
				finished = run.Turn
				break
			}
			if time.Now().After(deadline) {
				t.Fatalf("%s: the turn did not finish within %s: %d model requests, activity %+v, run %+v", label, systemUIWait, len(model.requests())-before, now.Activity, now.Activity.Run)
			}
			time.Sleep(10 * time.Millisecond)
		}
		sent := model.requests()
		if last := sent[len(sent)-1]; !strings.Contains(last, systemFile) || !strings.Contains(sent[before], systemPrompt) {
			t.Fatalf("%s: the turn did not carry the prompt and the file to the model: %s", label, last)
		}
		// The turn is counted just after its run commits.
		time.Sleep(500 * time.Millisecond)
		// The completed turn returned what it held.
		if after := idle(label); awake == (after == baseline) {
			t.Fatalf("%s: after the turn %d requests were taken and returned, %d before it", label, after, baseline)
		}
	}
	open := func() *terminalUI {
		window := openTerminalWindow(t, ctx, desk.owner, profile, session, nil)
		showSection(window, settings.General)
		settle(window, "the diagnostics group loads", systemUIWait, func() bool {
			m := window.runtime.App.Settings
			return !m.Telemetry.Pending && !m.Power.Pending && m.Telemetry.Revision == consent().Revision && m.Power.Revision == sleep().Revision && shown(window, "Diagnostics and privacy") == 1
		})
		return window
	}
	flip := func(window *terminalUI, name, outcome string) {
		t.Helper()
		before, _ := toggled(window, name)
		window.clickOn(bring(window, ui.RoleSwitch, name))
		toast(window, outcome)
		if after, found := toggled(window, name); !found || after.Checked == before.Checked {
			t.Fatalf("the switch %q stayed %v", name, before.Checked)
		}
	}
	switches := func(window *terminalUI) (bool, bool, bool) {
		t.Helper()
		errors, a := toggled(window, "Send error reports")
		metrics, b := toggled(window, "Share anonymous usage")
		awake, c := toggled(window, "Prevent sleep")
		if !a || !b || !c {
			t.Fatalf("the General page lacks a switch; elements: %s", window.elements())
		}
		return errors.Checked, metrics.Checked, awake.Checked
	}

	// Step 1: a new profile reports, and with Prevent sleep on an idle Node
	// holds nothing.
	first := consent()
	if !first.Errors || !first.Metrics || !sleep().PreventSleep {
		t.Fatalf("a new profile: telemetry %+v, power %+v", first, sleep())
	}
	idle("a new profile")
	if _, timers, stops := usage.counts(); timers != 1 || stops != 0 {
		t.Fatalf("the launch opened %d usage windows and stopped %d", timers, stops)
	}
	window := open()
	if errors, metrics, awake := switches(window); !errors || !metrics || !awake {
		t.Fatalf("the page shows errors %v, metrics %v, prevent sleep %v", errors, metrics, awake)
	}
	t.Logf("step 1 passed: the page loaded the Node's settings: error reports on, anonymous usage on, prevent sleep on; the idle Node holds no power request and one usage window is open")

	// Step 2: the switch alone takes nothing; the request follows work.
	flip(window, "Prevent sleep", "Saved")
	off := idle("prevent sleep off")
	if sleep().PreventSleep {
		t.Fatalf("prevent sleep off: Node %+v", sleep())
	}
	window.close()
	window = open()
	if _, _, awake := switches(window); awake {
		t.Fatal("a new Runtime shows prevent sleep on after it was switched off")
	}
	// Off, the window's own package reads took nothing.
	if held, acquired, released := backend.counts(); held || acquired != off || released != off {
		t.Fatalf("with prevent sleep off: held %v acquired %d released %d, was %d", held, acquired, released, off)
	}
	flip(window, "Prevent sleep", "Saved")
	idle("prevent sleep on")
	if !sleep().PreventSleep {
		t.Fatalf("prevent sleep on: Node %+v", sleep())
	}
	t.Logf("step 2 passed: Prevent sleep off saved on the Node and a new Runtime showed it off, during which no request was taken; switched on again, the idle Node holds none")

	// SGEN-002: a save that fails, one whose outcome is unknown and one that
	// lost its revision each toast their own outcome, keep the switch as the
	// user left it and leave the Node's setting alone.
	stored := sleep()
	refusal := func(label, outcome string, want bool) {
		t.Helper()
		window.clickOn(bring(window, ui.RoleSwitch, "Prevent sleep"))
		toast(window, outcome)
		for _, other := range []string{"Saved", "Could not load or save power settings", "Save outcome is unknown; reload to check", "Settings changed; reload to continue"} {
			if other != outcome {
				noToast(window, other)
			}
		}
		if now, found := toggled(window, "Prevent sleep"); !found || now.Checked != want || window.runtime.App.Settings.Power.Draft != want {
			t.Fatalf("%s: the switch did not keep the draft %v", label, want)
		}
	}
	release := window.gate.refuse("save_power_settings", errors.New("the link closed"))
	refusal("a failed save", "Could not load or save power settings", false)
	release()
	// The outcome stays unknown only while the Node cannot be asked about the request.
	release = window.gate.refuse("save_power_settings", &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "the save may have been stored"})
	unreachable := window.gate.refuse("inspect_request", errors.New("the link closed"))
	refusal("an unknown outcome", "Save outcome is unknown; reload to check", true)
	release()
	unreachable()
	if now := sleep(); now != stored {
		t.Fatalf("the refused saves changed the Node: %+v from %+v", now, stored)
	}
	// Another controller saves while the window's save is on its way.
	release = window.gate.hold("save_power_settings")
	window.clickOn(bring(window, ui.RoleSwitch, "Prevent sleep"))
	settle(window, "the held save is pending", systemUIWait, func() bool { return window.runtime.App.Settings.Power.Pending })
	desk.do(protocol.Command{Kind: "save_power_settings", Data: stored})
	release()
	toast(window, "Settings changed; reload to continue")
	noToast(window, "Saved")
	if now, found := toggled(window, "Prevent sleep"); !found || now.Checked || window.runtime.App.Settings.Power.Draft {
		t.Fatal("the conflict replaced the draft")
	}
	if now := sleep(); !now.PreventSleep || now.Revision != stored.Revision+1 {
		t.Fatalf("the conflicting save changed the Node: %+v from %+v", now, stored)
	}
	settle(window, "the window reads the newer revision", systemUIWait, func() bool {
		m := window.runtime.App.Settings.Power
		return !m.Pending && m.Revision == stored.Revision+1 && m.Saved && !m.Draft
	})
	flip(window, "Prevent sleep", "Saved")
	if now := sleep(); !now.PreventSleep || now.Revision != stored.Revision+2 {
		t.Fatalf("the save after the conflict: %+v", now)
	}
	t.Logf("step 2a passed: a failed save, an unknown outcome and a revision conflict each showed their own toast and no other, kept the switch as the user left it and left the Node at prevent sleep on; the next save stored the draft over the reloaded revision %d", stored.Revision+1)

	// Step 3: both telemetry switches off survive a new Runtime.
	flip(window, "Send error reports", "Privacy settings saved")
	flip(window, "Share anonymous usage", "Privacy settings saved")
	if now := consent(); now.Errors || now.Metrics || now.Revision != first.Revision+2 || now.InstallationID != first.InstallationID {
		t.Fatalf("the Node's telemetry settings after both switches: %+v from %+v", now, first)
	}
	if _, timers, stops := usage.counts(); timers != 1 || stops != 1 {
		t.Fatalf("switching usage off left %d timers with %d stopped", timers, stops)
	}
	window.close()
	window = open()
	if errors, metrics, awake := switches(window); errors || metrics || !awake {
		t.Fatalf("a new Runtime shows errors %v, metrics %v, prevent sleep %v", errors, metrics, awake)
	}
	t.Logf("step 3 passed: both switches saved off on the Node (revision %d), the usage window's timer was stopped, and a new Runtime showed both off with prevent sleep on", consent().Revision)

	// A report of a failure logged while error reports were still on is not
	// part of what follows: it is counted and named here.
	earlier := reports.sent()
	base := len(earlier)
	for _, body := range earlier {
		clean("an earlier error report", body)
		t.Logf("while error reports were still on the Node reported: %s", systemEventMessage.Find(body))
	}

	// Step 4: with both off, failures and a completed turn send nothing.
	// Prevent sleep is on: the request is held while the turn runs and
	// returned when it completes.
	work("with the switches off", true)
	if sent, timers, _ := usage.counts(); len(reports.sent()) != base || sent != 0 || timers != 1 {
		t.Fatalf("with the switches off: %d reports, %d usage requests, %d timers: %s", len(reports.sent()), sent, timers, bytes.Join(reports.sent(), []byte("\n====\n")))
	}
	t.Logf("step 4 passed: with both switches off a refused read, a refused stale turn and a completed turn that read a file sent 0 error reports and 0 usage requests and started no timer; with Prevent sleep on the power request was held while the turn ran and returned when it completed")

	// Step 5: Send diagnostics delivers the filtered log once.
	window.clickOn(bring(window, ui.RoleButton, "Send diagnostics"))
	toast(window, "Diagnostics sent")
	sent := reports.sent()
	if len(sent) != base+1 {
		t.Fatalf("Send diagnostics delivered %d reports", len(sent)-base)
	}
	sent = sent[base:]
	clean("the diagnostics report", sent[0])
	lines := bytes.Split(bytes.TrimSuffix(sent[0], []byte("\n")), []byte("\n"))
	if len(lines) < 5 {
		t.Fatalf("the diagnostics envelope has %d lines: %s", len(lines), sent[0])
	}
	var requested struct {
		Message, Level string
		Tags           map[string]string
	}
	var attachment struct {
		Type, Filename string
		Length         int
		ContentType    string `json:"content_type"`
	}
	if err := json.Unmarshal(lines[2], &requested); err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(lines[3], &attachment); err != nil {
		t.Fatal(err)
	}
	if requested.Message != "runtime.diagnostics:requested" || requested.Level != "info" || requested.Tags["installation_id"] != first.InstallationID.String() ||
		attachment.Type != "attachment" || attachment.Filename != "diagnostics.jsonl" || attachment.ContentType != "application/x-ndjson" || attachment.Length == 0 {
		t.Fatalf("the diagnostics event %s with attachment %s", lines[2], lines[3])
	}
	public := map[string]bool{"level": true, "timestamp": true, "area": true, "phase": true, "operation": true, "code": true, "cause_code": true,
		"duration_us": true, "count": true, "elapsed_us": true, "app_version": true, "package": true}
	records, failures := 0, 0
	var failed []string
	for _, entry := range lines[4:] {
		if len(entry) == 0 {
			continue
		}
		var record map[string]any
		if err := json.Unmarshal(entry, &record); err != nil {
			t.Fatalf("attachment line %q: %v", entry, err)
		}
		for key := range record {
			if !public[key] {
				t.Fatalf("the attachment carries %q: %s", key, entry)
			}
		}
		records++
		if record["phase"] == "failed" {
			failed = append(failed, string(entry))
		}
		if record["phase"] == "failed" && record["operation"] == "submit_turn" && record["code"] == "revision_conflict" {
			failures++
		}
	}
	current, err := logging.CurrentFile()
	if err != nil || current != filepath.Join(profile, "logs", "desktop", "current.log") {
		t.Fatalf("the current log is %q, %v", current, err)
	}
	written, err := os.ReadFile(current)
	if err != nil {
		t.Fatal(err)
	}
	if records < 4 || failures != 1 || !bytes.Contains(written, []byte("session="+session.ID.String())) || bytes.Contains(written, []byte("SECRET")) {
		t.Fatalf("the attachment has %d records and %d stale-turn failures; failed records: %v; the log file names the session: %v", records, failures, failed, bytes.Contains(written, []byte("session="+session.ID.String())))
	}
	t.Logf("step 5 passed: Send diagnostics showed Diagnostics sent and delivered one envelope whose diagnostics.jsonl attachment holds %d records of %s reduced to public fields, the refused stale turn among them, while the file itself names the session", records, current)

	// SGEN-012: an endpoint that refuses the report is the failure toast,
	// after one request, and the next press sends again.
	reports.refusing(true)
	window.clickOn(bring(window, ui.RoleButton, "Send diagnostics"))
	toast(window, "Could not confirm diagnostic delivery")
	noToast(window, "Diagnostics sent")
	if refused := reports.refusing(false); refused != 1 || len(reports.sent()) != base+1 {
		t.Fatalf("the refused diagnostics made %d requests and delivered %d reports", refused, len(reports.sent())-base-1)
	}
	settle(window, "Send diagnostics is enabled again", systemUIWait, func() bool {
		return !window.runtime.App.Settings.Telemetry.Pending && !bring(window, ui.RoleButton, "Send diagnostics").Disabled
	})
	t.Logf("step 5 also passed: with the endpoint answering 500 Send diagnostics made one request and showed Could not confirm diagnostic delivery")

	// Step 6: Reset gives a new anonymous identity.
	before := consent()
	window.clickOn(bring(window, ui.RoleButton, "Reset"))
	toast(window, "Anonymous ID reset")
	reset := consent()
	if reset.InstallationID == before.InstallationID || reset.Revision != before.Revision+1 || reset.Errors || reset.Metrics {
		t.Fatalf("reset gave %+v from %+v", reset, before)
	}
	window.clickOn(bring(window, ui.RoleButton, "Send diagnostics"))
	toast(window, "Diagnostics sent")
	sent = reports.sent()[base:]
	if len(sent) != 2 || !bytes.Contains(sent[1], []byte(reset.InstallationID.String())) || bytes.Contains(sent[1], []byte(before.InstallationID.String())) {
		t.Fatalf("after the reset %d reports were sent and the second names the new identity: %v", len(sent), len(sent) == 2 && bytes.Contains(sent[1], []byte(reset.InstallationID.String())))
	}
	clean("the second diagnostics report", sent[1])
	t.Logf("step 6 passed: Reset showed Anonymous ID reset, the Node stored a new identity at revision %d with both switches still off, and the next diagnostics carried only the new identity", reset.Revision)

	// Step 7: with both on, one failure is one report and the window
	// flushes once.
	flip(window, "Send error reports", "Privacy settings saved")
	flip(window, "Share anonymous usage", "Privacy settings saved")
	if now := consent(); !now.Errors || !now.Metrics || now.InstallationID != reset.InstallationID {
		t.Fatalf("the Node's telemetry settings after both switches on: %+v", now)
	}
	window.close()
	window = open()
	defer window.close()
	if errors, metrics, _ := switches(window); !errors || !metrics {
		t.Fatalf("a new Runtime shows errors %v, metrics %v after both were switched on", errors, metrics)
	}
	// Prevent sleep off: the same turn takes no request.
	flip(window, "Prevent sleep", "Saved")
	work("with the switches on", false)
	flip(window, "Prevent sleep", "Saved")
	settle(window, "the error report arrives", systemUIWait, func() bool { return len(reports.sent()) >= base+3 })
	sent = reports.sent()[base:]
	if len(sent) != 3 {
		t.Fatalf("with error reports on %d reports arrived, want one", len(sent)-2)
	}
	clean("the error report", sent[2])
	lines = bytes.Split(sent[2], []byte("\n"))
	var event map[string]json.RawMessage
	if err := json.Unmarshal(lines[2], &event); err != nil {
		t.Fatal(err)
	}
	allowed := map[string]bool{"event_id": true, "timestamp": true, "level": true, "logger": true, "platform": true, "message": true, "release": true, "environment": true,
		"fingerprint": true, "tags": true, "sdk": true, "contexts": true, "exception": true, "breadcrumbs": true}
	for key := range event {
		if !allowed[key] && !(key == "user" && string(event[key]) == "{}") {
			t.Fatalf("the error report carries %q: %s", key, event[key])
		}
	}
	var contexts map[string]map[string]any
	var tags map[string]string
	var crumbs []struct{ Data map[string]any }
	_ = json.Unmarshal(event["contexts"], &contexts)
	_ = json.Unmarshal(event["tags"], &tags)
	_ = json.Unmarshal(event["breadcrumbs"], &crumbs)
	fields := contexts["sailry"]
	if string(event["message"]) != `"command.submit_turn:revision_conflict"` || string(event["level"]) != `"error"` || len(contexts) != 1 ||
		len(tags) != 5 || tags["installation_id"] != reset.InstallationID.String() || tags["application"] != "desktop" {
		t.Fatalf("the error report is %s", lines[2])
	}
	for key := range fields {
		if !public[key] {
			t.Fatalf("the error report's fields carry %q: %s", key, event["contexts"])
		}
	}
	for _, crumb := range crumbs {
		for key := range crumb.Data {
			if !public[key] {
				t.Fatalf("a breadcrumb carries %q: %s", key, event["breadcrumbs"])
			}
		}
	}
	if count, timers, _ := usage.counts(); count != 0 || timers != 2 {
		t.Fatalf("before the window ends: %d usage requests, %d timers", count, timers)
	}
	usage.fire()
	usage.mu.Lock()
	bodies, headers, urls := usage.bodies, usage.headers, usage.urls
	usage.mu.Unlock()
	rows := map[string]bool{"event_id": true, "window_start": true, "window_end": true, "count": true, "installation_id": true, "channel": true, "application": true,
		"os": true, "arch": true, "release": true, "result": true, "mode": true, "subagent": true, "error_code": true, "tool": true, "elapsed_us_total": true, "elapsed_us_max": true,
		"api": true, "authentication": true, "model": true, "http_status_class": true, "retries": true, "input": true, "output": true, "cache_read": true, "cache_write": true}
	tables := map[string]int{}
	turns := 0
	for index, body := range bodies {
		clean("the usage request", body)
		if urls[index] != "POST https://api.telemetry.sh/log" || headers[index].Get("Authorization") != "Bearer usage-fixture-key" {
			t.Fatalf("usage request %s with %v", urls[index], headers[index])
		}
		var batch struct {
			Table string
			Data  []map[string]any
		}
		if err := json.Unmarshal(body, &batch); err != nil {
			t.Fatal(err)
		}
		tables[batch.Table]++
		for _, row := range batch.Data {
			for key := range row {
				if !rows[key] {
					t.Fatalf("a %s row carries %q: %s", batch.Table, key, body)
				}
			}
			if row["installation_id"] != reset.InstallationID.String() {
				t.Fatalf("a %s row names the identity %v", batch.Table, row["installation_id"])
			}
			if batch.Table == "turns" && row["result"] == "completed" && row["count"] == float64(1) && row["mode"] == "code" {
				turns++
			}
			if batch.Table == "model_requests" && row["model"] != "custom" {
				t.Fatalf("a model request names the model %v", row["model"])
			}
		}
	}
	if turns != 1 || tables["turns"] != 1 || tables["model_requests"] != 1 || len(tables) != len(bodies) {
		t.Fatalf("the flush sent %d requests for tables %v with %d completed turns", len(bodies), tables, turns)
	}
	if len(reports.sent()) != base+3 {
		t.Fatalf("%d reports after the flush", len(reports.sent())-base)
	}
	t.Logf("step 7 passed: with Prevent sleep off the turn took no power request; with both switches on (shown on by a new Runtime) the same work sent exactly one error report, command.submit_turn:revision_conflict, and one firing of the usage window's timer sent %d requests, one per table %v, with one completed turn; none carried the prompt, the file, the response, the key, a path, a name or a local identifier", len(bodies), tables)

	// Step 8: Help › Logs is enabled in live mode and names the log folder.
	app := window.runtime.App
	var chosen2 []string
	send := func(id string) { chosen2 = append(chosen2, id) }
	if item := menuItem(app.Menu("en", send), "help.logs"); item == nil || !item.Disabled {
		t.Fatalf("Help › Logs without a log folder: %+v", item)
	}
	var opened []string
	window.runtime.SetNativeFileActions(NativeFileActions{OpenLocal: func(path string) { opened = append(opened, path) }})
	window.runtime.SetLogFolder(logging.Directory())
	item := menuItem(app.Menu("en", send), "help.logs")
	if item == nil || item.Disabled || item.Click == nil || item.Label != "Logs" {
		t.Fatalf("Help › Logs with a log folder: %+v", item)
	}
	item.Click(item, nil)
	if len(chosen2) != 1 || chosen2[0] != "help.logs" {
		t.Fatalf("the menu item sent %v", chosen2)
	}
	app.Do(chosen2[0])
	window.frame()
	folder := filepath.Join(profile, "logs")
	if info, err := os.Stat(filepath.Join(folder, "desktop", "current.log")); len(opened) != 1 || opened[0] != folder || err != nil || info.Size() == 0 {
		t.Fatalf("Help › Logs opened %v, want %s; current log: %v", opened, folder, err)
	}
	noToast(window, "Could not open the file")
	t.Logf("step 8 passed: Help › Logs is disabled without a log folder and enabled with one; choosing it handed %s, which holds desktop/current.log, to the injected viewer once", folder)

	// Step 9: a stopped Node holds no power request.
	if err := desk.owner.Stop(context.Background()); err != nil {
		t.Fatal(err)
	}
	if held, acquired, released := backend.counts(); held || acquired != released {
		t.Fatalf("after the Node stopped: held %v acquired %d released %d", held, acquired, released)
	}
	if count, _, _ := usage.counts(); len(reports.sent()) != base+3 || count != len(bodies) {
		t.Fatalf("stopping the Node sent more: %d reports, %d usage requests", len(reports.sent()), count)
	}
	t.Logf("step 9 passed: the stopped Node holds no power request and returned every one it took, and stopping sent nothing further")
}
