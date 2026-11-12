package conversation_test

import (
	"fmt"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The transcript fills its pane under what floats over its end (Docked):
// the list, and so its scroll bar, runs the pane's height, its end rests on
// the dock as it did on the list's own bottom, follows the dock as it grows,
// and the jump button and the selection pill stay over the dock. Beside the
// dock's content the wheel and presses are the transcript's.
func TestDock(t *testing.T) {
	const (
		width, height = 1200, 640
		head          = 48
		last          = "Done as asked"
	)
	T := func(key string) string { return i18n.In("en", key) }
	turns := func() []conversation.Turn {
		var lines []string
		for i := 1; i <= 60; i++ {
			lines = append(lines, fmt.Sprintf("Reply line %02d stands alone", i))
		}
		return []conversation.Turn{
			{ID: "long", User: "Write sixty lines", Reply: conversation.Reply{Status: conversation.Completed, Blocks: []conversation.Block{
				{Kind: conversation.TextBlock, Text: strings.Join(lines, "\n\n")},
			}}},
			{ID: "short", User: "Go on", Reply: conversation.Reply{Status: conversation.Completed, Blocks: []conversation.Block{
				{Kind: conversation.TextBlock, Text: last},
			}}},
		}
	}
	// fixture is a transcript under a 48 DIP header with a dock tall DIPs
	// high, 0 for none: the dock's content is as wide as the turns'.
	type fixture struct {
		tt    *ui.Tester
		s     *conversation.State
		r     *quotes
		tall  float32
		dock  ui.Rect // the dock's content, as the last frame laid it out
		wheel int     // scrolls the dock's content took
	}
	open := func(t *testing.T, tall float32) *fixture {
		t.Helper()
		f := &fixture{s: conversation.New(), r: &quotes{}, tall: tall}
		f.r.turns = turns()
		f.tt = ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Dark)
			ui.Column(c).Fill().Children(func() {
				ui.Box(c).Height(head).FillWidth()
				if f.tall == 0 {
					conversation.View(c, f.s, "t", f.r.turns, &conversation.Session{}, f.r)
					return
				}
				conversation.Docked(c, f.s, "t", f.r.turns, &conversation.Session{}, f.r, func() {
					box := ui.Scroll(c.Key("stack")).FillWidth().Height(f.tall)
					box.HandleInput(func(ev ui.InputEvent) bool {
						if ev.Kind == ui.InputScroll {
							f.wheel++
							return true
						}
						return false
					})
					f.dock = box.Bounds()
				})
			})
		}, width, height)
		f.tt.SetLocale("en")
		f.tt.Frame()
		f.tt.Frame()
		return f
	}
	list := func(t *testing.T, f *fixture) ui.Rect {
		t.Helper()
		var found []ui.Rect
		for _, e := range f.tt.AccessAll() {
			if e.Role == ui.RoleLog {
				found = append(found, e.Bounds)
			}
		}
		if len(found) != 1 {
			t.Fatalf("%d transcript lists, want one", len(found))
		}
		return found[0]
	}
	find := func(t *testing.T, f *fixture, text string) ui.Rect {
		t.Helper()
		at, ok := f.tt.Find(text)
		if !ok {
			t.Fatalf("no %q in %q", text, f.tt.Texts())
		}
		return at
	}
	near := func(a, b float32) bool { return a-b < 0.5 && b-a < 0.5 }
	// rest is how far the last turn's text ends above y.
	rest := func(t *testing.T, f *fixture, y float32) float32 {
		t.Helper()
		at := find(t, f, last)
		return y - (at.Y + at.H)
	}

	// The gap the last turn keeps over the list's own bottom, without a dock.
	plain := open(t, 0)
	box := list(t, plain)
	if !near(box.Y, head) || !near(box.Y+box.H, height) {
		t.Fatalf("the list without a dock spans %+v, want the pane under the header", box)
	}
	gap := rest(t, plain, height)
	if gap < 24 {
		t.Fatalf("the last turn ends %.1f over the list's bottom, want its 24 DIP padding at least", gap)
	}

	t.Run("the list spans the pane and the dock lies in its bottom", func(t *testing.T) {
		f := open(t, 180)
		box := list(t, f)
		if !near(box.Y, head) || !near(box.Y+box.H, height) || !near(box.X, 0) || !near(box.W, width) {
			t.Errorf("the list spans %+v, want the whole pane under the header", box)
		}
		if !near(f.dock.Y+f.dock.H, height) || !near(f.dock.H, 180) || f.dock.Y < box.Y {
			t.Errorf("the dock lies at %+v, want the bottom 180 DIPs of the list %+v", f.dock, box)
		}
		if !near(f.dock.W, theme.ContentWidth) || !near(f.dock.X, (width-theme.ContentWidth)/2) {
			t.Errorf("the dock's content lies at %+v, want the centred content column", f.dock)
		}
		if !f.s.List("t").AtEnd() {
			t.Error("the transcript does not open at its end")
		}
		if got := rest(t, f, f.dock.Y); !near(got, gap) {
			t.Errorf("the last turn ends %.1f over the dock, want %.1f as over the list's bottom", got, gap)
		}
	})

	t.Run("the end follows the dock as it grows and shrinks", func(t *testing.T) {
		f := open(t, 120)
		for _, tall := range []float32{260, 90, 320} {
			f.tall = tall
			f.tt.Frame()
			if !near(f.dock.H, tall) {
				t.Fatalf("the dock is %.1f high, want %.1f", f.dock.H, tall)
			}
			if got := rest(t, f, f.dock.Y); !near(got, gap) {
				t.Errorf("a dock %.0f high: the last turn ends %.1f over it, want %.1f", tall, got, gap)
			}
			if !f.s.List("t").AtEnd() {
				t.Errorf("a dock %.0f high: the transcript left its end", tall)
			}
			if f.tt.FramePending() {
				t.Errorf("a dock %.0f high: the view still asks for frames", tall)
			}
		}
	})

	t.Run("scrolled away the place holds as the dock grows", func(t *testing.T) {
		f := open(t, 120)
		f.tt.Scroll(100, 300, 0, -900)
		f.tt.Frame()
		if f.s.List("t").AtEnd() {
			t.Fatal("a scroll up left the transcript at its end")
		}
		first, _ := f.s.List("t").Visible()
		var line string
		var before ui.Rect
		for i := 1; i <= 60; i++ {
			line = fmt.Sprintf("Reply line %02d stands alone", i)
			if at, ok := f.tt.Find(line); ok && at.Y > head+40 && at.Y < 300 {
				before = at
				break
			}
		}
		if before.H == 0 {
			t.Fatalf("no reply line in view to hold: %q", f.tt.Texts())
		}
		f.tall = 300
		f.tt.Frame()
		if at := find(t, f, line); !near(at.Y, before.Y) {
			t.Errorf("%q moved from %.1f to %.1f as the dock grew", line, before.Y, at.Y)
		}
		if now, _ := f.s.List("t").Visible(); now != first || f.s.List("t").AtEnd() {
			t.Errorf("the place moved: first turn in view %d, was %d; at end %v", now, first, f.s.List("t").AtEnd())
		}
	})

	t.Run("the wheel beside the dock scrolls the transcript", func(t *testing.T) {
		f := open(t, 220)
		// The gutter left of the content column, level with the dock.
		x, y := float32(60), f.dock.Y+f.dock.H/2
		if x >= f.dock.X {
			t.Fatalf("no gutter beside the dock at %+v", f.dock)
		}
		f.tt.Scroll(x, y, 0, -300)
		f.tt.Frame()
		if f.s.List("t").AtEnd() {
			t.Error("the wheel in the gutter beside the dock did not scroll the transcript")
		}
		if f.wheel != 0 {
			t.Errorf("the dock's content took %d scrolls made beside it", f.wheel)
		}
		// Over the dock's own content the wheel is the content's.
		f.s.List("t").ScrollToEnd()
		f.tt.Frame()
		f.tt.Scroll(f.dock.X+f.dock.W/2, y, 0, -300)
		f.tt.Frame()
		if f.wheel != 1 || !f.s.List("t").AtEnd() {
			t.Errorf("the dock's content took %d scrolls made over it, the transcript at its end %v", f.wheel, f.s.List("t").AtEnd())
		}
	})

	t.Run("the jump button stands over the dock", func(t *testing.T) {
		f := open(t, 220)
		f.tt.Scroll(100, 300, 0, -600)
		f.tt.Advance(conversationFade)
		b, ok := f.tt.AccessByRole(ui.RoleButton, T("turn_latest"))
		if !ok || b.Disabled {
			t.Fatalf("no jump button off the end: %+v", b)
		}
		// 16 DIPs over the bottom of what shows, as over the list's own.
		if got := f.dock.Y - (b.Bounds.Y + b.Bounds.H); !near(got, 16) {
			t.Errorf("the jump button ends %.1f over the dock at %+v, want 16", got, f.dock)
		}
		f.tt.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
		f.tt.Frame()
		if !f.s.List("t").AtEnd() {
			t.Error("the jump button did not show the end")
		}
		if got := rest(t, f, f.dock.Y); !near(got, gap) {
			t.Errorf("after the jump the last turn ends %.1f over the dock, want %.1f", got, gap)
		}
	})

	t.Run("the selection pill stays over the dock", func(t *testing.T) {
		f := open(t, 220)
		// The last line of the long reply shows just over the dock: its
		// pill has no room under it there.
		at := find(t, f, "Reply line 60 stands alone")
		if at.Y+at.H > f.dock.Y {
			t.Fatalf("the line at %+v is behind the dock at %+v", at, f.dock)
		}
		f.tt.Press(at.X+1, at.Y+at.H/2)
		f.tt.Move(at.X+at.W/2, at.Y+at.H/2)
		f.tt.Move(at.X+at.W-1, at.Y+at.H/2)
		f.tt.Frame()
		f.tt.Release(at.X+at.W-1, at.Y+at.H/2)
		f.tt.Frame()
		f.tt.Frame()
		quote, ok := f.tt.AccessByRole(ui.RoleButton, T("message_quote"))
		if !ok {
			t.Fatalf("no selection pill: %q", f.tt.Texts())
		}
		if bottom := quote.Bounds.Y + quote.Bounds.H + 4; bottom > f.dock.Y || quote.Bounds.Y-4 < head {
			t.Errorf("the pill at %+v leaves what shows of the transcript, %d to %.1f", quote.Bounds, head, f.dock.Y)
		}
	})

	t.Run("a press in the gutter beside the dock is the transcript's", func(t *testing.T) {
		f := open(t, 220)
		at := find(t, f, last)
		f.tt.Press(at.X+1, at.Y+at.H/2)
		f.tt.Move(at.X+at.W-1, at.Y+at.H/2)
		f.tt.Frame()
		f.tt.Release(at.X+at.W-1, at.Y+at.H/2)
		f.tt.Frame()
		f.tt.Frame()
		if _, ok := f.tt.AccessByRole(ui.RoleButton, T("message_quote")); !ok {
			t.Fatalf("no selection pill: %q", f.tt.Texts())
		}
		// A press on the transcript elsewhere dismisses the pill: one in the
		// gutter level with the dock reaches the transcript.
		f.tt.ClickAt(60, f.dock.Y+f.dock.H/2)
		f.tt.Frame()
		f.tt.Frame()
		if _, ok := f.tt.AccessByRole(ui.RoleButton, T("message_quote")); ok {
			t.Error("a press in the gutter beside the dock did not reach the transcript: the pill stays")
		}
	})
}
