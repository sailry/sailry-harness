package conversation_test

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// streamTester shows a preview conversation, playing a sent reply on the
// tester's clock as the shell does.
func streamTester(t *testing.T) (*ui.ListState, *preview.Conversation, *ui.Tester) {
	t.Helper()
	cv := preview.Load().Find("sailry-go/theme")
	s := conversation.New()
	tt := ui.NewTester(func(c *ui.Context) {
		preview.Advance(cv, c.Now())
		if cv.Playing() {
			c.After(preview.Tick)
		}
		ui.Column(c).Fill().Children(func() { conversation.View(c, s, cv.ID, cv.Turns, nil, noActions{}) })
	}, 900, 640)
	tt.SetLocale("en")
	tt.Frame()
	return s.List(cv.ID), cv, tt
}

var latest = i18n.In("en", "turn_latest")

// jumpButton returns the button, and whether it shows.
func jumpButton(tt *ui.Tester) (ui.AccessInfo, bool) {
	return tt.AccessByRole(ui.RoleButton, latest)
}

func scrollUp(tt *ui.Tester) {
	for range 5 {
		tt.Scroll(450, 300, 0, -200)
	}
	tt.Advance(conversationFade)
}

// conversationFade is past the button's 200 ms fade.
const conversationFade = 250 * time.Millisecond

// The button shows only off the end, with no threshold, and fades out once
// the end shows again.
func TestJumpAppearsAndHides(t *testing.T) {
	l, _, tt := streamTester(t)
	if !l.AtEnd() {
		t.Fatal("the transcript does not open at its end")
	}
	if _, ok := jumpButton(tt); ok {
		t.Fatal("the button shows at the end")
	}
	tt.Scroll(450, 300, 0, -2) // the smallest scroll up
	tt.Advance(conversationFade)
	b, ok := jumpButton(tt)
	if !ok || b.Disabled {
		t.Fatalf("a 2 DIP scroll up does not show the button: %+v", b)
	}
	// Scrolling back to the end hides it.
	tt.Scroll(450, 300, 0, 50)
	if b, ok = jumpButton(tt); ok && !b.Disabled {
		t.Error("the button stays enabled back at the end")
	}
	tt.Advance(conversationFade)
	if _, ok = jumpButton(tt); ok {
		t.Error("the button stays after its fade")
	}
}

// A click shows the end at once and the list follows it again.
func TestJumpClickScrollsToEnd(t *testing.T) {
	l, _, tt := streamTester(t)
	scrollUp(tt)
	b, ok := jumpButton(tt)
	if !ok || l.AtEnd() {
		t.Fatalf("not scrolled off the end: button %v", ok)
	}
	tt.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
	tt.Frame()
	if !l.AtEnd() {
		t.Fatal("the click did not show the end")
	}
	if b, ok = jumpButton(tt); ok && !b.Disabled {
		t.Error("the button is enabled at the end")
	}
	tt.Advance(conversationFade)
	if _, ok = jumpButton(tt); ok {
		t.Error("the button stays after the click")
	}
}

// A streaming reply keeps the end in view as it grows, until the user
// scrolls up; the button brings the following back.
func TestJumpStickWhileStreaming(t *testing.T) {
	l, cv, tt := streamTester(t)
	preview.Send(cv, "Stream something", tt.Now())
	l.ScrollToEnd()
	tt.Frame()
	for i := range 40 {
		tt.Advance(preview.Tick)
		if !l.AtEnd() {
			t.Fatalf("the end left the view %d ticks into the reply", i)
		}
		if b, ok := jumpButton(tt); ok {
			t.Fatalf("the button shows while following, tick %d: %+v", i, b)
		}
	}
	if !cv.Playing() {
		t.Fatal("the reply ended before the user scrolled")
	}
	scrollUp(tt)
	for range 10 {
		tt.Advance(preview.Tick)
	}
	if l.AtEnd() {
		t.Fatal("the reply pulled the scrolled-up transcript to its end")
	}
	b, ok := jumpButton(tt)
	if !ok {
		t.Fatal("no button while scrolled up from a streaming reply")
	}
	tt.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
	for i := range 20 {
		tt.Advance(preview.Tick)
		if !l.AtEnd() {
			t.Fatalf("the end left the view %d ticks after the click", i)
		}
	}
	if !cv.Playing() {
		t.Log("the reply ended during the test")
	}
}
