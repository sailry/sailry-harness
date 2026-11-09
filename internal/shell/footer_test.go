package shell

import (
	"fmt"
	"testing"

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
