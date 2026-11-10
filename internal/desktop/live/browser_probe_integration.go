//go:build integration

package live

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"github.com/egoist/mygo"
)

// BrowserProbe exercises, on the current platform's WebView, what the browser
// tabs and the session documents rely on: an untrusted page loads an address
// and answers the automation script, its pixels are captured and encoded, and
// a document's frame has an opaque origin and stays where it is. place shows
// a WebView in the window's native UI.
func BrowserProbe(ctx context.Context, window *mygo.Window, place func(*mygo.WebView), address string) error {
	if !mygo.WebViewAvailable() {
		return fmt.Errorf("no WebView on this system")
	}
	settle := func(view *mygo.WebView) error {
		deadline := time.Now().Add(20 * time.Second)
		for browserSleep(ctx, 200*time.Millisecond) {
			if !view.Page().IsLoading() {
				return nil
			}
			if time.Now().After(deadline) {
				return fmt.Errorf("page is still loading")
			}
		}
		return ctx.Err()
	}
	evaluate := func(view *mygo.WebView, request string) (map[string]any, error) {
		value, failure := browserEvaluate(ctx, view, []byte(request))
		if failure != nil {
			return nil, fmt.Errorf("%s: %s", request, failure.Err.Message)
		}
		return value, nil
	}

	view, err := window.NewWebView(mygo.WebViewOptions{URL: address, Page: mygo.PageOptions{Untrusted: true}})
	if err != nil {
		return err
	}
	defer view.Destroy()
	place(view)
	if err := settle(view); err != nil {
		return err
	}
	read, err := evaluate(view, `{"action":"read","snapshot":"probe"}`)
	if err != nil {
		return err
	}
	if location, _ := read["url"].(string); !strings.HasPrefix(location, address) {
		return fmt.Errorf("read reports %v, not %s", read["url"], address)
	}
	facts, err := evaluate(view, `{"action":"target","target":null,"scroll":false}`)
	if err != nil {
		return err
	}
	number := func(object any, key string) float64 {
		values, _ := object.(map[string]any)
		value, _ := values[key].(float64)
		return value
	}
	bounds, viewport := facts["bounds"], facts["viewport"]
	plan, visible := planBrowserCapture(browserRect{number(bounds, "x"), number(bounds, "y"), number(bounds, "width"), number(bounds, "height")},
		[2]float64{number(viewport, "width"), number(viewport, "height")}, number(facts, "ratio"), "css")
	if !visible {
		return fmt.Errorf("page measures no visible region: %v", facts)
	}
	pixels, err := view.CapturePage()
	if err != nil {
		return fmt.Errorf("capture: %w", err)
	}
	for _, format := range []string{"png", "jpeg"} {
		encoded, width, height, ok := encodeBrowserCapture(pixels, plan, format)
		if !ok || width != plan.width || height != plan.height {
			return fmt.Errorf("%s capture is %dx%d (%d bytes), planned %dx%d", format, width, height, len(encoded), plan.width, plan.height)
		}
	}

	document, err := window.NewWebView(mygo.WebViewOptions{Page: documentOptions()})
	if err != nil {
		return err
	}
	defer document.Destroy()
	page := document.Page()
	page.SetWindowOpenHandler(func(mygo.WindowOpenRequest) *mygo.WindowOptions { return nil })
	page.OnWillNavigate(func(event *mygo.NavigateEvent) {
		if !documentNavigation(event.URL) {
			event.PreventDefault()
		}
	})
	page.LoadHTML(documentPage(`<!doctype html><p id="state">Loading</p><script>document.getElementById("state").textContent="Rendered"</script>`), "about:blank")
	place(document)
	if err := settle(document); err != nil {
		return err
	}
	opaque, err := page.EvalContext(ctx, `const frame = document.querySelector("iframe"); return frame !== null && frame.contentDocument === null`)
	if err != nil || opaque != true {
		return fmt.Errorf("document frame shares its parent's origin: %v %v", opaque, err)
	}
	target, _ := json.Marshal(address)
	if _, err := page.EvalContext(ctx, "location.href = "+string(target)+"; return true"); err != nil {
		return err
	}
	if !browserSleep(ctx, time.Second) {
		return ctx.Err()
	}
	location, err := page.EvalContext(ctx, "return location.href")
	if err != nil || location != "about:blank" {
		return fmt.Errorf("document navigated to %v: %v", location, err)
	}
	return nil
}
