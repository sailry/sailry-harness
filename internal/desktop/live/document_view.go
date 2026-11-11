package live

import (
	"html"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// documentView is one session document's sandboxed page.
type documentView struct {
	view   *mygo.WebView
	source string
	shown  bool // the last frame placed it
}

// documentOptions is the page a session document renders in: untrusted, so it
// calls nothing of the app, with storage of its own that ends with it.
func documentOptions() mygo.PageOptions {
	return mygo.PageOptions{Untrusted: true, Session: mygo.NewEphemeralSession(), DevTools: mygo.DevToolsDisabled}
}

// documentPage wraps a document's HTML in a frame of an opaque origin: its
// scripts run, without a network, a parent to reach or a place to navigate.
func documentPage(source string) string {
	return `<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; frame-src about: 'self'; img-src data: blob:; font-src data:; connect-src 'none'; form-action 'none'; object-src 'none'; base-uri 'none'"><style>html,body{margin:0;width:100%;height:100%;overflow:hidden}iframe{border:0;width:100%;height:100%;background:white}</style></head><body><iframe sandbox="allow-scripts" srcdoc="` + html.EscapeString(source) + `"></iframe></body></html>`
}

// documentNavigation reports whether a document's page may go to an address:
// only the blank pages the wrapper and its frame are.
func documentNavigation(address string) bool {
	return address == "about:blank" || address == "about:srcdoc"
}

// documentView renders a session document's HTML in its sandbox. It reports
// false where no WebView can show it, and the caller keeps its fallback.
func (r *Runtime) documentView(c *ui.Context, artifact protocol.ArtifactSummary, source string) bool {
	root := r.localRuntime()
	state := root.browser
	if state == nil || !state.available {
		return false
	}
	id := artifact.ID.String()
	document := state.documents[id]
	if document == nil || document.source != source {
		// The WebView is made, or loads the new revision, before the next
		// frame builds.
		root.enqueue(func(*ui.Context) {
			document := state.documents[id]
			if document != nil && !document.view.IsDestroyed() {
				if document.source != source {
					document.source = source
					document.view.Page().LoadHTML(documentPage(source), "about:blank")
				}
				return
			}
			view, err := state.window.NewWebView(mygo.WebViewOptions{Page: documentOptions()})
			if err != nil {
				state.available = false
				root.Report(err)
				return
			}
			page := view.Page()
			page.SetWindowOpenHandler(func(mygo.WindowOpenRequest) *mygo.WindowOptions { return nil })
			page.OnWillNavigate(func(event *mygo.NavigateEvent) {
				if !documentNavigation(event.URL) {
					event.PreventDefault()
				}
			})
			page.LoadHTML(documentPage(source), "about:blank")
			// Shown until the frame that follows decides.
			state.documents[id] = &documentView{view: view, source: source, shown: true}
		})
		if document == nil {
			ui.Row(c.Key("document")).Grow(1).Center().Children(func() { kit.Loader(c, 20) })
			return true
		}
	}
	document.shown = true
	ui.WebView(c.Key("document"), document.view).Grow(1)
	return true
}

// sessionDocumentView renders a panel document tab from the HTML read for it.
func (r *Runtime) sessionDocumentView(c *ui.Context, id string) bool {
	if r.browser == nil {
		return false
	}
	content, read := r.browser.sources[id]
	return read && r.documentView(c, content.Artifact, content.Html)
}

// storeDocument keeps the HTML read for a panel document tab.
func (r *Runtime) storeDocument(id string, content protocol.ArtifactContent) {
	if root := r.localRuntime(); root.browser != nil {
		root.browser.sources[id] = content
	}
}
