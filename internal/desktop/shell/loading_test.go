package shell

import (
	"image"
	"math"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// inkRows is the ink of each device row of img across r, from 6 DIPs above
// it to 6 below: how far the row's pixels are from the color beside r's
// left, summed. The first row is device row first.
func inkRows(img *image.RGBA, scale float32, r ui.Rect) (first int, rows []float64) {
	bg := img.RGBAAt(int((r.X-3)*scale), int((r.Y+r.H/2)*scale))
	x0, x1 := int(r.X*scale), int(math.Ceil(float64((r.X+r.W)*scale)))
	first = int((r.Y - 6) * scale)
	for y := first; y < int(math.Ceil(float64((r.Y+r.H+6)*scale))); y++ {
		sum := 0.0
		for x := x0; x < x1; x++ {
			p := img.RGBAAt(x, y)
			sum += math.Abs(float64(p.R)-float64(bg.R)) + math.Abs(float64(p.G)-float64(bg.G)) + math.Abs(float64(p.B)-float64(bg.B))
		}
		rows = append(rows, sum)
	}
	return first, rows
}

// inkCentre is the height, in DIPs, of the middle of the ink in r by its
// weight: the middle of the capitals of a text of capitals, and of the
// body of ideographs.
func inkCentre(img *image.RGBA, scale float32, r ui.Rect) float64 {
	first, rows := inkRows(img, scale, r)
	var sum, weighted float64
	for i, v := range rows {
		sum += v
		weighted += v * (float64(first+i) + 0.5)
	}
	return weighted / sum / float64(scale)
}

// inkMiddle is the height, in DIPs, halfway between the ink's first row and
// its last in r: the middle of an icon, whatever is drawn inside its
// outline.
func inkMiddle(img *image.RGBA, scale float32, r ui.Rect) float64 {
	_, middle := inkHeight(img, scale, r)
	return middle
}

// inkHeight is how high the ink in r is, in DIPs, and the height of its
// middle.
func inkHeight(img *image.RGBA, scale float32, r ui.Rect) (height, middle float64) {
	first, rows := inkRows(img, scale, r)
	peak := 0.0
	for _, v := range rows {
		peak = max(peak, v)
	}
	top, bottom := -1, -1
	for i, v := range rows {
		if v > peak/20 {
			if top < 0 {
				top = i
			}
			bottom = i
		}
	}
	return float64(bottom-top+1) / float64(scale), (float64(first+top) + float64(first+bottom) + 1) / 2 / float64(scale)
}

// The demo's running conversation shows the loading marks in their places:
// the loader on the sidebar's running rows and in the running capsules, no
// larger than the capsules' icons, and the thinking marks on the phase row.
// Its pending rows' icons, labels and details are level within half a DIP
// at scale 1 and 2, the question's too. A sidebar with no session running
// has no loader and asks for no frame, and none with Reduce Motion.
// $SAILRY_SNAPSHOTS takes the frames of the review's contact sheet.
func testLoadingMarks(t *testing.T) {
	running := i18n.In("zh-CN", "activity_running")
	for _, mode := range []theme.Mode{theme.Dark, theme.Light} {
		name := map[theme.Mode]string{theme.Dark: "dark", theme.Light: "light"}[mode]
		a := New(preview.Load())
		a.Mode = mode
		a.Settings.Model.DemoMode = true
		tt := ui.NewTesterWith(a.View, 1280, 820, opaque)
		tt.SetLocale("zh-CN")
		tt.SetScale(2)
		tt.Frame()
		tt.Advance(time.Second)
		snapshot(t, tt, "loading-demo-"+name)

		// The first running row's loader, in the side column.
		if r, ok := tt.Find(running); !ok || r.W != 12 || r.H != 12 || r.X > RailWidth+a.NavWidth {
			t.Fatalf("%s: the sidebar's loader: %+v, %v", name, r, ok)
		}
		// A running capsule's loader is where the port capsule's globe is,
		// 12 DIPs in, and no larger: both in the middle of the capsule.
		img := tt.Image()
		var inks [2]float64
		for i, chip := range []string{i18n.In("zh-CN", "background_count", "count", 2) + ": " + i18n.In("zh-CN", "chat_running"), "8080"} {
			b, ok := tt.AccessByRole(ui.RoleButton, chip)
			if !ok {
				t.Fatalf("%s: no capsule %q: %q", name, chip, tt.Texts())
			}
			// Inside the capsule's border.
			height, middle := inkHeight(img, 2, ui.Rect{X: b.Bounds.X + 12, Y: b.Bounds.Y + 8, W: 14, H: b.Bounds.H - 16})
			if off := math.Abs(middle - float64(b.Bounds.Y+b.Bounds.H/2)); height > 14 || off > 0.5 {
				t.Errorf("%s: the mark of %q is %.2f DIPs high, %.2f off the capsule's middle", name, chip, height, off)
			}
			inks[i] = height
		}
		if inks[0] > inks[1]+1 {
			t.Errorf("%s: the capsule's loader is %.2f DIPs high, its neighbour's icon %.2f", name, inks[0], inks[1])
		}
		t.Logf("%s: capsule loader %.2f DIPs high, globe %.2f", name, inks[0], inks[1])

		// The pending rows, with texts whose ink has its middle at the
		// capitals' and the ideographs'.
		cv := a.Data.Find(preview.DemoStart)
		reply := &cv.Turns[len(cv.Turns)-1].Reply
		kept := append([]conversation.Block(nil), reply.Blocks...)
		type row struct{ label, detail string }
		for _, shown := range [][]row{{{"HIDE", "OXIDE"}, {"ECHO", ""}}, {{"田口", "CODEC"}}} {
			pending := 0
			for i := range reply.Blocks {
				b := &reply.Blocks[i]
				b.Calls = append([]conversation.Call(nil), b.Calls...)
				for k := range b.Calls {
					if call := &b.Calls[k]; call.Pending {
						call.Label, call.Detail, call.Code = shown[0].label, shown[0].detail, true
						pending++
					}
				}
				if b.Question != nil {
					q := *b.Question
					q.Prompt = "ECHO"
					b.Question = &q
				}
			}
			if pending == 0 {
				t.Fatalf("%s: no pending call", name)
			}
			for _, scale := range []float32{1, 2} {
				tt.SetScale(scale)
				tt.Frame()
				img = tt.Image()
				for _, r := range shown {
					label, ok := tt.Find(r.label)
					if !ok {
						t.Fatalf("%s: no row %q: %q", name, r.label, tt.Texts())
					}
					// The icon is the 16 DIPs before the label's 8 DIP gap.
					icon := inkMiddle(img, scale, ui.Rect{X: label.X - 24, Y: label.Y, W: 16, H: label.H})
					text := inkCentre(img, scale, label)
					off := math.Abs(text - icon)
					if r.detail != "" {
						detail, ok := tt.Find(r.detail)
						if !ok {
							t.Fatalf("%s: no detail %q", name, r.detail)
						}
						mono := inkCentre(img, scale, detail)
						off = max(off, math.Abs(mono-icon), math.Abs(mono-text))
						t.Logf("%s, scale %v, %q: icon %.2f, label %+.2f, detail %+.2f", name, scale, r.label, icon, text-icon, mono-icon)
					} else {
						t.Logf("%s, scale %v, %q: icon %.2f, label %+.2f", name, scale, r.label, icon, text-icon)
					}
					if off > 0.5 {
						t.Errorf("%s, scale %v: the row %q is %.2f DIPs off level", name, scale, r.label, off)
					}
				}
			}
		}
		tt.SetScale(2)

		// With nothing pending the phase row shows: thinking, then replying.
		reply.Blocks = kept[:1]
		for _, phase := range []struct {
			phase conversation.Phase
			key   string
		}{{conversation.Thinking, "turn_thinking"}, {conversation.Generating, "turn_generating"}} {
			reply.Phase = phase.phase
			tt.Frame()
			tt.Advance(300 * time.Millisecond)
			label, ok := tt.Find(i18n.In("zh-CN", phase.key))
			if !ok {
				t.Fatalf("%s: no phase row %q", name, phase.key)
			}
			mark := false
			for _, e := range tt.AccessAll() {
				b := e.Bounds
				mark = mark || e.Role == ui.RoleProgress && b.H == 16 && b.X < label.X && label.X-b.X-b.W == 8 && math.Abs(float64(b.Y+b.H/2-label.Y-label.H/2)) < 1
			}
			if !mark {
				t.Errorf("%s: no mark before %q", name, phase.key)
			}
			snapshot(t, tt, "loading-"+phase.key+"-"+name)
		}
		reply.Blocks = kept
	}

	// The sidebar alone: its loaders are all that moves.
	d := preview.Demo("zh-CN", "local")
	side := sidebar.New(d, preview.DemoStart)
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		ui.Column(c).Fill().Background(theme.Of(c).Sidebar).Children(func() { sidebar.View(c, side) })
	}, NavWidth, 700, opaque)
	tt.SetLocale("zh-CN")
	settle := func() {
		tt.Frame()
		tt.Advance(time.Second)
		tt.Frame()
	}
	settle()
	loader := func() bool {
		r, ok := tt.Find(running)
		return ok && r.W == 12 && r.H == 12
	}
	if !loader() || !tt.FramePending() {
		t.Fatalf("the sidebar's running rows: loader %v, frame pending %v", loader(), tt.FramePending())
	}
	tt.SetPreferences(ui.Preferences{ReduceMotion: true, ReduceTransparency: true})
	settle()
	if !loader() || tt.FramePending() {
		t.Errorf("Reduce Motion: loader %v, frame pending %v", loader(), tt.FramePending())
	}
	tt.SetPreferences(opaque.Preferences)
	for _, p := range d.Projects {
		for _, c := range p.Conversations {
			c.Status = preview.Idle
		}
	}
	for _, c := range d.Unassigned {
		c.Status = preview.Idle
	}
	settle()
	if _, ok := tt.Find(running); ok || tt.FramePending() {
		t.Errorf("no session runs: loader %v, frame pending %v", ok, tt.FramePending())
	}
}
