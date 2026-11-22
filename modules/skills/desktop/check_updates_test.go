package desktop

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestCheckUpdatesWhilePending(t *testing.T) {
	s := settings.NewLive()
	s.SetNativePages([]settings.NativePage{Page()})
	s.Open(settings.Skills)
	calls := 0
	s.PackageActions = &settings.PackageActions{CheckUpdates: func(kind viewmodel.PackageKind) {
		if kind != preview.KindSkill {
			t.Fatalf("checking updates for %v", kind)
		}
		calls++
		s.SetBusy(settings.BusySkillUpdates, true)
	}}
	tester := ui.NewTester(func(c *ui.Context) {
		i18n.Bind(c, testCatalog)
		theme.Apply(c, "", theme.Dark)
		settings.Page(c, s, nil, settings.Look{})
	}, 900, 700)
	tester.SetLocale("en")
	label := testCatalog.In("en", "plugins_check_updates")
	button := func() ui.AccessInfo {
		t.Helper()
		b, ok := tester.AccessByRole(ui.RoleButton, label)
		if !ok {
			t.Fatalf("missing %q: %q", label, tester.Texts())
		}
		return b
	}
	press := func(b ui.AccessInfo) {
		tester.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
		tester.Frame()
	}
	tester.Frame()
	press(button())
	if b := button(); !b.Busy || !b.Disabled || calls != 1 {
		t.Fatalf("pending button busy=%v disabled=%v after %d requests", b.Busy, b.Disabled, calls)
	}
	press(button())
	if calls != 1 {
		t.Fatalf("pending button sent %d requests", calls)
	}
	s.SetBusy(settings.BusySkillUpdates, false)
	tester.Frame()
	if b := button(); b.Busy || b.Disabled {
		t.Fatalf("finished button busy=%v disabled=%v", b.Busy, b.Disabled)
	}
}
