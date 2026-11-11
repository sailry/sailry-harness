package live

import (
	"bytes"
	"context"
	"image"
	"image/png"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestDocumentSandboxNoBridge covers what a session document's page is given:
// no host function, no storage that outlives it, no origin and no place to
// navigate.
func TestDocumentSandboxNoBridge(t *testing.T) {
	options := documentOptions()
	if !options.Untrusted || options.PreloadScript != "" || options.PreloadAllFrames || len(options.TrustedOrigins) != 0 || options.DevTools != mygo.DevToolsDisabled {
		t.Fatalf("document page options expose the app: %+v", options)
	}
	if options.Session == nil || !options.Session.IsEphemeral() {
		t.Fatal("document page shares a persistent session")
	}
	if second := documentOptions(); second.Session == options.Session {
		t.Fatal("documents share one session")
	}

	source := `<p onclick="x">"></iframe><script>parent.location='/'</script>`
	page := documentPage(source)
	if strings.Count(page, "<iframe") != 1 || strings.Count(page, "<script") != 0 || !strings.Contains(page, `<iframe sandbox="allow-scripts" srcdoc="&lt;p onclick=&#34;x&#34;&gt;&#34;&gt;&lt;/iframe&gt;&lt;script&gt;`) {
		t.Fatalf("document escapes its frame: %s", page)
	}
	for _, grant := range []string{"allow-same-origin", "allow-top-navigation", "allow-popups", "allow-forms", "allow-modals", "allow-downloads"} {
		if strings.Contains(page, grant) {
			t.Fatalf("document frame is granted %s", grant)
		}
	}
	for _, rule := range []string{"default-src 'none'", "connect-src 'none'", "form-action 'none'", "object-src 'none'", "base-uri 'none'"} {
		if !strings.Contains(page, rule) {
			t.Fatalf("document policy lacks %s", rule)
		}
	}

	for _, address := range []string{"about:blank", "about:srcdoc"} {
		if !documentNavigation(address) {
			t.Fatalf("document cannot load its own %s", address)
		}
	}
	for _, address := range []string{"", "/", "/settings", "app://localhost/", "mygo://app/", "http://localhost:5173/", "https://example.com/", "file:///etc/hosts", "javascript:alert(1)", "data:text/html,x", "about:blank#x"} {
		if documentNavigation(address) {
			t.Fatalf("document navigates to %q", address)
		}
	}
}

// testBrowserAddress covers the addresses a browser tab loads.
func testBrowserAddress(t *testing.T) {
	for _, address := range []string{"https://example.com", "http://localhost:3000/a?b=c", "https://example.com/#top"} {
		if !browserAddress(address) {
			t.Fatalf("browser rejects %q", address)
		}
	}
	for _, address := range []string{"", "example.com", "/", "about:blank", "file:///etc/hosts", "javascript:alert(1)", "data:text/html,x", "app://localhost/", "https://user:secret@example.com/", "https://"} {
		if browserAddress(address) {
			t.Fatalf("browser loads %q", address)
		}
	}
}

// testBrowserDeadline covers a browser call that ended before its action's
// turn on the main thread: the action does not run, and the call reports its
// own deadline rather than a closed browser.
func testBrowserDeadline(t *testing.T) {
	expired, cancel := context.WithDeadline(t.Context(), time.Now().Add(-time.Second))
	defer cancel()
	done, runs := make(chan int, 2), 0
	work := func() int { runs++; return runs }
	browserQueued(expired, done, work)
	if runs != 0 || len(done) != 0 {
		t.Fatalf("a queued browser action ran %d times after its call ended", runs)
	}
	if result := browserEnded(expired); result.Err == nil || result.Err.Code != protocol.ErrorCodeCancelled || result.Err.Message != "browser call expired" {
		t.Fatalf("an expired call ended as %+v", result.Err)
	}
	if browserSleep(expired, time.Hour) {
		t.Fatal("a browser wait outlasted its call")
	}
	if result := (&Runtime{}).executeBrowserCall(expired, nil, protocol.BrowserCall{ExpiresAtMs: uint64(time.Now().Add(time.Hour).UnixMilli())}); result.Err == nil || result.Err.Code != protocol.ErrorCodeCancelled {
		t.Fatalf("a call whose context ended ran: %+v", result)
	}
	closed, stop := context.WithCancel(t.Context())
	stop()
	if result := browserEnded(closed); result.Err == nil || result.Err.Message != "browser is closed" {
		t.Fatalf("a closed browser ended a call as %+v", result.Err)
	}
	browserQueued(t.Context(), done, work)
	if runs != 1 || <-done != 1 {
		t.Fatalf("a queued browser action of a live call ran %d times", runs)
	}
}

// testBrowserCapture covers a capture's geometry and its bounded encoding.
func testBrowserCapture(t *testing.T) {
	viewport := [2]float64{800, 600}
	plan, visible := planBrowserCapture(browserRect{-10, 550, 110, 100}, viewport, 2, "device")
	if !visible || plan.region != (browserRect{0, 550, 100, 50}) || plan.width != 200 || plan.height != 100 {
		t.Fatalf("device plan = %+v, %v", plan, visible)
	}
	plan, visible = planBrowserCapture(browserRect{0, 0, 4000, 600}, [2]float64{4000, 600}, 1, "css")
	if !visible || plan.width != browserMaxImageSide || plan.height != 235 {
		t.Fatalf("fitted plan = %+v, %v", plan, visible)
	}
	if _, visible := planBrowserCapture(browserRect{0, 700, 100, 100}, viewport, 1, "css"); visible {
		t.Fatal("a region below the viewport is captured")
	}
	if _, visible := planBrowserCapture(browserRect{0, 0, 100, 100}, viewport, 0, "css"); visible {
		t.Fatal("a capture without a pixel ratio is planned")
	}
	if crop := cropBrowserCapture(browserRect{10.4, 20.6, 100, 50}, viewport, image.Pt(1600, 1200)); crop != image.Rect(20, 41, 221, 142) {
		t.Fatalf("crop = %v", crop)
	}

	var capture bytes.Buffer
	if err := png.Encode(&capture, image.NewRGBA(image.Rect(0, 0, 1600, 1200))); err != nil {
		t.Fatal(err)
	}
	plan, _ = planBrowserCapture(browserRect{0, 0, 400, 300}, viewport, 2, "css")
	for _, format := range []string{"png", "jpeg"} {
		encoded, width, height, ok := encodeBrowserCapture(capture.Bytes(), plan, format)
		if !ok || width != 400 || height != 300 || len(encoded) > browserMaxImageBytes {
			t.Fatalf("%s capture = %d bytes, %dx%d, %v", format, len(encoded), width, height, ok)
		}
		config, decoded, err := image.DecodeConfig(bytes.NewReader(encoded))
		if err != nil || decoded != format || config.Width != 400 || config.Height != 300 {
			t.Fatalf("%s capture decodes as %s %dx%d: %v", format, decoded, config.Width, config.Height, err)
		}
	}
	if _, _, _, ok := encodeBrowserCapture([]byte("pixels"), plan, "png"); ok {
		t.Fatal("an undecodable capture is encoded")
	}
}
