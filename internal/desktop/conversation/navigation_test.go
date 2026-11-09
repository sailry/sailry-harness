package conversation_test

import (
	"fmt"
	"image/color"
	"testing"

	"github.com/egoist/mygo/ui"
)

// rail returns the message navigation's box and the value it reads.
func rail(t *testing.T, tt *ui.Tester) (ui.Rect, string) {
	t.Helper()
	r, ok := tt.AccessByRole(ui.RoleSlider, "Message navigation")
	if !ok {
		t.Fatal("no message navigation")
	}
	return r.Bounds, r.Value
}

// ink reports how far the pixel at (x, y) is from the transcript's
// background, the tester's image being in device pixels at scale 1.
func ink(tt *ui.Tester, x, y float32) int {
	img := tt.Image()
	at := func(x, y float32) color.RGBA { return img.RGBAAt(int(x), int(y)) }
	a, b := at(x, y), at(x+30, y) // past the ticks' resting width
	d := func(p, q uint8) int { return max(int(p)-int(q), int(q)-int(p)) }
	return d(a.R, b.R) + d(a.G, b.G) + d(a.B, b.B)
}

// The rail ticks each turn, the last current while the list shows its
// end; a press on a tick shows its turn at the top and makes it current,
// and scrolling moves the current tick with the turn at the top.
func TestNavigation(t *testing.T) {
	s, cv, tt := bigTester(t)
	tt.SetLocale("en")
	tt.Frame()
	l := s.List(cv.ID)
	r, value := rail(t, tt)
	if r.W != 50 || value != "1000/1000" {
		t.Fatalf("rail %v reads %q, want 50 wide at 1000/1000", r, value)
	}
	x := r.X + 12 + 2 + 2 // on the ticks, past the rail's padding and inset
	for _, f := range []float32{0.05, 0.5, 0.95} {
		if ink(tt, x, r.Y+r.H*f) == 0 {
			t.Errorf("no tick at %.0f%% of the rail", f*100)
		}
	}

	// A press on the tick at 30% of the rail shows that turn.
	y := r.Y + r.H*0.3 + 0.1
	want := int((y - r.Y) / r.H * 1000)
	tt.ClickAt(x, y)
	tt.Frame()
	if first, _ := l.Visible(); first != want {
		t.Fatalf("the press showed turn %d first, want %d", first, want)
	}
	if !tt.HasText(cv.Turns[want].User) {
		t.Errorf("turn %d is not shown", want)
	}
	tt.Move(600, 300) // off the rail: no hover widens the ticks
	tt.Frame()
	if _, value = rail(t, tt); value != fmt.Sprintf("%d/1000", want+1) {
		t.Errorf("after the press the rail reads %q, want %d/1000", value, want+1)
	}

	// Scrolling up moves the current turn with the first one shown.
	for range 30 {
		tt.Scroll(600, 300, 0, -400)
	}
	tt.Frame()
	first, _ := l.Visible()
	if first >= want {
		t.Fatalf("scrolling up did not move: turn %d first", first)
	}
	if _, value = rail(t, tt); value != fmt.Sprintf("%d/1000", first+1) {
		t.Errorf("after scrolling the rail reads %q, want %d/1000", value, first+1)
	}
	// The current tick is the darkest: foreground, against the band.
	cur := r.Y + (float32(first)+0.5)*r.H/1000 + 1
	if a, b := ink(tt, x, cur), ink(tt, x, r.Y+r.H*0.9); a <= b {
		t.Errorf("the current tick (%d) is no darker than the band (%d)", a, b)
	}

	// The press focused the rail: Home and End show the first and last.
	tt.Key(0, ui.KeyHome)
	tt.Frame()
	if first, _ := l.Visible(); first != 0 {
		t.Errorf("Home showed turn %d first", first)
	}
	tt.Key(0, ui.KeyEnd)
	tt.Frame()
	if _, last := l.Visible(); last != 999 {
		t.Errorf("End showed turn %d last", last)
	}
}

// The rail hides where the transcript is narrower than the content and a
// rail on each side (900 DIPs).
func TestNavigationHidesNarrow(t *testing.T) {
	_, _, tt := bigTester(t)
	tt.SetLocale("en")
	tt.SetSize(880, 760)
	tt.Frame()
	if _, ok := tt.AccessByRole(ui.RoleSlider, "Message navigation"); ok {
		t.Error("the rail shows at 880 DIPs")
	}
}
