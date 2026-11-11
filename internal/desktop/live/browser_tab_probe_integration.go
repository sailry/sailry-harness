//go:build integration

package live

import (
	"context"
	"encoding/json"
	"fmt"
	"math"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// BrowserTabProbe is the real App over a real Node, for a window with a real
// WebView: what the headless browser tab test replaces with a fake history.
type BrowserTabProbe struct {
	Runtime    *Runtime
	owner      *node.Node
	controller *client.Client
	session    protocol.Session
}

// StartBrowserTabProbe starts a Node on profile with one session of a project
// at project, and the Runtime whose App.View the probe's window shows.
func StartBrowserTabProbe(ctx context.Context, profile, project string) (*BrowserTabProbe, error) {
	owner, err := node.Start(ctx, profile)
	if err != nil {
		return nil, err
	}
	controller := client.New(link.NewLocal(owner.ID(), owner.ID(), owner))
	probe := &BrowserTabProbe{owner: owner, controller: controller}
	if err := controller.Start(ctx); err != nil {
		probe.Close()
		return nil, err
	}
	provider := protocol.DefaultConversationProvider()
	provider.ID = protocol.NewProviderID()
	provider.Name, provider.Api = "Browser probe", protocol.ConversationModelApiChatCompletions
	provider.Authentication, provider.Endpoint = protocol.AuthenticationApiKey, "http://127.0.0.1:9/v1"
	provider.Enabled, provider.DefaultModel = true, "fixture"
	model := protocol.DefaultConversationModel()
	model.ID, model.Context, model.Output = "fixture", 200000, 8192
	model.Reasoning, model.DefaultEffort = true, protocol.Effort{Kind: "none"}
	model.Efforts = []protocol.Effort{model.DefaultEffort}
	provider.Models = []protocol.ConversationModel{model}
	secret := protocol.NewSecret("fixture-key")
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Effort = provider.ID, model.ID, model.DefaultEffort
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionFull
	fail := func(err error) (*BrowserTabProbe, error) {
		probe.Close()
		return nil, err
	}
	if _, err := controller.Do(ctx, protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}}); err != nil {
		return fail(err)
	}
	output, err := controller.Do(ctx, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Browser probe", Path: project, Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"},
	}})
	if err != nil {
		return fail(err)
	}
	registered, valid := detail[protocol.Project](output.Data)
	if !valid {
		return fail(fmt.Errorf("registered project was not returned"))
	}
	if output, err = controller.Do(ctx, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &registered.ID, Config: &config}}); err != nil {
		return fail(err)
	}
	if probe.session, valid = detail[protocol.Session](output.Data); !valid {
		return fail(fmt.Errorf("created session was not returned"))
	}
	if probe.Runtime, err = NewWithPreferences(ctx, controller, preferences.Open(filepath.Join(profile, "desktop", "preferences.json"))); err != nil {
		return fail(err)
	}
	return probe, nil
}

// Close stops the Runtime, the Client and the Node. It must not run on MyGo's
// main thread while the browser is serving a call.
func (probe *BrowserTabProbe) Close() {
	if probe.Runtime != nil {
		_ = probe.Runtime.Close()
	}
	_ = probe.controller.Close()
	_ = probe.owner.Stop(context.Background())
}

// Run exercises a panel browser tab of window, which shows the Runtime's
// App.View, against the pages base+"/" and base+"/next": the tab's real
// WebView history drives Back and Forward, Reload fetches the page again,
// and the session's use_browser calls travel from the Node through this
// controller's adapter and the page script and back through
// complete_browser. Then the chrome's states against the real page: a slow
// page loads under Stop and the bar until it is stopped or finishes, a wide
// page gets exactly the body's width through a resize of the panel, an
// element picked in the page reaches the composer, and a page that cannot
// load shows its failure. requests counts the fetches of a path. Run is
// called off the main thread.
func (probe *BrowserTabProbe) Run(ctx context.Context, window *mygo.Window, base string, requests func(string) int) error {
	runtime := probe.Runtime
	first, second := base+"/", base+"/next"
	type state struct {
		page    panel.BrowserPage
		loading bool
		made    bool
		active  panel.Tab
	}
	var id string
	// observe reads the tab on the main thread.
	observe := func() (state, error) {
		current, ran := browserMain(ctx, func() state {
			current := state{page: runtime.browserPage(id), active: runtime.App.Panel.Active}
			if tab := runtime.browser.tabs[id]; tab != nil && tab.view != nil && !tab.view.IsDestroyed() {
				current.made, current.loading = true, tab.view.Page().IsLoading()
			}
			return current
		})
		if !ran {
			return current, ctx.Err()
		}
		return current, nil
	}
	// settle polls until the tab shows address, loaded, with the expected
	// history and extra condition, for at most 15 seconds.
	settle := func(label, address string, back, forward bool, extra func() bool) error {
		deadline := time.Now().Add(15 * time.Second)
		for {
			current, err := observe()
			if err != nil {
				return fmt.Errorf("%s: %w", label, err)
			}
			if page := current.page; current.made && !current.loading && !page.Loading && page.Error == "" && page.Address == address &&
				page.Back == back && page.Forward == forward && (extra == nil || extra()) {
				return nil
			}
			if time.Now().After(deadline) {
				return fmt.Errorf("%s: the tab shows %+v (page made %v, loading %v), want %s with Back %v and Forward %v",
					label, current.page, current.made, current.loading, address, back, forward)
			}
			if !browserSleep(ctx, 50*time.Millisecond) {
				return fmt.Errorf("%s: %w", label, ctx.Err())
			}
		}
	}
	act := func(work func(actions *panel.BrowserActions)) error {
		if _, ran := browserMain(ctx, func() bool { work(runtime.App.Panel.BrowserActions); runtime.wake(); return true }); !ran {
			return ctx.Err()
		}
		return nil
	}

	if _, ran := browserMain(ctx, func() bool {
		runtime.SetInvalidate(window.Invalidate)
		runtime.SetUpdate(window.Update)
		runtime.SetWebViewWindow(window)
		runtime.OpenSession(probe.session.ID.String())
		return true
	}); !ran {
		return ctx.Err()
	}
	deadline := time.Now().Add(15 * time.Second)
	for {
		ready, ran := browserMain(ctx, func() bool {
			runtime.OpenSession(probe.session.ID.String())
			return runtime.App.Sidebar.Selected == probe.session.ID.String() && runtime.workspace != nil && runtime.workspace.selected == probe.session.Worktree
		})
		if !ran {
			return ctx.Err()
		}
		if ready {
			break
		}
		if time.Now().After(deadline) || !browserSleep(ctx, 50*time.Millisecond) {
			return fmt.Errorf("the session's workspace did not open")
		}
	}
	opened, ran := browserMain(ctx, func() panel.Tab {
		if runtime.browser == nil || !runtime.browser.available {
			return panel.Tab{}
		}
		runtime.App.Panel.New(panel.Browser)
		runtime.App.PanelOpen = true
		runtime.wake()
		return runtime.App.Panel.Active
	})
	if !ran {
		return ctx.Err()
	}
	if opened.Kind != panel.Browser {
		return fmt.Errorf("no browser tab opened: active tab %+v", opened)
	}
	id = opened.ID
	number, err := strconv.ParseUint(id, 10, 64)
	if err != nil {
		return err
	}

	// The user's history.
	if err := act(func(actions *panel.BrowserActions) { actions.Navigate(id, first) }); err != nil {
		return err
	}
	if err := settle("first address", first, false, false, nil); err != nil {
		return err
	}
	if err := act(func(actions *panel.BrowserActions) { actions.Navigate(id, second) }); err != nil {
		return err
	}
	if err := settle("second address", second, true, false, nil); err != nil {
		return err
	}
	if err := act(func(actions *panel.BrowserActions) { actions.Back(id) }); err != nil {
		return err
	}
	if err := settle("Back", first, false, true, nil); err != nil {
		return err
	}
	if err := act(func(actions *panel.BrowserActions) { actions.Forward(id) }); err != nil {
		return err
	}
	if err := settle("Forward", second, true, false, nil); err != nil {
		return err
	}
	fetched := requests("/next")
	if err := act(func(actions *panel.BrowserActions) { actions.Reload(id) }); err != nil {
		return err
	}
	if err := settle("Reload", second, true, false, func() bool { return requests("/next") == fetched+1 }); err != nil {
		return fmt.Errorf("%w; /next was fetched %d times, %d before Reload", err, requests("/next"), fetched)
	}

	// The agent's calls, as the session's browser tool sends them.
	use := func(action protocol.BrowserAction) (map[string]any, error) {
		output, err := probe.controller.Do(ctx, protocol.Command{Kind: "use_browser", Data: protocol.CommandUseBrowserPayload{
			Session: probe.session.ID, Worktree: probe.session.Worktree, Action: action,
		}})
		if err != nil {
			return nil, fmt.Errorf("use_browser %s: %w", action.Kind, err)
		}
		encoded, err := json.Marshal(output.Data)
		var value map[string]any
		if err != nil || output.Kind != "browser" || json.Unmarshal(encoded, &value) != nil {
			return nil, fmt.Errorf("use_browser %s returned %s %s", action.Kind, output.Kind, encoded)
		}
		return value, nil
	}
	value, err := use(protocol.BrowserAction{Kind: "tabs"})
	if err != nil {
		return err
	}
	tabs, _ := value["tabs"].([]any)
	if len(tabs) != 1 {
		return fmt.Errorf("the session lists tabs %v", value)
	}
	if tab, _ := tabs[0].(map[string]any); tab["id"] != float64(number) || tab["url"] != second || tab["title"] != "Browser probe next" {
		return fmt.Errorf("the session lists tab %v, not %d at %s", tabs[0], number, second)
	}
	if value, err = use(protocol.BrowserAction{Kind: "read", Data: protocol.DefaultBrowserActionReadPayload()}); err != nil {
		return err
	}
	if value["tab"] != float64(number) || value["url"] != second || value["title"] != "Browser probe next" {
		return fmt.Errorf("read returned %v", value)
	}
	if value, err = use(protocol.BrowserAction{Kind: "navigate", Data: protocol.BrowserActionNavigatePayload{Url: first, Tab: &number}}); err != nil {
		return err
	}
	if value["tab"] != float64(number) || value["url"] != first || value["title"] != "Browser probe" {
		return fmt.Errorf("navigate returned %v", value)
	}
	if err := settle("the agent's navigation", first, true, false, nil); err != nil {
		return err
	}
	if value, err = use(protocol.BrowserAction{Kind: "back", Data: protocol.BrowserActionBackPayload{Tab: number}}); err != nil {
		return err
	}
	if value["tab"] != float64(number) || value["url"] != second {
		return fmt.Errorf("back returned %v", value)
	}
	if err := settle("the agent's Back", second, true, true, nil); err != nil {
		return err
	}
	if current, err := observe(); err != nil || current.active != opened {
		return fmt.Errorf("the agent's calls left tab %+v active: %v", current.active, err)
	}

	// until polls the tab's chrome state for at most 15 seconds.
	until := func(label string, ready func(panel.BrowserPage) bool) error {
		deadline := time.Now().Add(15 * time.Second)
		for {
			current, err := observe()
			if err != nil {
				return fmt.Errorf("%s: %w", label, err)
			}
			if ready(current.page) {
				return nil
			}
			if time.Now().After(deadline) {
				return fmt.Errorf("%s: the tab shows %+v (page loading %v)", label, current.page, current.loading)
			}
			if !browserSleep(ctx, 20*time.Millisecond) {
				return fmt.Errorf("%s: %w", label, ctx.Err())
			}
		}
	}
	view, ran := browserMain(ctx, func() *mygo.WebView { return runtime.browser.tabs[id].view })
	if !ran {
		return ctx.Err()
	}
	evaluate := func(code string) (any, error) {
		limit, cancel := context.WithTimeout(ctx, 5*time.Second)
		defer cancel()
		return view.Page().EvalContext(limit, code)
	}
	agent, err := evaluate("return navigator.userAgent")
	if text, _ := agent.(string); err != nil || !strings.Contains(text, "Safari/605.1.15 Sailry/") {
		return fmt.Errorf("the page's user agent is %q: %v", agent, err)
	}

	// A slow page: loading from the first frame after Enter, with the
	// address, until the response; Stop ends a load without a failure.
	slow := base + "/slow"
	if err := act(func(actions *panel.BrowserActions) { actions.Navigate(id, slow) }); err != nil {
		return err
	}
	if err := until("a slow page loading", func(page panel.BrowserPage) bool { return page.Loading && page.Address == slow && page.Error == "" }); err != nil {
		return err
	}
	if err := until("a slow page loaded", func(page panel.BrowserPage) bool {
		return !page.Loading && page.Error == "" && page.Address == slow && page.Title == "Browser probe slow"
	}); err != nil {
		return err
	}
	if err := act(func(actions *panel.BrowserActions) { actions.Navigate(id, slow+"?again") }); err != nil {
		return err
	}
	if err := until("a second slow load", func(page panel.BrowserPage) bool { return page.Loading }); err != nil {
		return err
	}
	if err := act(func(actions *panel.BrowserActions) { actions.Stop(id) }); err != nil {
		return err
	}
	if !browserSleep(ctx, 500*time.Millisecond) {
		return ctx.Err()
	}
	if err := until("Stop", func(page panel.BrowserPage) bool { return !page.Loading && page.Error == "" }); err != nil {
		return err
	}

	// A wide page gets the body's width: the panel's less its 1 DIP border,
	// and again after the panel is resized.
	wide := base + "/wide"
	if err := act(func(actions *panel.BrowserActions) { actions.Navigate(id, wide) }); err != nil {
		return err
	}
	if err := until("the wide page", func(page panel.BrowserPage) bool {
		return !page.Loading && page.Address == wide && page.Title == "Browser probe wide"
	}); err != nil {
		return err
	}
	fits := func(label string) error {
		deadline := time.Now().Add(5 * time.Second)
		for {
			want, ran := browserMain(ctx, func() float64 {
				width, _ := window.ContentSize()
				nav := float32(0)
				if runtime.App.NavOpen {
					nav = runtime.App.NavWidth
				}
				return float64(shell.ClampPanel(runtime.App.PanelWidth, float32(width), nav)) - 1
			})
			if !ran {
				return ctx.Err()
			}
			got, err := evaluate("return window.innerWidth")
			if width, _ := got.(float64); err == nil && math.Abs(width-want) <= 1 {
				fmt.Printf("%s: the page is %v CSS pixels wide in a body of %v DIPs\n", label, width, want)
				return nil
			}
			if time.Now().After(deadline) {
				return fmt.Errorf("%s: the page is %v CSS pixels wide in a body of %v DIPs: %v", label, got, want, err)
			}
			if !browserSleep(ctx, 50*time.Millisecond) {
				return ctx.Err()
			}
		}
	}
	if err := fits("panel as opened"); err != nil {
		return err
	}
	for _, width := range []float32{420, 640} {
		if _, ran := browserMain(ctx, func() bool { runtime.App.PanelWidth = width; runtime.wake(); return true }); !ran {
			return ctx.Err()
		}
		if err := fits(fmt.Sprintf("panel resized to %v", width)); err != nil {
			return err
		}
	}

	// The element selection: a click on a known element reaches the
	// session's composer as its picture and its page context.
	if err := act(func(actions *panel.BrowserActions) { actions.Pick(id) }); err != nil {
		return err
	}
	if err := until("the selection started", func(page panel.BrowserPage) bool { return page.Picking }); err != nil {
		return err
	}
	deadline = time.Now().Add(10 * time.Second)
	for {
		ready, err := evaluate("return !!window.__sailryElementSelection")
		if on, _ := ready.(bool); err == nil && on {
			break
		}
		if time.Now().After(deadline) || !browserSleep(ctx, 50*time.Millisecond) {
			return fmt.Errorf("the page did not start selecting: %v", err)
		}
	}
	if _, err := evaluate("document.querySelector('#pick').click()"); err != nil {
		return fmt.Errorf("click the known element: %w", err)
	}
	type attached struct {
		names   []string
		context []byte
		picture int
	}
	deadline = time.Now().Add(15 * time.Second)
	for {
		current, ran := browserMain(ctx, func() attached {
			var current attached
			for _, attachment := range runtime.App.Composer(probe.session.ID.String()).Attachments {
				current.names = append(current.names, attachment.Name)
				if attachment.Mime == "application/json" {
					current.context = attachment.Data
				} else {
					current.picture = len(attachment.Data)
				}
			}
			return current
		})
		if !ran {
			return ctx.Err()
		}
		if len(current.names) == 2 {
			var document struct {
				Kind    string
				Page    struct{ URL, Title string }
				Element struct {
					Tag, ID, Selector, Text string
				}
			}
			if json.Unmarshal(current.context, &document) != nil || document.Kind != "browser_element" || document.Page.URL != wide ||
				document.Element.Tag != "button" || document.Element.ID != "pick" || !strings.Contains(document.Element.Selector, "pick") ||
				document.Element.Text != "Pick me" || current.picture == 0 {
				return fmt.Errorf("the picked element reached the composer as %v with a %d byte picture: %s", current.names, current.picture, current.context)
			}
			fmt.Printf("picked %s %q: %v, a %d byte picture\n", document.Element.Selector, document.Element.Text, current.names, current.picture)
			break
		}
		if time.Now().After(deadline) || !browserSleep(ctx, 50*time.Millisecond) {
			state, _ := observe()
			return fmt.Errorf("the picked element did not reach the composer: attachments %v, tab %+v", current.names, state.page)
		}
	}
	if err := act(func(actions *panel.BrowserActions) { actions.Pick(id) }); err != nil {
		return err
	}
	if err := until("the selection cancelled", func(page panel.BrowserPage) bool { return !page.Picking }); err != nil {
		return err
	}

	// A page that cannot load: the tab says so instead of a blank page.
	if err := act(func(actions *panel.BrowserActions) { actions.Navigate(id, "http://127.0.0.1:1/") }); err != nil {
		return err
	}
	if err := until("a refused connection", func(page panel.BrowserPage) bool { return !page.Loading && page.Error == "browser_load_failed" }); err != nil {
		return err
	}
	return nil
}

// Visit loads an outside address in a new browser tab and reports how the
// load ended. It needs the network, so it is evidence, not a check.
func (probe *BrowserTabProbe) Visit(ctx context.Context, address string) string {
	runtime := probe.Runtime
	id, ran := browserMain(ctx, func() string {
		runtime.App.Panel.New(panel.Browser)
		id := runtime.App.Panel.Active.ID
		runtime.App.Panel.BrowserActions.Navigate(id, address)
		runtime.wake()
		return id
	})
	if !ran {
		return ctx.Err().Error()
	}
	deadline := time.Now().Add(20 * time.Second)
	for {
		page, ran := browserMain(ctx, func() panel.BrowserPage { return runtime.browserPage(id) })
		if !ran {
			return ctx.Err().Error()
		}
		if page.Address != "" && !page.Loading || time.Now().After(deadline) {
			return fmt.Sprintf("%s ended at %s, title %q, loading %v, error %q", address, page.Address, page.Title, page.Loading, page.Error)
		}
		if !browserSleep(ctx, 100*time.Millisecond) {
			return ctx.Err().Error()
		}
	}
}
