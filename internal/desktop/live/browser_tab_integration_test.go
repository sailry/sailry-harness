//go:build integration

package live

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestLiveBrowserTabIntegration drives the panel's browser tab of the real
// App over a real temporary Node (KEY-015, PANEL-041 to PANEL-045, PANEL-048). The headless tester hosts no WebView and MyGo exports no
// fake of one, so the page behind the tab is a fake history that writes the
// tab's address and history flags as the WebView callbacks of browserLoad do.
// Everything else is production: the shortcut, the chrome with its loading,
// failed and selecting states and its menu, the address policy, the Node's browser call routing and the adapter's tab planning.
// The real WebView history, the main-thread hop of executeBrowserCall and the
// page script are covered by the browserprobe program.
func TestLiveBrowserTabIntegration(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 90*time.Second)
	defer cancel()
	profile := filepath.Join(t.TempDir(), "profile")
	owner, err := node.Start(ctx, profile)
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Stop(context.Background())
	controller := client.New(link.NewLocal(owner.ID(), owner.ID(), owner))
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	do := func(command protocol.Command) protocol.Output {
		t.Helper()
		output, err := controller.Do(ctx, command)
		if err != nil {
			t.Fatalf("%s: %v", command.Kind, err)
		}
		return output
	}
	provider, model := fixtureProvider("Browser fixture")
	secret := protocol.NewSecret("fixture-key")
	do(protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
	project, valid := detail[protocol.Project](do(protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Browser workspace", Path: t.TempDir(), Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"},
	}}).Data)
	if !valid {
		t.Fatal("registered project was not returned")
	}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Effort = provider.ID, model.ID, model.DefaultEffort
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionFull
	create := func() protocol.Session {
		session, valid := detail[protocol.Session](do(protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data)
		if !valid {
			t.Fatal("created session was not returned")
		}
		return session
	}
	shown, other := create(), create()

	runtime, err := NewWithPreferences(ctx, controller, preferences.Open(filepath.Join(profile, "desktop", "preferences.json")))
	if err != nil {
		t.Fatal(err)
	}
	defer runtime.Close()
	// A window without native content: NewWebView fails on it, so no page is
	// ever made. The serving slot is taken so that the test serves the Node's
	// browser calls itself: executeBrowserCall waits for MyGo's main loop,
	// which no test runs.
	runtime.SetWebViewWindow(new(mygo.Window))
	runtime.browser.available = true
	runtime.browser.serving[runtime] = func() {}
	production := *runtime.App.Panel.BrowserActions
	type history struct {
		entries []string
		at      int
	}
	histories := map[string]*history{}
	reloads := 0
	sync := func(id string) {
		tab, past := runtime.browser.tabs[id], histories[id]
		tab.url, tab.back, tab.forward = past.entries[past.at], past.at > 0, past.at < len(past.entries)-1
		// As OnDidNavigate and OnDidFinishLoad: the page is there and loaded.
		tab.loading, tab.failure = false, ""
	}
	actions := runtime.App.Panel.BrowserActions
	actions.Navigate = func(id, address string) {
		production.Navigate(id, address)
		// After the production load took the address, as OnDidNavigate.
		runtime.enqueue(func(*ui.Context) {
			tab := runtime.browser.tabs[id]
			if tab == nil || tab.url != address {
				return
			}
			past := histories[id]
			if past == nil {
				past = &history{at: -1}
				histories[id] = past
			}
			past.entries = append(past.entries[:past.at+1], address)
			past.at++
			sync(id)
		})
	}
	actions.Back = func(id string) { histories[id].at--; sync(id) }
	actions.Forward = func(id string) { histories[id].at++; sync(id) }
	actions.Reload = func(id string) {
		reloads++
		production.Reload(id)
		// After the production reload, as the page's events.
		runtime.enqueue(func(*ui.Context) { sync(id) })
	}

	runtime.OpenSession(shown.ID.String())
	tester := ui.NewTester(runtime.App.View, 1440, 1000)
	tester.SetLocale("en")
	type job struct {
		call  protocol.BrowserCall
		reply chan protocol.BrowserResult
	}
	jobs := make(chan job)
	served := make(chan struct{})
	serveCtx, stop := context.WithCancel(ctx)
	go func() {
		defer close(served)
		_ = controller.ServeBrowser(serveCtx, func(ctx context.Context, call protocol.BrowserCall) protocol.BrowserResult {
			reply := make(chan protocol.BrowserResult, 1)
			select {
			case jobs <- job{call, reply}:
				return <-reply
			case <-ctx.Done():
				return browserFailure(protocol.ErrorCodeCancelled, "browser is closed")
			}
		})
	}()
	defer func() { stop(); <-served }()
	var routed []protocol.BrowserCall
	// wait builds frames, and plans the routed browser calls between them as
	// the main thread does, until ready, for at most 15 seconds.
	wait := func(label string, ready func() bool) {
		t.Helper()
		deadline := time.Now().Add(15 * time.Second)
		for {
			tester.Advance(16 * time.Millisecond)
			tester.Frame()
			select {
			case next := <-jobs:
				routed = append(routed, next.call)
				action, err := json.Marshal(next.call.Action)
				var request browserRequest
				if err != nil || json.Unmarshal(action, &request) != nil {
					t.Fatalf("%s: routed action does not decode: %s", label, action)
				}
				plan := runtime.planBrowserCall(controller.Target(), next.call.Session.String(), request)
				if plan.result == nil {
					t.Fatalf("%s: %s needs a page, which this test has none of", label, request.Action)
				}
				next.reply <- *plan.result
			default:
			}
			if ready() {
				return
			}
			if time.Now().After(deadline) || ctx.Err() != nil {
				t.Fatalf("%s did not settle; panel tabs: %v; visible text: %q", label, runtime.App.Panel.Tabs, tester.Texts())
			}
			time.Sleep(5 * time.Millisecond)
		}
	}
	element := func(role ui.Role, name string) ui.AccessInfo {
		t.Helper()
		var found []ui.AccessInfo
		for _, entry := range tester.AccessAll() {
			if entry.Role == role && entry.Name == name && entry.Bounds.W > 0 && entry.Bounds.H > 0 {
				found = append(found, entry)
			}
		}
		if len(found) != 1 {
			t.Fatalf("%d elements named %q with role %v; visible text: %q", len(found), name, role, tester.Texts())
		}
		return found[0]
	}
	click := func(entry ui.AccessInfo) {
		tester.ClickAt(entry.Bounds.X+entry.Bounds.W/2, entry.Bounds.Y+entry.Bounds.H/2)
	}
	browserTabs := func() []string {
		var ids []string
		for _, tab := range runtime.App.Panel.Tabs {
			if tab.Kind == panel.Browser {
				ids = append(ids, tab.ID)
			}
		}
		return ids
	}
	// chrome asserts the toolbar's enabled state and the address field.
	chrome := func(label, address string, back, forward, reload bool) {
		t.Helper()
		wait(label, func() bool {
			for _, entry := range tester.AccessAll() {
				if entry.Role == ui.RoleTextField && entry.Name == "Search or enter an address" {
					return entry.Value == address
				}
			}
			return false
		})
		for name, enabled := range map[string]bool{"Back": back, "Forward": forward, "Reload": reload} {
			if got := !element(ui.RoleButton, name).Disabled; got != enabled {
				t.Fatalf("%s: %s enabled = %v, want %v", label, name, got, enabled)
			}
		}
	}
	wait("session workspace", func() bool {
		return runtime.App.Sidebar.Selected == shown.ID.String() && runtime.workspace != nil && runtime.workspace.selected == shown.Worktree
	})

	// KEY-015: ⌘T opens a Browser tab.
	if tabs := browserTabs(); len(tabs) != 0 {
		t.Fatalf("browser tabs before the shortcut: %v", tabs)
	}
	tester.Key(ui.Cmd, ui.KeyT)
	wait("⌘T opens a Browser tab", func() bool {
		return len(browserTabs()) == 1 && runtime.App.Panel.Active.Kind == panel.Browser && runtime.App.PanelOpen
	})
	id := runtime.App.Panel.Active.ID
	chrome("a new tab", "", false, false, false)
	// Nothing to reload yet.
	tester.Key(ui.Cmd, ui.KeyR)
	wait("⌘R on an empty tab", func() bool { return true })
	if reloads != 0 {
		t.Fatalf("⌘R reloaded an empty tab %d times", reloads)
	}

	t.Logf("KEY-015 passed: ⌘T opened browser tab %s with an empty address and Back, Forward and Reload disabled", id)

	// PANEL-045: Enter loads the address, as https:// when it names a host
	// without a scheme and as a web search when it names none.
	enter := func(address string) {
		click(element(ui.RoleTextField, "Search or enter an address"))
		tester.Key(ui.Cmd, ui.KeyA)
		tester.Type(address)
		tester.Key(0, ui.KeyEnter)
	}
	enter("example.com")
	chrome("the first address", "https://example.com", false, false, true)
	if tab := runtime.browser.tabs[id]; tab == nil || tab.url != "https://example.com" || tab.conversation != shown.ID.String() || tab.node != controller.Target() {
		t.Fatalf("the tab after the first address: %+v", tab)
	}
	// An address the browser must not load is refused before any page.
	enter("file:///etc/passwd")
	wait("a refused address", func() bool { return true })
	if tab := runtime.browser.tabs[id]; tab.url != "https://example.com" || len(histories[id].entries) != 1 {
		t.Fatalf("a file address was loaded: %q, history %v", tab.url, histories[id].entries)
	}
	// Words are a web search, which the address field shows.
	if address, ok := panel.BrowserDestination("two words"); !ok || address != "https://www.google.com/search?q=two+words" {
		t.Fatalf("words go to %q", address)
	}
	enter("http://example.org/next")
	chrome("the second address", "http://example.org/next", true, false, true)

	t.Logf("PANEL-045 passed: Enter loaded https://example.com for example.com, refused a file address and loaded %v", histories[id].entries)

	// PANEL-041, PANEL-042: Back and Forward follow the history.
	click(element(ui.RoleButton, "Forward"))
	wait("a disabled Forward", func() bool { return true })
	if histories[id].at != 1 {
		t.Fatal("a disabled Forward went forward")
	}
	click(element(ui.RoleButton, "Back"))
	chrome("after Back", "https://example.com", false, true, true)
	click(element(ui.RoleButton, "Back"))
	wait("a disabled Back", func() bool { return true })
	if histories[id].at != 0 {
		t.Fatal("a disabled Back went back")
	}
	click(element(ui.RoleButton, "Forward"))
	chrome("after Forward", "http://example.org/next", true, false, true)

	t.Log("PANEL-041, PANEL-042 passed: Back and Forward were enabled only with history behind and ahead and moved the address")

	// PANEL-043, PANEL-044: Reload and ⌘R reload the page, keeping the history.
	click(element(ui.RoleButton, "Reload"))
	wait("Reload", func() bool { return reloads == 1 })
	tester.Key(ui.Cmd, ui.KeyR)
	wait("⌘R", func() bool { return reloads == 2 })
	chrome("after reloading", "http://example.org/next", true, false, true)
	if entries := histories[id].entries; len(entries) != 2 {
		t.Fatalf("reloading changed the history: %v", entries)
	}

	t.Logf("PANEL-043, PANEL-044 passed: the Reload button and ⌘R reloaded the page %d times", reloads)

	// The page's state in the chrome. The fake history finishes a load at
	// once, so the states are the tab's, set as the page's events set them.
	named := func(role ui.Role, name string) []ui.AccessInfo {
		var found []ui.AccessInfo
		for _, entry := range tester.AccessAll() {
			if entry.Role == role && entry.Name == name && entry.Bounds.W > 0 && entry.Bounds.H > 0 {
				found = append(found, entry)
			}
		}
		return found
	}
	bar := func() bool {
		for _, entry := range tester.AccessAll() {
			if entry.Role == ui.RoleProgress && entry.Bounds.H > 0 && entry.Bounds.H <= 2 {
				return true
			}
		}
		return false
	}
	page := runtime.browser.tabs[id]
	if bar() || len(named(ui.RoleButton, "Stop")) != 0 || element(ui.RoleButton, "Select element").Disabled {
		t.Fatal("a loaded page shows a progress bar or Stop, or offers no element selection")
	}
	page.begin()
	wait("a loading page", func() bool {
		return bar() && len(named(ui.RoleButton, "Stop")) == 1 && len(named(ui.RoleButton, "Reload")) == 0
	})
	if !element(ui.RoleButton, "Select element").Disabled {
		t.Fatal("Select element is enabled while the page loads")
	}
	click(element(ui.RoleButton, "Stop"))
	wait("Stop", func() bool { return !page.loading && !bar() && len(named(ui.RoleButton, "Reload")) == 1 })
	page.loading, page.failure = false, "browser_load_failed"
	wait("a failed page", func() bool {
		return tester.HasText("Page failed to load") && len(named(ui.RoleButton, "Reload")) == 2 && !bar()
	})
	if !element(ui.RoleButton, "Select element").Disabled {
		t.Fatal("Select element is enabled on a failed page")
	}
	// The body's Reload, under the toolbar's, loads the address again.
	retry := named(ui.RoleButton, "Reload")
	if retry[0].Bounds.Y > retry[1].Bounds.Y {
		retry[0], retry[1] = retry[1], retry[0]
	}
	click(retry[1])
	wait("Reload on a failed page", func() bool { return reloads == 3 })
	page.loading, page.failure = false, ""
	// A selection under way shows as Cancel selection, which ends it.
	page.pick = &browserPick{token: "probe", cancel: func() {}}
	wait("a selecting page", func() bool { return len(named(ui.RoleButton, "Cancel selection")) == 1 })
	click(element(ui.RoleButton, "Cancel selection"))
	wait("Cancel selection", func() bool { return page.pick == nil && len(named(ui.RoleButton, "Select element")) == 1 })
	// The menu lists the web inspector and the settings; there is one
	// control that opens a tab, the header's.
	if len(named(ui.RoleButton, "New tab")) != 0 {
		t.Fatal("the toolbar has a New tab button beside the header's +")
	}
	click(element(ui.RoleMenuButton, "More actions"))
	var items []string
	wait("the browser's menu", func() bool {
		items = items[:0]
		for _, entry := range tester.AccessAll() {
			if entry.Role == ui.RoleMenuItem {
				items = append(items, entry.Name)
			}
		}
		return len(items) == 2
	})
	if !strings.HasPrefix(items[0], "Web inspector") || !strings.HasPrefix(items[1], "Settings") {
		t.Fatalf("the browser's menu lists %q", items)
	}
	tester.Key(0, ui.KeyEscape)
	wait("the menu closed", func() bool {
		for _, entry := range tester.AccessAll() {
			if entry.Role == ui.RoleMenuItem {
				return false
			}
		}
		return true
	})
	chrome("after the chrome states", "http://example.org/next", true, false, true)

	t.Log("the chrome passed: a loading page showed the bar and Stop, a loaded one Reload, a failed one its error and Reload, a selecting one Cancel selection; the menu listed the web inspector and Settings")

	// PANEL-048: the Node routes a session's use_browser to this controller,
	// whose adapter addresses the tabs of that session only, and the result
	// returns through complete_browser.
	call := func(label string, session protocol.Session, action protocol.BrowserAction) (map[string]any, error) {
		t.Helper()
		type outcome struct {
			output protocol.Output
			err    error
		}
		done := make(chan outcome, 1)
		go func() {
			output, err := controller.Do(ctx, protocol.Command{Kind: "use_browser", Data: protocol.CommandUseBrowserPayload{
				Session: session.ID, Worktree: session.Worktree, Action: action,
			}})
			done <- outcome{output, err}
		}()
		var result *outcome
		before := len(routed)
		wait(label, func() bool {
			select {
			case got := <-done:
				result = &got
			default:
			}
			return result != nil
		})
		if len(routed) != before+1 || routed[before].Session != session.ID || routed[before].Action.Kind != action.Kind {
			t.Fatalf("%s: routed calls %+v", label, routed[before:])
		}
		if result.err != nil {
			return nil, result.err
		}
		if result.output.Kind != "browser" {
			t.Fatalf("%s: output kind %q", label, result.output.Kind)
		}
		encoded, err := json.Marshal(result.output.Data)
		var value map[string]any
		if err != nil || json.Unmarshal(encoded, &value) != nil {
			t.Fatalf("%s: result %s: %v", label, encoded, err)
		}
		return value, nil
	}
	use := func(label string, session protocol.Session, action protocol.BrowserAction) map[string]any {
		t.Helper()
		value, err := call(label, session, action)
		if err != nil {
			t.Fatalf("%s: %v", label, err)
		}
		return value
	}
	// refused is a call the adapter settles with a fault.
	refused := func(label string, session protocol.Session, action protocol.BrowserAction, code protocol.ErrorCode, message string) {
		t.Helper()
		value, err := call(label, session, action)
		var fault protocol.Fault
		if pointer := (*protocol.Fault)(nil); errors.As(err, &pointer) {
			fault = *pointer
		} else {
			errors.As(err, &fault)
		}
		if err == nil || fault.Code != code || fault.Message != message {
			t.Fatalf("%s: %v, %v; want %s %q", label, value, err, code, message)
		}
	}
	number, _ := strconv.ParseUint(id, 10, 64)
	listed := func(label string, session protocol.Session) []map[string]any {
		t.Helper()
		raw, _ := use(label, session, protocol.BrowserAction{Kind: "tabs"})["tabs"].([]any)
		var tabs []map[string]any
		for _, entry := range raw {
			tabs = append(tabs, entry.(map[string]any))
		}
		return tabs
	}
	if tabs := listed("the shown session's tabs", shown); len(tabs) != 1 || tabs[0]["id"] != float64(number) || tabs[0]["url"] != "http://example.org/next" {
		t.Fatalf("the shown session's tabs: %v", tabs)
	}
	if tabs := listed("the other session's tabs", other); len(tabs) != 0 {
		t.Fatalf("the other session sees the shown session's tab: %v", tabs)
	}
	// The other session's agent gets a tab of its own, behind the shown one.
	refused("the other session reads", other, protocol.BrowserAction{Kind: "read", Data: protocol.DefaultBrowserActionReadPayload()},
		protocol.ErrorCodeNotFound, "browser tab is empty; navigate to a URL before reading or interacting")
	tabs := browserTabs()
	if len(tabs) != 2 || tabs[0] != id || runtime.App.Panel.Active.ID != id {
		t.Fatalf("panel tabs after the other session's call: %v, active %v", tabs, runtime.App.Panel.Active)
	}
	own, _ := strconv.ParseUint(tabs[1], 10, 64)
	if tab := runtime.browser.tabs[tabs[1]]; tab == nil || tab.conversation != other.ID.String() || tab.node != controller.Target() {
		t.Fatalf("the other session's tab: %+v", tab)
	}
	if tabs := listed("the other session's own tab", other); len(tabs) != 1 || tabs[0]["id"] != float64(own) || tabs[0]["url"] != "" {
		t.Fatalf("the other session's tabs: %v", tabs)
	}
	if tabs := listed("the shown session's tabs again", shown); len(tabs) != 1 || tabs[0]["id"] != float64(number) {
		t.Fatalf("the shown session's tabs after the other's call: %v", tabs)
	}
	focused := use("the other session focuses its tab", other, protocol.BrowserAction{Kind: "focus", Data: protocol.BrowserActionFocusPayload{Tab: own}})
	if focused["tab"] != float64(own) || focused["url"] != "" || runtime.App.Panel.Active.ID != id {
		t.Fatalf("the other session's focus: %v, active %v", focused, runtime.App.Panel.Active)
	}
	// A tab of another session is not addressable.
	refused("the other session focuses the shown session's tab", other, protocol.BrowserAction{Kind: "focus", Data: protocol.BrowserActionFocusPayload{Tab: number}},
		protocol.ErrorCodeNotFound, "browser tab is closed")
	if got := runtime.browser.tabs[id]; got.url != "http://example.org/next" || got.conversation != shown.ID.String() {
		t.Fatalf("the shown session's tab after the other session's calls: %+v", got)
	}
	t.Logf("PANEL-048 passed: the Node routed %d use_browser calls to this controller; each session addressed only its own tabs and every result returned through complete_browser", len(routed))
}
