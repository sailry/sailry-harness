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

// quotes keeps what the selection pill's Quote hands the composer.
type quotes struct {
	recorder
	quoted []string
}

func (q *quotes) Quote(text string) { q.quoted = append(q.quoted, text) }

// The selection pill stands clear of the selected lines and inside the
// visible transcript, shows once the pointer lets go, acts, and goes away
// on Escape, a press elsewhere and scrolling.
func TestSelectionPill(t *testing.T) {
	const (
		width, height = 900, 640
		head, foot    = 48, 72 // what stands above and below the transcript
	)
	T := func(key string) string { return i18n.In("en", key) }
	var lines []string
	for i := 1; i <= 60; i++ {
		lines = append(lines, fmt.Sprintf("Reply line %02d stands alone", i))
	}
	r := &quotes{}
	r.turns = []conversation.Turn{
		{ID: "long", User: "Write sixty lines", Reply: conversation.Reply{Status: conversation.Completed, Blocks: []conversation.Block{
			{Kind: conversation.TextBlock, Text: strings.Join(lines, "\n\n")},
		}}},
		{ID: "short", User: "Go on", Reply: conversation.Reply{Status: conversation.Completed, Blocks: []conversation.Block{
			{Kind: conversation.TextBlock, Text: "Done as asked"},
		}}},
	}
	s := conversation.New()
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		ui.Column(c).Fill().Children(func() {
			ui.Box(c).Height(head).FillWidth()
			conversation.View(c, s, "t", r.turns, &conversation.Session{}, r)
			ui.Box(c).Height(foot).FillWidth()
		})
	}, width, height)
	tt.SetLocale("en")
	tt.Frame()
	tt.Frame()
	view := ui.Rect{X: 0, Y: head, W: width, H: height - head - foot}

	// pill is the pill's box, from its first and last buttons and its padding.
	pill := func() (ui.Rect, bool) {
		quote, ok := tt.AccessByRole(ui.RoleButton, T("message_quote"))
		start, ok2 := tt.AccessByRole(ui.RoleButton, T("chat_new"))
		if !ok || !ok2 {
			return ui.Rect{}, false
		}
		a, b := quote.Bounds, start.Bounds
		return ui.Rect{X: a.X - 4, Y: a.Y - 4, W: b.X + b.W - a.X + 8, H: a.H + 8}, true
	}
	find := func(text string) ui.Rect {
		t.Helper()
		at, ok := tt.Find(text)
		if !ok {
			t.Fatalf("no %q in %q", text, tt.Texts())
		}
		return at
	}
	// drag selects from the start of one text to the end of another and
	// reports whether the pill showed while the button was held.
	drag := func(from, to ui.Rect) bool {
		t.Helper()
		tt.Press(from.X+1, from.Y+from.H/2)
		tt.Move((from.X+to.X+to.W)/2, (from.Y+to.Y+to.H)/2)
		tt.Move(to.X+to.W-1, to.Y+to.H/2)
		tt.Frame()
		_, held := pill()
		tt.Release(to.X+to.W-1, to.Y+to.H/2)
		tt.Frame()
		tt.Frame()
		return held
	}
	intersects := func(a, b ui.Rect) bool {
		return a.X < b.X+b.W && b.X < a.X+a.W && a.Y < b.Y+b.H && b.Y < a.Y+a.H
	}
	// shown checks the pill against the selected lines and the view.
	shown := func(selected ...ui.Rect) ui.Rect {
		t.Helper()
		p, ok := pill()
		if !ok {
			t.Fatalf("no pill: %q", tt.Texts())
		}
		for _, line := range selected {
			if intersects(p, line) {
				t.Fatalf("the pill %+v covers the selected line %+v", p, line)
			}
			if p.X > line.X+line.W || p.X+p.W < line.X {
				t.Fatalf("the pill %+v stands aside of the selected line %+v", p, line)
			}
		}
		if p.X < view.X || p.Y < view.Y || p.X+p.W > view.X+view.W || p.Y+p.H > view.Y+view.H {
			t.Fatalf("the pill %+v leaves the visible transcript %+v", p, view)
		}
		return p
	}
	gone := func(why string) {
		t.Helper()
		tt.Frame()
		if p, ok := pill(); ok {
			t.Fatalf("%s left the pill at %+v", why, p)
		}
	}
	click := func(label string) {
		t.Helper()
		if err := tt.Click(label); err != nil {
			t.Fatal(err, tt.Texts())
		}
		tt.Frame()
	}
	// visible is the text of a reply line wholly inside the view, the n-th
	// from its top.
	visible := func(n int) string {
		t.Helper()
		for _, line := range lines {
			at, ok := tt.Find(line)
			// A line out of view is found clipped: an empty or cut box.
			if !ok || at.W < 100 || at.H < 20 || at.Y < view.Y+2 || at.Y+at.H > view.Y+view.H-2 {
				continue
			}
			if n == 0 {
				return line
			}
			n--
		}
		t.Fatalf("no visible reply line: %q", tt.Texts())
		return ""
	}

	t.Run("user bubble", func(t *testing.T) {
		bubble := find("Go on")
		if drag(bubble, bubble) {
			t.Fatal("the pill shows while the button is held")
		}
		p := shown(bubble)
		if p.Y+p.H > bubble.Y {
			t.Fatalf("the pill %+v is not above the bubble's text %+v", p, bubble)
		}
		click(T("message_copy"))
		if got := tt.Clipboard(); !strings.Contains("Go on", got) || got == "" {
			t.Fatalf("Copy wrote %q", got)
		}
		gone("Copy")
	})

	t.Run("short reply", func(t *testing.T) {
		reply := find("Done as asked")
		if drag(reply, reply) {
			t.Fatal("the pill shows while the button is held")
		}
		shown(reply)
		click(T("message_quote"))
		if len(r.quoted) != 1 || !strings.HasPrefix(r.quoted[0], "> ") || !strings.Contains(r.quoted[0], "asked") {
			t.Fatalf("Quote handed %q", r.quoted)
		}
		gone("Quote")

		// A quick drag, its selection told a frame after the release, is
		// placed from the pointer as well.
		tt.ClickAt(reply.X+reply.W+200, reply.Y+reply.H/2)
		tt.Frame()
		tt.Press(reply.X+1, reply.Y+reply.H/2)
		tt.Move(reply.X+reply.W-1, reply.Y+reply.H/2)
		tt.Release(reply.X+reply.W-1, reply.Y+reply.H/2)
		tt.Frame()
		shown(reply)
		tt.Key(0, ui.KeyEscape)
		gone("Escape")
	})

	t.Run("long reply", func(t *testing.T) {
		// Scroll into the long reply, so that its ends are out of view.
		tt.Scroll(width/2, view.Y+view.H/2, 0, -900)
		tt.Frame()
		tt.Frame()
		if _, ok := tt.Find("Go on"); ok {
			if at := find("Go on"); at.Y < view.Y+view.H {
				t.Fatalf("the transcript did not scroll: the last bubble is at %+v", at)
			}
		}
		// A line in the middle: the pill is above it.
		middle := find(visible(5))
		if drag(middle, middle) {
			t.Fatal("the pill shows while the button is held")
		}
		if p := shown(middle); p.Y+p.H > middle.Y {
			t.Fatalf("the pill %+v is not above the line %+v", p, middle)
		}
		tt.Key(0, ui.KeyEscape)
		gone("Escape")

		// The first visible line has no room above: the pill is below it.
		// The drag at the view's edge scrolls the transcript a little, which
		// does not dismiss the pill: the line is read again where it went.
		name := visible(0)
		drag(find(name), find(name))
		first := find(name)
		if p := shown(first); p.Y < first.Y+first.H {
			t.Fatalf("the pill %+v is not below the first visible line %+v", p, first)
		}
		// A press elsewhere dismisses it.
		other := find(visible(8))
		tt.ClickAt(other.X+other.W+120, other.Y+other.H/2)
		gone("a press elsewhere")

		// Several paragraphs: clear of each and still in view.
		names := []string{visible(3), visible(4), visible(5), visible(6)}
		drag(find(names[0]), find(names[3]))
		shown(find(names[0]), find(names[1]), find(names[2]), find(names[3]))
		click(T("chat_new"))
		if len(r.started) != 1 || !strings.Contains(r.started[0], "stands alone\n> ") {
			t.Fatalf("New conversation started %q", r.started)
		}
		gone("New conversation")

		// Scrolling dismisses it, and the text keeps its selection.
		line := find(visible(4))
		drag(line, line)
		shown(line)
		tt.Scroll(width/2, view.Y+view.H/2, 0, -120)
		gone("scrolling")
		tt.Frame()
		gone("the frame after scrolling")
	})
}
