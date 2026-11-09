package shell

import (
	"fmt"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
)

// ring finds the context ring of the selected conversation.
func ring(t *testing.T, a *App, tt *ui.Tester) ui.Rect {
	t.Helper()
	u := a.Data.Find(a.Sidebar.Selected).Session.Usage
	label := fmt.Sprintf("%s %d%%", i18n.In("en", "composer_compression_hint"), int(u.Context*100+0.5))
	r, ok := tt.Find(label)
	if !ok {
		t.Fatalf("no context ring %q: %q", label, tt.Texts())
	}
	return r
}

// The context ring sits in the composer's toolbar directly left of the
// model chip (live/composer.rs Composer-end controls), not after the
// usage figures under the composer.
func TestContextRingBesideModel(t *testing.T) {
	a, tt := newTester(t)
	r := ring(t, a, tt)
	m := a.Composer(a.Sidebar.Selected).Chosen(a.Data.Models)
	chip, ok := tt.Access(i18n.In("en", "composer_model_settings") + ": " + m.Name)
	if !ok {
		t.Fatalf("no model chip for %q", m.Name)
	}
	b := chip.Bounds
	if r.W != 20 || r.H != 20 {
		t.Errorf("ring is %vx%v, want 20x20", r.W, r.H)
	}
	if gap := b.X - (r.X + r.W); gap < 0 || gap > 12 {
		t.Errorf("ring ends %v before the chip, want it directly left", gap)
	}
	if mid, chipMid := r.Y+r.H/2, b.Y+b.H/2; mid < chipMid-2 || mid > chipMid+2 {
		t.Errorf("ring centre %v is off the chip's row %v", mid, chipMid)
	}
}

// Below the compact width the usage under the composer is one button
// (usage/summary.rs) after the location, not over it; a click holds every
// figure in a popover.
func TestUsageCompact(t *testing.T) {
	a, tt := newTester(t)
	tt.SetSize(760, 820)
	tt.Frame()
	tt.Frame()
	stats, ok := tt.Access(i18n.In("en", "composer_statistics"))
	if !ok {
		t.Fatalf("no compact usage button: %q", tt.Texts())
	}
	u := a.Data.Find(a.Sidebar.Selected).Session.Usage
	if tt.HasText(fmt.Sprint(u.Turns)) {
		t.Errorf("the turns figure shows outside the popover")
	}
	branch, ok := tt.AccessByRole(ui.RoleMenuButton, a.Composer(a.Sidebar.Selected).Worktree)
	if !ok {
		t.Fatalf("no worktree button")
	}
	if end := branch.Bounds.X + branch.Bounds.W; end > stats.Bounds.X {
		t.Errorf("the worktree button ends at %v, over the usage at %v", end, stats.Bounds.X)
	}
	b := stats.Bounds
	tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	for _, key := range []string{"composer_tokens", "composer_turns"} {
		if !tt.HasText(i18n.In("en", key)) {
			t.Errorf("the popover lacks %s: %q", key, tt.Texts())
		}
	}
	if !tt.HasText(fmt.Sprint(u.Turns)) {
		t.Errorf("the popover lacks the turns figure")
	}
}
