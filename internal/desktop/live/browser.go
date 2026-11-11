package live

import (
	"context"
	"net/url"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// browserState is the window's browser: one WebView per panel browser tab,
// the controller side of each Node's browser calls, and the sandboxed
// document views. Everything but serving runs on MyGo's main thread.
type browserState struct {
	window    *mygo.Window
	available bool
	tabs      map[string]*browserTab
	serving   map[*Runtime]context.CancelFunc
	documents map[string]*documentView
	sources   map[string]protocol.ArtifactContent
	// persistent is whether the open pages keep their website data.
	persistent bool
	// elements counts the elements picked for a conversation, for their names.
	elements int
}

// browserTab is a panel browser tab's page. node and conversation name the
// conversation the tab was opened in, whose agent may drive it.
type browserTab struct {
	id           uint64
	node         protocol.NodeID
	conversation string
	url, title   string
	// loading is set from the start of a navigation until it finishes, fails
	// or is stopped; failure is the i18n key of what failed, "" otherwise.
	loading       bool
	failure       string
	started       time.Time // when loading was set last
	background    bool
	back, forward bool
	view          *mygo.WebView
	// pick is the element selection under way in the page, nil without one.
	pick *browserPick
}

// begin marks the start of a navigation.
func (tab *browserTab) begin() { tab.loading, tab.failure, tab.started = true, "", time.Now() }

// failed reports whether the tab's page failed to load.
func (tab *browserTab) failed() bool { return tab.failure != "" }

// SetWebViewWindow gives the window whose native UI shows the browser tabs
// and the session documents. Without it both keep their fallbacks.
func (r *Runtime) SetWebViewWindow(window *mygo.Window) {
	if r.parent != nil || window == nil {
		return
	}
	r.browser = &browserState{window: window, available: mygo.WebViewAvailable(), tabs: map[string]*browserTab{},
		serving: map[*Runtime]context.CancelFunc{}, documents: map[string]*documentView{}, sources: map[string]protocol.ArtifactContent{}}
	r.browser.persistent = r.browserPersistent()
	r.App.Panel.BrowserActions = &panel.BrowserActions{View: r.browserView, Page: r.browserPage, Navigate: r.browserNavigate,
		Back: func(id string) { r.browserStep(id, -1) }, Forward: func(id string) { r.browserStep(id, 1) }, Reload: func(id string) { r.browserStep(id, 0) },
		Stop: r.browserStop, Pick: r.browserPickToggle, Inspect: r.browserInspect, Settings: r.browserSettings}
	r.App.Panel.SetDocumentView(r.sessionDocumentView)
	if r.attachments != nil && r.attachments.native.DocumentView == nil {
		actions := r.attachments.native
		actions.DocumentView = r.documentView
		r.SetNativeFileActions(actions)
	}
	r.wake()
}

// browserAddress reports whether a browser tab may load an address: HTTP or
// HTTPS without embedded credentials.
func browserAddress(address string) bool {
	target, err := url.Parse(address)
	return err == nil && (target.Scheme == "http" || target.Scheme == "https") && target.Host != "" && target.User == nil
}

// browserFrame runs before every frame: it serves the browser calls of the
// local Node and of every bound one, closes the pages of closed tabs and of
// documents no longer shown, and reopens the pages in the other website data
// store when the browser's persistence preference changed.
func (r *Runtime) browserFrame() {
	state := r.browser
	if state == nil {
		return
	}
	owners := []*Runtime{r}
	for _, bound := range r.bindings {
		owners = append(owners, bound)
	}
	for owner, cancel := range state.serving {
		if !slices.Contains(owners, owner) {
			cancel()
			delete(state.serving, owner)
		}
	}
	for _, owner := range owners {
		if state.serving[owner] != nil {
			continue
		}
		ctx, cancel := context.WithCancel(owner.ctx)
		if !owner.async(func() {
			_ = owner.controller.ServeBrowser(ctx, func(ctx context.Context, call protocol.BrowserCall) protocol.BrowserResult {
				return r.executeBrowserCall(ctx, owner, call)
			})
		}) {
			cancel()
			continue
		}
		state.serving[owner] = cancel
	}
	for id, tab := range state.tabs {
		// A page stays with its tab in any conversation's panel.
		if !r.App.HasPanelTab(panel.Tab{Kind: panel.Browser, ID: id}) {
			tab.endPick()
			if tab.view != nil {
				tab.view.Destroy()
			}
			delete(state.tabs, id)
		}
	}
	for _, tab := range state.tabs {
		// A step inside one document (a fragment, a script's history entry)
		// ends without a load event: the page itself says it is not loading.
		if tab.loading && tab.view != nil && !tab.view.IsDestroyed() && time.Since(tab.started) > time.Second && !tab.view.Page().IsLoading() {
			tab.loading = false
			tab.back, tab.forward = tab.view.Page().CanGoBack(), tab.view.Page().CanGoForward()
		}
	}
	if persistent := r.browserPersistent(); persistent != state.persistent {
		state.persistent = persistent
		for _, tab := range state.tabs {
			if tab.view == nil {
				continue
			}
			tab.endPick()
			tab.view.Destroy()
			tab.view, tab.back, tab.forward = nil, false, false
			if err := r.browserLoad(tab, tab.url); err != nil {
				r.Report(err)
			}
		}
	}
	for id, document := range state.documents {
		if !document.shown {
			document.view.Destroy()
			delete(state.documents, id)
			continue
		}
		document.shown = false
	}
}

// browserTabFor is a panel browser tab's page state, made on first use for
// the conversation selected then.
func (r *Runtime) browserTabFor(id string) *browserTab {
	state := r.browser
	if tab := state.tabs[id]; tab != nil {
		return tab
	}
	number, err := strconv.ParseUint(id, 10, 64)
	if err != nil {
		return nil
	}
	tab := &browserTab{id: number, node: r.controller.Target(), conversation: r.App.Sidebar.Selected}
	if owner := r.selectedRuntime(); owner != nil {
		tab.node = owner.controller.Target()
	}
	state.tabs[id] = tab
	return tab
}

// browserAdopt gives the tabs opened in a draft to the session that draft
// became on this runtime's Node, which a draft's location may have changed
// since the tab opened. No other tab changes its conversation.
func (r *Runtime) browserAdopt(draft, session string) {
	state := r.localRuntime().browser
	if state == nil {
		return
	}
	for _, tab := range state.tabs {
		if tab.conversation == draft {
			tab.node, tab.conversation = r.controller.Target(), session
		}
	}
}

// browserAgent is appended to the WebView's own user agent. A WebView in
// native UI otherwise sends WebKit's bare one, without the Safari token that
// sites match: Baidu answers it with a page that redirects HTTPS to HTTP,
// which the app's transport security then refuses.
func browserAgent() string { return "Safari/605.1.15 Sailry/" + mygo.App.Version() }

// browserLoad shows an address in a tab, making its WebView on the first one.
// The page is untrusted: it calls nothing of the app, gets no permission,
// opens no window, downloads nothing and stays on HTTP(S).
func (r *Runtime) browserLoad(tab *browserTab, address string) error {
	tab.endPick()
	tab.url, tab.title = address, ""
	tab.begin()
	if tab.view != nil && !tab.view.IsDestroyed() {
		if err := tab.view.Page().LoadURL(address); err != nil {
			tab.loading, tab.failure = false, "browser_load_failed"
			return err
		}
		return nil
	}
	view, err := r.browser.window.NewWebView(mygo.WebViewOptions{URL: address, Page: mygo.PageOptions{Untrusted: true, Session: r.browserSession(),
		UserAgentSuffix: browserAgent(), DevTools: mygo.DevToolsEnabled}})
	if err != nil {
		tab.loading, tab.failure = false, "browser_unavailable"
		return err
	}
	tab.view = view
	page := view.Page()
	page.SetWindowOpenHandler(func(mygo.WindowOpenRequest) *mygo.WindowOptions { return nil })
	page.OnWillNavigate(func(event *mygo.NavigateEvent) {
		if !browserAddress(event.URL) {
			event.PreventDefault()
			return
		}
		// A link or a script takes the page elsewhere.
		tab.endPick()
		tab.begin()
		r.wake()
	})
	history := func() {
		tab.back, tab.forward = page.CanGoBack(), page.CanGoForward()
		r.wake()
	}
	page.OnDidNavigate(func(address string) {
		if browserAddress(address) {
			tab.url = address
		}
		tab.endPick()
		tab.failure, tab.loading = "", page.IsLoading()
		history()
	})
	page.OnDidFinishLoad(func() {
		tab.loading = false
		history()
	})
	page.OnDidFailLoad(func(failure *mygo.LoadError) {
		// WebKit reports a navigation replaced by another, or stopped, as
		// cancelled: the page it left is still the one shown.
		if failure == nil || failure.Code != -999 {
			tab.loading, tab.failure = false, "browser_load_failed"
		} else {
			tab.loading = page.IsLoading()
		}
		history()
	})
	page.OnRenderProcessGone(func(string) {
		tab.endPick()
		tab.loading, tab.failure = false, "browser_load_failed"
		r.wake()
	})
	page.OnPageTitleUpdated(func(event *mygo.TitleEvent) {
		tab.title = event.Title
		r.wake()
	})
	return nil
}

func (r *Runtime) browserView(c *ui.Context, id string) bool {
	state := r.browser
	if state == nil || !state.available {
		return false
	}
	tab := r.browserTabFor(id)
	if tab == nil {
		return false
	}
	if tab.view == nil || tab.view.IsDestroyed() {
		// No page yet: the address field above takes the first one.
		kit.Result(c, kit.ResultProps{Icon: icons.Web, Title: i18n.T(c, "browser_empty")}).Grow(1).Justify(ui.Center)
		return true
	}
	// The user brought the tab forward.
	tab.background = false
	browserSurface(c, tab.view)
	return true
}

// browserSurface places a page's native view over the whole body of its tab,
// the rest of the column under the toolbar. MyGo moves the view to the
// element's box on every frame, so the page follows the panel's handle, the
// window, a split and the display's scale with the layout.
func browserSurface(c *ui.Context, view ui.NativeWebView) ui.Element {
	return ui.WebView(c.Key("page"), view).Grow(1).MinWidth(0).MinHeight(0).FillWidth()
}

func (r *Runtime) browserPage(id string) panel.BrowserPage {
	if r.browser == nil {
		return panel.BrowserPage{}
	}
	tab := r.browser.tabs[id]
	if tab == nil {
		return panel.BrowserPage{}
	}
	return panel.BrowserPage{Address: tab.url, Title: tab.title, Back: tab.back, Forward: tab.forward,
		Loading: tab.loading, Error: tab.failure, Picking: tab.pick != nil}
}

func (r *Runtime) browserNavigate(id, address string) {
	if r.browser == nil || !r.browser.available {
		r.Report(pluginNativeFault(protocol.ErrorCodeUnavailable, "browser page is unavailable"))
		return
	}
	if !browserAddress(strings.TrimSpace(address)) {
		r.Report(pluginNativeFault(protocol.ErrorCodeInvalidRequest, "browser URL must use HTTP or HTTPS without embedded credentials"))
		return
	}
	// The WebView is made before the next frame builds, not inside this one.
	r.enqueue(func(*ui.Context) {
		if tab := r.browserTabFor(id); tab != nil {
			// A page that cannot be made is the tab's failed state, which
			// its body shows: no toast repeats it.
			_ = r.browserLoad(tab, strings.TrimSpace(address))
		}
	})
}

// browserStop stops the load under way: the page keeps what it has.
func (r *Runtime) browserStop(id string) {
	if r.browser == nil {
		return
	}
	tab := r.browser.tabs[id]
	if tab == nil {
		return
	}
	tab.loading = false
	if tab.view != nil && !tab.view.IsDestroyed() {
		tab.view.Page().Stop()
	}
}

// browserInspect opens or closes the web inspector of a tab's page.
func (r *Runtime) browserInspect(id string) {
	if r.browser == nil {
		return
	}
	if tab := r.browser.tabs[id]; tab != nil && tab.view != nil && !tab.view.IsDestroyed() {
		tab.view.Page().ToggleDevTools()
	}
}

// browserSettings opens the browser package's settings.
func (r *Runtime) browserSettings() {
	root := r.localRuntime()
	section := settings.Section("plugin:" + root.controller.Target().String() + "/browser")
	if root.pluginViews != nil {
		if _, exists := root.pluginViews.settings[section]; exists {
			root.App.Settings.Open(section)
			root.App.Navigate(shell.SettingsPage)
			return
		}
	}
	root.configurePackage("browser")
}

// browserStep goes back (-1) or forward (1) in a tab's history, or reloads
// its page (0).
func (r *Runtime) browserStep(id string, step int) {
	if r.browser == nil {
		return
	}
	tab := r.browser.tabs[id]
	if tab == nil || tab.url == "" {
		return
	}
	if tab.view == nil || tab.view.IsDestroyed() {
		if step == 0 {
			r.browserNavigate(id, tab.url)
		}
		return
	}
	page := tab.view.Page()
	tab.endPick()
	tab.begin()
	switch {
	case step < 0:
		page.GoBack()
	case step > 0:
		page.GoForward()
	default:
		page.Reload()
	}
}

// browserReveal shows a tab the agent brought forward, when its conversation
// is the one on screen.
func (r *Runtime) browserReveal(tab *browserTab) {
	if r.App.Sidebar.Selected != tab.conversation {
		return
	}
	r.App.Panel.Show(panel.Tab{Kind: panel.Browser, ID: strconv.FormatUint(tab.id, 10)})
	if r.App.Page == shell.ConversationPage {
		r.App.PanelOpen = true
	}
}
