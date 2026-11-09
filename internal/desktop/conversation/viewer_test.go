package conversation_test

import (
	"image"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The viewer turns a quarter at a time, flips, and fits back.
func TestViewerRotateFlip(t *testing.T) {
	v := conversation.Viewer{Images: make([]conversation.Image, 1), Zoom: 1}
	for range 5 {
		v.Rotate()
	}
	if v.Turns != 1 {
		t.Errorf("five quarter turns = %d", v.Turns)
	}
	v.FlipH, v.FlipV = !v.FlipH, !v.FlipV
	v.ZoomBy(2)
	v.Fit()
	if v.Turns != 0 || v.FlipH || v.FlipV || v.Zoom != 1 || v.X != 0 || v.Y != 0 {
		t.Errorf("fit kept the view: %+v", v)
	}
}

// Zoom stays within the reference app's 0.25 and 8 and steps by 1.25 and 0.8;
// back at the fitted size it centers again.
func TestViewerZoomLimits(t *testing.T) {
	if conversation.MinZoom != 0.25 || conversation.MaxZoom != 8 || kit.ZoomIn != 1.25 || kit.ZoomOut != 0.8 {
		t.Fatalf("limits %v..%v, steps %v %v", conversation.MinZoom, conversation.MaxZoom, kit.ZoomIn, kit.ZoomOut)
	}
	v := conversation.Viewer{Images: make([]conversation.Image, 1), Zoom: 1}
	for range 20 {
		v.ZoomBy(kit.ZoomOut)
	}
	if v.Zoom != conversation.MinZoom {
		t.Errorf("zoom %v under the limit", v.Zoom)
	}
	v.ZoomBy(0)
	v.ZoomBy(-1)
	if v.Zoom != conversation.MinZoom {
		t.Errorf("a bad factor zoomed: %v", v.Zoom)
	}
	v.Zoom = 4
	v.Pan(1000, -1000, 800, 600, 400, 400)
	if v.X != 200 || v.Y != -100 {
		t.Errorf("pan %v,%v past the edges", v.X, v.Y)
	}
	v.ZoomBy(0.1)
	if v.X != 0 || v.Y != 0 {
		t.Errorf("zooming back kept the pan %v,%v", v.X, v.Y)
	}
}

// The fitted size is the reference app's.
func TestFitImage(t *testing.T) {
	for _, c := range []struct{ w, h, cw, ch, ww, wh float32 }{
		{3840, 1920, 1600, 1000, 960, 480},
		{960, 1920, 1600, 1000, 320, 640},
		{2000, 1000, 500, 400, 410, 205},
		{1000, 2000, 500, 400, 164, 328},
		{32, 16, 500, 400, 32, 16},
	} {
		w, h := kit.FitImage(c.w, c.h, c.cw, c.ch)
		if d := max(w-c.ww, c.ww-w, h-c.wh, c.wh-h); d > 0.001 {
			t.Errorf("FitImage(%v, %v, %v, %v) = %v, %v", c.w, c.h, c.cw, c.ch, w, h)
		}
	}
}

// The counter tells where the gallery is; Step stays at the ends.
func TestViewerGallery(t *testing.T) {
	v := conversation.Viewer{Images: make([]conversation.Image, 3), Zoom: 1}
	if v.Counter() != "1 / 3" {
		t.Errorf("counter %q", v.Counter())
	}
	v.Step(false)
	v.Step(true)
	v.Step(true)
	v.Step(true)
	if v.Index != 2 || v.Counter() != "3 / 3" {
		t.Errorf("at %d, counter %q", v.Index, v.Counter())
	}
	one := conversation.Viewer{Images: make([]conversation.Image, 1)}
	if one.Counter() != "" {
		t.Errorf("one picture counts %q", one.Counter())
	}
}

// images records the viewer's Copy, Save and Open.
type images struct {
	noActions
	got []kit.ImageAction
}

func (r *images) ImageAction(a kit.ImageAction, _ conversation.Image) { r.got = append(r.got, a) }

// A message's thumbnail opens Kit's viewer on its gallery, whose keys and
// toolbar drive the Viewer, and whose Copy, Save and Open go to the actions;
// Escape closes it.
func TestViewerShortcuts(t *testing.T) {
	pic := func() *ui.Bitmap { return ui.NewBitmap(image.NewRGBA(image.Rect(0, 0, 400, 300))) }
	turns := []conversation.Turn{{ID: "t", User: "Look", Images: []conversation.Image{
		{ID: "a", Name: "a.png", Width: 400, Height: 300, Source: pic()},
		{ID: "b", Name: "b.png", Width: 400, Height: 300, Source: pic()},
	}}}
	s := conversation.New()
	r := &images{}
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { conversation.View(c, s, "v", turns, nil, r) })
	}, 1024, 760)
	tt.Frame()
	if err := tt.Click("b.png"); err != nil {
		t.Fatal(err)
	}
	tt.Advance(1e9)
	if !tt.HasText("2 / 2") || !tt.HasText("100%") {
		t.Fatalf("viewer did not open on the second picture: %q", tt.Texts())
	}
	for _, k := range []struct {
		mods ui.Modifiers
		key  ui.Key
		want string
	}{
		{0, ui.KeyLeft, "1 / 2"},
		{0, ui.KeyRight, "2 / 2"},
		{0, ui.KeyEqual, "125%"},
		{ui.Shift, ui.KeyEqual, "156%"},
		{0, ui.KeyMinus, "125%"},
		{0, ui.Key0, "100%"},
		{0, ui.KeyMinus, "80%"},
	} {
		tt.Key(k.mods, k.key)
		tt.Advance(1e9)
		if !tt.HasText(k.want) {
			t.Errorf("after %v: no %q in %q", k.key, k.want, tt.Texts())
		}
	}
	for _, label := range []string{"Copy", "Save as", "Open"} {
		if err := tt.Click(label); err != nil {
			t.Fatal(err)
		}
	}
	if len(r.got) != 3 || r.got[0] != kit.ImageCopy || r.got[1] != kit.ImageSave || r.got[2] != kit.ImageOpen {
		t.Errorf("toolbar actions %v", r.got)
	}
	for _, label := range []string{"Rotate", "Flip horizontally", "Flip vertically"} {
		if err := tt.Click(label); err != nil {
			t.Fatal(err)
		}
	}
	tt.Key(0, ui.KeyEscape)
	tt.Advance(1e9)
	if tt.HasText("2 / 2") || tt.HasText("1 / 2") {
		t.Errorf("Escape left the viewer open: %q", tt.Texts())
	}
}
