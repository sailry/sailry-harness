package conversation_test

import (
	"image"
	"image/color"
	"image/draw"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The viewer's model: turns and flips, the zoom's limits and steps, and the
// gallery's steps.
func TestViewerModel(t *testing.T) {
	// The viewer turns a quarter at a time, flips, and fits back.
	t.Run("RotateFlip", func(t *testing.T) {
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
	})
	// Zoom stays within the reference app's 0.25 and 8 by its steps of 1.25
	// and 0.8; back at the fitted size it centers again.
	t.Run("ZoomLimits", func(t *testing.T) {
		v := conversation.Viewer{Images: make([]conversation.Image, 1), Zoom: 1}
		v.ZoomBy(kit.ZoomIn)
		if v.Zoom != 1.25 {
			t.Errorf("a step in zooms to %v", v.Zoom)
		}
		v.ZoomBy(kit.ZoomOut)
		v.ZoomBy(kit.ZoomOut)
		if d := v.Zoom - 0.8; d > 1e-6 || d < -1e-6 {
			t.Errorf("a step out zooms to %v", v.Zoom)
		}
		for range 20 {
			v.ZoomBy(kit.ZoomIn)
		}
		if v.Zoom != 8 {
			t.Errorf("zoom %v past the limit", v.Zoom)
		}
		for range 40 {
			v.ZoomBy(kit.ZoomOut)
		}
		if v.Zoom != 0.25 {
			t.Errorf("zoom %v under the limit", v.Zoom)
		}
		v.ZoomBy(0)
		v.ZoomBy(-1)
		if v.Zoom != 0.25 {
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
	})
	// The counter tells where the gallery is; Step stays at the ends and
	// shows each picture fitted.
	t.Run("Gallery", func(t *testing.T) {
		v := conversation.Viewer{Images: make([]conversation.Image, 3), Zoom: 1}
		if v.Counter() != "1 / 3" {
			t.Errorf("counter %q", v.Counter())
		}
		v.Step(false)
		v.ZoomBy(2)
		v.Turns, v.FlipH = 1, true
		v.Step(true)
		if v.Index != 1 || v.Zoom != 1 || v.Turns != 0 || v.FlipH {
			t.Errorf("step kept the view: %+v", v)
		}
		v.Step(true)
		v.Step(true)
		if v.Index != 2 || v.Counter() != "3 / 3" {
			t.Errorf("at %d, counter %q", v.Index, v.Counter())
		}
		one := conversation.Viewer{Images: make([]conversation.Image, 1)}
		if one.Counter() != "" {
			t.Errorf("one picture counts %q", one.Counter())
		}
	})
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
	// A picture with one corner filled: a turn or a flip shows.
	pic := func() *ui.Bitmap {
		img := image.NewRGBA(image.Rect(0, 0, 400, 300))
		draw.Draw(img, image.Rect(0, 0, 200, 150), image.NewUniform(color.RGBA{R: 200, A: 255}), image.Point{}, draw.Src)
		return ui.NewBitmap(img)
	}
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
	// Each of the toolbar's turns and flips changes the picture drawn.
	for _, label := range []string{"Rotate", "Flip horizontally", "Flip vertically"} {
		before := slices.Clone(tt.Image().Pix)
		if err := tt.Click(label); err != nil {
			t.Fatal(err)
		}
		tt.Advance(1e9)
		if slices.Equal(before, tt.Image().Pix) {
			t.Errorf("%s left the picture as it was", label)
		}
	}
	tt.Key(0, ui.KeyEscape)
	tt.Advance(1e9)
	if tt.HasText("2 / 2") || tt.HasText("1 / 2") {
		t.Errorf("Escape left the viewer open: %q", tt.Texts())
	}
}
