package shell

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// The view zoom steps through its levels and stops at their ends; the
// window's content is laid out at it while the conversation's text size
// stays the user's own.
func TestViewZoomLevels(t *testing.T) {
	a := New(preview.Load())
	var zoom, width float32
	tt := ui.NewTesterWith(func(c *ui.Context) {
		a.View(c)
		zoom = c.Zoom()
		width, _ = c.Size()
	}, 1280, 820, opaque)
	tt.SetLocale("en")
	tt.Frame()
	if zoom != 1 || width != 1280 {
		t.Fatalf("a new window shows zoom %v, %v wide", zoom, width)
	}
	text := a.Settings.TextSize
	for _, step := range []struct {
		id   string
		want uint16
	}{
		{ViewZoomIn, 110}, {ViewZoomIn, 125}, {ViewZoomIn, 150}, {ViewZoomIn, 150},
		{ViewZoomOut, 125}, {ViewZoomReset, 100}, {ViewZoomOut, 100}, {ViewZoomIn, 110},
	} {
		if !a.runViewZoom(step.id) {
			t.Fatalf("%s was not a zoom command", step.id)
		}
		tt.Frame()
		factor := float32(step.want) / 100
		if a.ViewZoom != step.want || zoom != factor {
			t.Fatalf("%s: zoom %d, window %v; want %d", step.id, a.ViewZoom, zoom, step.want)
		}
		if d := width - 1280/factor; d > 0.01 || d < -0.01 {
			t.Fatalf("%s: content %v wide at %v", step.id, width, factor)
		}
	}
	if a.Settings.TextSize != text {
		t.Errorf("the view zoom changed the conversation text size to %v", a.Settings.TextSize)
	}
	if a.runViewZoom("app.settings") || a.ViewZoom != 110 {
		t.Error("another command changed the zoom")
	}
	a.ViewZoom = 105
	tt.Frame()
	if zoom != 1 {
		t.Errorf("an unknown zoom shows at %v", zoom)
	}
}
