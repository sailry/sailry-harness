package shell

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// The stress conversation's rail ticks the turns of the pages loaded, as
// the live view's does (it reads its history 20 turns at a time): one
// page shows separate ticks 10 apart in the middle of the rail, not a
// band down its whole height. Loading an older page adds its ticks.
func TestNavigationTicksLoadedPages(t *testing.T) {
	a, tt := newTester(t)
	cv := a.Data.Find(preview.Big)
	nav := func() ui.AccessInfo {
		t.Helper()
		r, ok := tt.AccessByRole(ui.RoleSlider, i18n.In("en", "chat_navigation"))
		if !ok {
			t.Fatal("no message navigation")
		}
		return r
	}
	r := nav()
	if r.Value != "20/20" || len(cv.Turns) != 20 {
		t.Fatalf("the rail reads %q over %d turns, want one page of 20", r.Value, len(cv.Turns))
	}
	// The rail is the pane's, not the list's: it reaches past the list, down
	// the composer under it, so its ticks centre in the pane.
	lists := 0
	for _, e := range tt.AccessAll() {
		if e.Role == ui.RoleLog && e.Bounds.H > 0 {
			lists++
			if list, rail := e.Bounds.Y+e.Bounds.H, r.Bounds.Y+r.Bounds.H; rail < list+60 || r.Bounds.Y > e.Bounds.Y {
				t.Errorf("the rail spans %v, the list %v: want the rail over the composer too", r.Bounds, e.Bounds)
			}
		}
	}
	if lists != 1 {
		t.Fatalf("%d transcript lists, want one to measure the rail against", lists)
	}
	snapshot(t, tt, "navigation")
	// Ticks are 2.5 tall in 10 DIP slots: the rail's middle 200 DIPs hold
	// them, and its ends are empty.
	img := tt.Image()
	s := float32(img.Bounds().Dx()) / 1280
	x := r.Bounds.X + 12 + 2 + 2 // past the padding and inset, inside a tick
	inked := func(y float32) bool {
		c := img.RGBAAt(int(x*s), int(y*s))
		return int(c.R)+int(c.G)+int(c.B) < 3*235
	}
	mid := r.Bounds.Y + r.Bounds.H/2
	ticks := 0
	for y := mid - 100; y < mid+100; y += 10 {
		for d := float32(0); d < 10; d++ {
			if inked(y + d) {
				ticks++
				break
			}
		}
	}
	if ticks != 20 {
		t.Errorf("%d slots of the middle 200 DIPs hold a tick, want 20", ticks)
	}
	for _, y := range []float32{r.Bounds.Y + 20, r.Bounds.Y + r.Bounds.H - 20} {
		if inked(y) {
			t.Errorf("the rail is inked at %v, past the page's ticks", y)
		}
	}
	// A page before them adds its ticks.
	preview.LoadOlder(cv)
	for range 4 {
		tt.Advance(time.Second)
		tt.Frame()
	}
	if v := nav().Value; len(cv.Turns) != 40 || v[len(v)-3:] != "/40" {
		t.Errorf("after a page the rail reads %q over %d turns", v, len(cv.Turns))
	}
	// A page that loads while a reply plays leaves the reply on the turn
	// sent, which the page moved down the list.
	t.Run("page during a reply", func(t *testing.T) {
		now := tt.Now()
		preview.Send(cv, "Stream something", now)
		preview.LoadOlder(cv)
		preview.Advance(cv, now)
		// The page puts the turn now at 20 where the sent one was.
		before := cv.Turns[20].Reply
		preview.Advance(cv, now.Add(2*time.Second))
		if len(cv.Turns) != 61 {
			t.Fatalf("%d turns after the send and a page", len(cv.Turns))
		}
		sent, moved := cv.Turns[60], cv.Turns[40]
		if sent.User != "Stream something" || len(sent.Reply.Blocks) != 2 {
			t.Errorf("the sent turn %q has %d blocks of its reply", sent.User, len(sent.Reply.Blocks))
		}
		if moved.Reply.Status != before.Status || len(moved.Reply.Blocks) != len(before.Blocks) {
			t.Errorf("the reply played over an earlier turn: %v, %d blocks", moved.Reply.Status, len(moved.Reply.Blocks))
		}
		preview.Stop(cv)
	})
}
