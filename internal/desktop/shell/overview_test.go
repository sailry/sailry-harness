package shell

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// A conversation shown alone with room beside its transcript has its
// overview as a floating card, no taller than what it holds, whose head
// folds and unfolds it as the saved choice; the header has no button then.
// Narrower, the header's button opens the popover.
func TestOverviewDocksWhereItFits(t *testing.T) {
	title := i18n.In("en", "chat_assets_title")
	settle := func(tt *ui.Tester) {
		for range 3 {
			tt.Frame()
			tt.Advance(300 * time.Millisecond)
		}
		tt.Frame()
	}
	button := func(tt *ui.Tester) (ui.AccessInfo, bool) {
		for _, e := range tt.AccessAll() {
			if e.Role == ui.RoleButton && e.Name == title && e.Bounds.Y < HeaderHeight {
				return e, true
			}
		}
		return ui.AccessInfo{}, false
	}
	click := func(tt *ui.Tester, e ui.AccessInfo) {
		tt.ClickAt(e.Bounds.X+e.Bounds.W/2, e.Bounds.Y+e.Bounds.H/2)
		settle(tt)
	}
	card := func(tt *ui.Tester) (ui.AccessInfo, bool) { return tt.AccessByRole(ui.RoleComplementary, title) }

	a := New(preview.Load())
	tt := ui.NewTesterWith(a.View, 2100, 900, opaque)
	tt.SetLocale("en")
	settle(tt)
	snapshot(t, tt, "overview-docked")
	shown, ok := card(tt)
	if !ok || !tt.HasText(i18n.In("en", "chat_assets_changes")) {
		t.Fatalf("a wide conversation has no overview card: %q", tt.Texts())
	}
	if _, ok := button(tt); ok {
		t.Error("the header keeps the overview button beside the card")
	}
	if shown.Bounds.H > overviewCardMax+8 {
		t.Errorf("the card %v is taller than %v: a column, not a card", shown.Bounds, overviewCardMax)
	}
	if tt.HasText(i18n.In("en", "chat_assets_waiting")) {
		t.Errorf("an overview with rows says it waits: %q", tt.Texts())
	}
	head, ok := tt.Access(i18n.In("en", "chat_assets_fold"))
	if !ok {
		t.Fatalf("the card has no fold: %q", tt.Texts())
	}
	click(tt, head)
	folded, _ := card(tt)
	if !a.OverviewHidden || !a.prefs().OverviewHidden || tt.HasText(i18n.In("en", "chat_assets_changes")) || folded.Bounds.H >= shown.Bounds.H {
		t.Fatalf("the head did not fold the card: hidden %v, %v from %v", a.OverviewHidden, folded.Bounds, shown.Bounds)
	}
	snapshot(t, tt, "overview-folded")
	head, _ = tt.Access(i18n.In("en", "chat_assets_unfold"))
	click(tt, head)
	if a.OverviewHidden || !tt.HasText(i18n.In("en", "chat_assets_changes")) {
		t.Fatal("the head did not unfold the card")
	}

	// Between the tiers the transcript gives way to the card: they stand side
	// by side, the composer ending before the card begins.
	m := New(preview.Load())
	medium := ui.NewTesterWith(m.View, 1500, 820, opaque)
	medium.SetLocale("en")
	settle(medium)
	snapshot(t, medium, "overview-side")
	side, ok := card(medium)
	if !ok {
		t.Fatalf("a conversation with room for the card beside it has none: %q", medium.Texts())
	}
	field, ok := medium.AccessByRole(ui.RoleTextField, i18n.In("en", "composer"))
	if !ok || field.Bounds.X+field.Bounds.W > side.Bounds.X {
		t.Errorf("the composer %v runs under the card %v", field.Bounds, side.Bounds)
	}

	// Widening past the side-by-side tier eases the transcript to the
	// center rather than jumping: a frame into it the composer stands
	// between where it was and where it settles.
	before, _ := medium.AccessByRole(ui.RoleTextField, i18n.In("en", "composer"))
	medium.SetSize(2100, 820)
	medium.Frame()
	medium.Frame()
	medium.Advance(60 * time.Millisecond)
	medium.Frame()
	moving, _ := medium.AccessByRole(ui.RoleTextField, i18n.In("en", "composer"))
	settle(medium)
	after, _ := medium.AccessByRole(ui.RoleTextField, i18n.In("en", "composer"))
	if !(before.Bounds.X < moving.Bounds.X && moving.Bounds.X < after.Bounds.X) {
		t.Errorf("the composer went from %v through %v to %v, not easing", before.Bounds.X, moving.Bounds.X, after.Bounds.X)
	}

	b := New(preview.Load())
	narrow := ui.NewTesterWith(b.View, 1100, 820, opaque)
	narrow.SetLocale("en")
	settle(narrow)
	if _, ok := card(narrow); ok {
		t.Fatal("a narrow conversation docks its overview")
	}
	open, ok := button(narrow)
	if !ok {
		t.Fatalf("no overview button: %q", narrow.Texts())
	}
	click(narrow, open)
	snapshot(t, narrow, "overview-popover")
	if _, docked := card(narrow); !narrow.HasText(i18n.In("en", "chat_assets_changes")) || docked {
		t.Fatalf("the narrow overview did not open as a popover: %q", narrow.Texts())
	}
}

// The aside grows from none, where centering leaves the card its room, to
// the card's whole room at the side-by-side width, without a jump.
func TestOverviewAside(t *testing.T) {
	for _, c := range []struct{ w, want float32 }{
		{overviewCenterMin + 100, 0},
		{overviewCenterMin, 0},
		{overviewCenterMin - 50, 50},
		{overviewSideMin, overviewSide},
	} {
		if got := overviewAside(c.w); got != c.want {
			t.Errorf("aside at %v is %v, want %v", c.w, got, c.want)
		}
	}
}
