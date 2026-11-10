package exporter

import (
	"context"
	"html"
	"time"

	"github.com/egoist/mygo"
)

// PreviewImage captures an opaque, offline authored preview without an app bridge.
func PreviewImage(ctx context.Context, parent *mygo.Window, source string) ([]byte, error) {
	ctx, cancel := context.WithTimeout(ctx, 60*time.Second)
	defer cancel()
	document := `<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; frame-src about: 'self'; img-src data: blob:; font-src data:; connect-src 'none'; form-action 'none'; object-src 'none'; base-uri 'none'"><style>html,body{margin:0;width:100%;height:100%;overflow:hidden}iframe{border:0;width:100%;height:100%;background:white}</style></head><body><iframe sandbox="allow-scripts" srcdoc="` + html.EscapeString(source) + `"></iframe></body></html>`
	view := mygo.NewWindow(mygo.WindowOptions{Title: "Document preview", Parent: parent, Hidden: true, Width: 960, Height: 768, UseContentSize: true, Page: mygo.PageOptions{Untrusted: true, Session: mygo.NewEphemeralSession(), DevTools: mygo.DevToolsDisabled, BackgroundThrottling: mygo.ThrottlingOff}})
	if view == nil {
		return nil, exportError("native document renderer could not be created")
	}
	defer view.Close()
	page := view.Page()
	if page == nil {
		return nil, exportError("native document renderer has no page")
	}
	page.SetWindowOpenHandler(func(mygo.WindowOpenRequest) *mygo.WindowOptions { return nil })
	offNavigation := page.OnWillNavigate(func(event *mygo.NavigateEvent) { event.PreventDefault() })
	defer offNavigation()
	ready := make(chan error, 1)
	send := func(err error) {
		select {
		case ready <- err:
		default:
		}
	}
	offReady := page.OnDidFinishLoad(func() { send(nil) })
	defer offReady()
	offFailed := page.OnDidFailLoad(func(*mygo.LoadError) { send(exportError("native document renderer could not load")) })
	defer offFailed()
	offClose := view.OnClosed(cancel)
	defer offClose()
	if parent != nil {
		off := parent.OnClosed(cancel)
		defer off()
	}
	page.LoadHTML(document, "about:blank")
	select {
	case err := <-ready:
		if err != nil {
			return nil, err
		}
	case <-ctx.Done():
		return nil, renderContextError(ctx.Err())
	}
	if _, err := page.EvalContext(ctx, "return document.readyState"); err != nil {
		return nil, err
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return view.CapturePage()
}
