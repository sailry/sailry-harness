package shell

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// A conversation shown alone with room beside its transcript has its
// overview as a column, which the header's button hides and shows again as
// the saved choice; narrower, the button opens the popover.
func TestOverviewDocksWhereItFits(t *testing.T) {
	title := i18n.In("en", "chat_assets_title")
	settle := func(tt *ui.Tester) {
		for range 3 {
			tt.Frame()
			tt.Advance(300 * time.Millisecond)
		}
		tt.Frame()
	}
	button := func(t *testing.T, tt *ui.Tester) ui.AccessInfo {
		t.Helper()
		for _, e := range tt.AccessAll() {
			if e.Role == ui.RoleButton && e.Name == title && e.Bounds.Y < HeaderHeight {
				return e
			}
		}
		t.Fatalf("no overview button: %q", tt.Texts())
		return ui.AccessInfo{}
	}
	press := func(t *testing.T, tt *ui.Tester) {
		b := button(t, tt).Bounds
		tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
		settle(tt)
	}
	docked := func(tt *ui.Tester) bool {
		_, ok := tt.AccessByRole(ui.RoleComplementary, title)
		return ok
	}

	a := New(preview.Load())
	tt := ui.NewTesterWith(a.View, 1760, 900, opaque)
	tt.SetLocale("en")
	settle(tt)
	snapshot(t, tt, "overview-docked")
	if !docked(tt) || !tt.HasText(i18n.In("en", "chat_assets_changes")) || !button(t, tt).Expanded {
		t.Fatalf("a wide conversation has no overview column: %q", tt.Texts())
	}
	// Sections without rows do not show.
	if tt.HasText(i18n.In("en", "chat_assets_waiting")) {
		t.Errorf("an overview with rows says it waits: %q", tt.Texts())
	}
	press(t, tt)
	if docked(tt) || !a.OverviewHidden || !a.prefs().OverviewHidden {
		t.Fatalf("the button left the column: docked %v, hidden %v", docked(tt), a.OverviewHidden)
	}
	if _, open := tt.AccessByRole(ui.RoleDialog, title); open {
		t.Error("hiding the column opened the popover")
	}
	press(t, tt)
	if !docked(tt) || a.OverviewHidden {
		t.Fatal("the button did not show the column again")
	}

	b := New(preview.Load())
	narrow := ui.NewTesterWith(b.View, 1280, 820, opaque)
	narrow.SetLocale("en")
	settle(narrow)
	if docked(narrow) {
		t.Fatal("a narrow conversation docks its overview")
	}
	press(t, narrow)
	snapshot(t, narrow, "overview-popover")
	if !narrow.HasText(i18n.In("en", "chat_assets_changes")) || docked(narrow) {
		t.Fatalf("the narrow overview did not open as a popover: %q", narrow.Texts())
	}
}
