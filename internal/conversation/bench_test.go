package conversation_test

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/conversation"
	"github.com/sailry/sailry-go/internal/preview"
)

// noActions ignores the transcript's actions.
type noActions struct{}

func (noActions) OpenChild(string) {}
func (noActions) Retry(string)     {}

// bigTester shows the 1000-turn conversation at the window's default
// size, scrolled to its end as it opens.
func bigTester(tb testing.TB) (*conversation.State, *preview.Conversation, *ui.Tester) {
	d := preview.Load()
	cv := d.Find(preview.Big)
	if len(cv.Turns) != 1000 {
		tb.Fatalf("%d turns", len(cv.Turns))
	}
	s := conversation.New()
	tt := ui.NewTester(func(c *ui.Context) {
		ui.Column(c).Fill().Children(func() { conversation.View(c, s, cv.ID, cv.Turns, noActions{}) })
	}, 1024, 760)
	tt.Frame()
	return s, cv, tt
}

// settle lets the transitions started by a change run out, as the
// window's clock does: the tester's stands still between Advances, so a
// transition started under it would ask for frames for ever.
func settle(tt *ui.Tester) { tt.Advance(time.Second) }

// The list builds only the turns in view, wherever it is scrolled.
func TestBigConversationScrolls(t *testing.T) {
	s, cv, tt := bigTester(t)
	l := s.List(cv.ID)
	first, last := l.Visible()
	if last != len(cv.Turns)-1 || last-first > 10 {
		t.Fatalf("opened at turns %d..%d of %d", first, last, len(cv.Turns))
	}
	l.ScrollTo(500, ui.Center)
	tt.Frame()
	if first, last = l.Visible(); first > 500 || last < 500 {
		t.Fatalf("ScrollTo(500) shows turns %d..%d", first, last)
	}
	if !tt.HasText(cv.Turns[500].User) {
		t.Errorf("turn 500 is not shown")
	}
	for range 50 {
		tt.Scroll(500, 300, 0, -400) // negative: toward the start
	}
	if f, _ := l.Visible(); f >= first {
		t.Errorf("scrolling up did not move: %d then %d", first, f)
	}
}

// BenchmarkScroll is the cost of a frame scrolled 120 DIPs up through the
// transcript, as a touchpad scrolls: layout, newly built rows (Markdown
// parsed and laid out) and the CPU rendering of the tester.
func BenchmarkScroll(b *testing.B) {
	s, cv, tt := bigTester(b)
	l := s.List(cv.ID)
	tt.Scroll(500, 300, 0, -120) // the jump button shows from here on
	settle(tt)
	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		if f, _ := l.Visible(); f == 0 {
			b.StopTimer()
			l.ScrollToEnd()
			tt.Frame()
			tt.Scroll(500, 300, 0, -120)
			settle(tt)
			b.StartTimer()
		}
		tt.Scroll(500, 300, 0, -120)
	}
}

// BenchmarkSteady is a frame with nothing changed, as while the pointer moves.
func BenchmarkSteady(b *testing.B) {
	s, cv, tt := bigTester(b)
	s.List(cv.ID).ScrollTo(500, ui.Center)
	tt.Frame()
	settle(tt)
	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		tt.Move(500, float32(300+b.N%2))
	}
}

// BenchmarkJump is the worst case: each frame shows turns never built
// before (their Markdown parsed and laid out from nothing), as dragging the
// scroll bar across the transcript does.
func BenchmarkJump(b *testing.B) {
	s, cv, tt := bigTester(b)
	l := s.List(cv.ID)
	l.ScrollTo(0, ui.Center)
	tt.Frame()
	settle(tt)
	turn := 0
	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		turn = (turn + 37) % len(cv.Turns)
		l.ScrollTo(turn, ui.Center)
		tt.Frame()
	}
}

// BenchmarkHover is the pointer sweeping down the transcript a row at a
// time, across work headers, copy buttons and links: each frame where the
// element under it looks at its hover builds the rows in view again.
func BenchmarkHover(b *testing.B) {
	s, cv, tt := bigTester(b)
	s.List(cv.ID).ScrollTo(500, ui.Center)
	tt.Frame()
	settle(tt)
	b.ReportAllocs()
	b.ResetTimer()
	i := 0
	for b.Loop() {
		i++
		tt.Move(320, float32(40+(i%28)*24))
	}
}
