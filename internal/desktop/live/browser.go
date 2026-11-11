package live

import (
	"context"
	"net/url"
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
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
}

// browserTab is a panel browser tab's page. node and conversation name the
// conversation the tab was opened in, whose agent may drive it.
type browserTab struct {
	id            uint64
	node          protocol.NodeID
	conversation  string
	url, title    string
	failed        bool
	background    bool
	back, forward bool
	view          *mygo.WebView
}

// SetWebViewWindow gives the window whose native UI shows the browser tabs
// and the session documents. Without it both keep their fallbacks.
func (r *Runtime) SetWebViewWindow(window *mygo.Window) {
	if r.parent != nil || window == nil {
		return
	}
	r.browser = &browserState{window: window, available: mygo.WebViewAvailable(), tabs: map[string]*browserTab{},
		serving: map[*Runtime]context.CancelFunc{}, documents: map[string]*documentView{}, sources: map[string]protocol.ArtifactContent{}}
	r.App.Panel.BrowserActions = &panel.BrowserActions{View: r.browserView, Page: r.browserPage, Navigate: r.browserNavigate,
		Back: func(id string) { r.browserStep(id, -1) }, Forward: func(id string) { r.browserStep(id, 1) }, Reload: func(id string) { r.browserStep(id, 0) }}
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
// local Node and of every bound one, and closes the pages of closed tabs and
// of documents no longer shown.
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
		if !slices.Contains(r.App.Panel.Tabs, panel.Tab{Kind: panel.Browser, ID: id}) {
			if tab.view != nil {
				tab.view.Destroy()
			}
			delete(state.tabs, id)
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

// browserLoad shows an address in a tab, making its WebView on the first one.
// The page is untrusted: it calls nothing of the app, gets no permission,
// opens no window, downloads nothing and stays on HTTP(S).
func (r *Runtime) browserLoad(tab *browserTab, address string) error {
	tab.url, tab.failed = address, false
	if tab.view != nil && !tab.view.IsDestroyed() {
		return tab.view.Page().LoadURL(address)
	}
	view, err := r.browser.window.NewWebView(mygo.WebViewOptions{URL: address, Page: mygo.PageOptions{Untrusted: true}})
	if err != nil {
		tab.failed = true
		return err
	}
	tab.view = view
	page := view.Page()
	page.SetWindowOpenHandler(func(mygo.WindowOpenRequest) *mygo.WindowOptions { return nil })
	page.OnWillNavigate(func(event *mygo.NavigateEvent) {
		if !browserAddress(event.URL) {
			event.PreventDefault()
		}
	})
	history := func() {
		tab.back, tab.forward = page.CanGoBack(), page.CanGoForward()
		r.wake()
	}
	page.OnDidNavigate(func(address string) {
		if browserAddress(address) {
			tab.url = address
		}
		tab.failed = false
		history()
	})
	page.OnDidFinishLoad(history)
	page.OnDidFailLoad(func(failure *mygo.LoadError) {
		// WebKit reports a navigation replaced by another as cancelled.
		if failure == nil || failure.Code != -999 {
			tab.failed = true
		}
		history()
	})
	page.OnPageTitleUpdated(func(event *mygo.TitleEvent) { tab.title = event.Title })
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
		kit.Result(c, kit.ResultProps{Icon: icons.Web, Title: i18n.T(c, "browser_address")}).Grow(1).Justify(ui.Center)
		return true
	}
	// The user brought the tab forward.
	tab.background = false
	ui.WebView(c.Key("page"), tab.view).Grow(1)
	return true
}

func (r *Runtime) browserPage(id string) panel.BrowserPage {
	if r.browser == nil {
		return panel.BrowserPage{}
	}
	tab := r.browser.tabs[id]
	if tab == nil {
		return panel.BrowserPage{}
	}
	return panel.BrowserPage{Address: tab.url, Back: tab.back, Forward: tab.forward}
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
			if err := r.browserLoad(tab, strings.TrimSpace(address)); err != nil {
				r.Report(err)
			}
		}
	})
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
	switch {
	case step < 0:
		page.GoBack()
	case step > 0:
		page.GoForward()
	default:
		tab.failed = false
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
