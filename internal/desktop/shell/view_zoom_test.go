package shell

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

// zoomTester is newTester that also reports the window's zoom and the
// content's width in its last frame.
func zoomTester(t *testing.T) (*App, *ui.Tester, *[2]float32) {
	t.Helper()
	a := New(preview.Load())
	seen := &[2]float32{}
	tt := ui.NewTesterWith(func(c *ui.Context) {
		a.View(c)
		seen[0] = c.Zoom()
		seen[1], _ = c.Size()
	}, 1280, 820, opaque)
	tt.SetLocale("en")
	tt.Frame()
	return a, tt, seen
}

// In steps the interface size, then past its largest the conversation's text
// size; Out takes those text steps back before the interface size, and keeps
// a text size the user chose; Reset returns both to their defaults.
func TestViewZoomSteps(t *testing.T) {
	a := New(preview.Load())
	m := a.Settings.Model
	m.TextSize = 16
	for i, step := range []struct {
		id    string
		level int
		text  float64
	}{
		{ViewZoomIn, 1, 16}, {ViewZoomIn, 2, 16}, {ViewZoomIn, 3, 16},
		{ViewZoomIn, 3, 17}, {ViewZoomIn, 3, 18}, {ViewZoomIn, 3, 18},
		{ViewZoomOut, 3, 17}, {ViewZoomOut, 3, 16}, {ViewZoomOut, 2, 16},
		{ViewZoomOut, 1, 16}, {ViewZoomOut, 0, 16}, {ViewZoomOut, 0, 16},
	} {
		if !a.runViewZoom(step.id) {
			t.Fatalf("%d: %s was not a zoom command", i, step.id)
		}
		if m.ViewZoom != step.level || m.TextSize != step.text {
			t.Fatalf("%d %s: level %d, text %v; want %d, %v", i, step.id, m.ViewZoom, m.TextSize, step.level, step.text)
		}
	}
	// A text size chosen elsewhere ends the steps In added: Out lowers the
	// interface size and keeps it.
	for range 4 {
		a.runViewZoom(ViewZoomIn)
	}
	if m.ViewZoom != 3 || m.TextSize != 17 {
		t.Fatalf("level %d, text %v", m.ViewZoom, m.TextSize)
	}
	m.TextSize = 15
	a.runViewZoom(ViewZoomOut)
	if m.ViewZoom != 2 || m.TextSize != 15 {
		t.Fatalf("after a chosen text size Out shows level %d, text %v", m.ViewZoom, m.TextSize)
	}
	a.runViewZoom(ViewZoomReset)
	if m.ViewZoom != 0 || m.TextSize != settings.DefaultTextSize {
		t.Fatalf("reset to level %d, text %v", m.ViewZoom, m.TextSize)
	}
	if a.runViewZoom("app.settings") || m.ViewZoom != 0 {
		t.Error("another command changed the zoom")
	}
}

// handleCenter is the middle of a column's resize handle labelled label, in
// the window's DIPs.
func handleCenter(t *testing.T, tt *ui.Tester, label string) (x, y float32) {
	t.Helper()
	for _, info := range tt.AccessAll() {
		if info.Name == label && info.Bounds.H > 200 && info.Bounds.W < 20 {
			return info.Bounds.X + info.Bounds.W/2, info.Bounds.Y + info.Bounds.H/2
		}
	}
	t.Fatalf("no resize handle %q", label)
	return 0, 0
}

func TestViewZoomAdaptsSidebar(t *testing.T) {
	a, tt, seen := zoomTester(t)
	tt.Key(ui.Cmd, ui.KeyP)
	settle(tt)
	nav, details := i18n.In("en", "sidebar_toggle"), i18n.In("en", "details")
	navX, _ := handleCenter(t, tt, nav)
	panelX, _ := handleCenter(t, tt, details)
	main := a.mainLayoutWidth
	saved := a.prefs()
	// The body starts with a one-DIP left outline before its columns.
	if want := float32(RailWidth+1) + saved.NavWidth; !near(navX, want) {
		t.Fatalf("100%% sidebar edge %v, want %v", navX, want)
	}
	lastEdge := navX
	for i, level := range preferences.ViewZoomLevels {
		a.Settings.ViewZoom = i
		settle(tt)
		z := float32(level) / 100
		if seen[0] != z || !near(seen[1], 1280/z) {
			t.Fatalf("%d%%: zoom %v, content %v wide", level, seen[0], seen[1])
		}
		button, ok := tt.AccessByRole(ui.RoleButton, i18n.In("en", "shell_sessions"))
		wantRail := float32(RailWidth)
		if level == 150 {
			wantRail = 72
		}
		if !ok || !near(button.Bounds.W, RailButton*z) || !near(2*button.Bounds.X+button.Bounds.W, wantRail) {
			t.Errorf("%d%%: rail button %+v is not centered in %v window DIPs", level, button.Bounds, wantRail)
		}
		x, _ := handleCenter(t, tt, nav)
		limit := navX + 56 + (wantRail - RailWidth) + (z - 1)
		if i > 0 && x <= lastEdge || i == 0 && !near(x, navX) || x > limit && !near(x, limit) {
			t.Errorf("%d%%: sidebar edge %v, previous %v, at 100%% %v", level, x, lastEdge, navX)
		}
		lastEdge = x
		if x, _ := handleCenter(t, tt, details); !near(x, panelX) {
			t.Errorf("%d%%: the panel's edge at %v, at 100%% %v", level, x, panelX)
		}
		if want := main - (x - navX); !near(a.mainLayoutWidth*z, want) {
			t.Errorf("%d%%: the conversation %v window DIPs wide, want %v", level, a.mainLayoutWidth*z, want)
		}
		if p := a.prefs(); p.NavWidth != saved.NavWidth || p.PanelWidth != saved.PanelWidth || !a.PanelOpen {
			t.Errorf("%d%%: saved widths %v, %v, panel open %v", level, p.NavWidth, p.PanelWidth, a.PanelOpen)
		}
	}
	// A drag moves the edge as far on the window as the pointer, and the
	// width is saved at 100%. Give the full drag room next to the 150% rail.
	tt.SetSize(1320, 820)
	settle(tt)
	x, y := handleCenter(t, tt, nav)
	tt.Press(x, y)
	tt.Move(x+20, y)
	tt.Move(x+40, y)
	tt.Release(x+40, y)
	settle(tt)
	if !near(a.NavWidth, saved.NavWidth+40) {
		t.Errorf("a 40 DIP drag at 150%% made the sidebar %v, from %v", a.NavWidth, saved.NavWidth)
	}
	if got, _ := handleCenter(t, tt, nav); !near(got, x+40) {
		t.Errorf("the dragged edge is at %v, want %v", got, x+40)
	}
	tt.SetSize(1280, 820)
	settle(tt)
	if got, _ := handleCenter(t, tt, nav); !near(got, 1280-MainMin-saved.PanelWidth) || !near(a.mainLayoutWidth*seen[0], MainMin) {
		t.Errorf("the zoom allowance must yield at the main minimum: edge %v, main %v", got, a.mainLayoutWidth*seen[0])
	}

	// At the minimum Desktop width, preserve the fitted details and the main
	// area's width instead of adding zoom padding to a full window.
	tt.SetSize(1024, 820)
	a.Settings.ViewZoom = 0
	settle(tt)
	baseEdge, _ := handleCenter(t, tt, nav)
	baseDetails, _ := handleCenter(t, tt, details)
	baseMain := a.mainLayoutWidth
	if !a.panes.Drop(session(preview.Big), session(themeChat), panes.Right) {
		t.Fatal("cannot split the conversation")
	}
	a.focusPane(session(preview.Big))
	group := a.panes.Displayed()
	group.Sizes[0], group.Sizes[1] = 0.4, 0.6
	shares := slices.Clone(group.Sizes)
	a.Settings.ViewZoom = len(preferences.ViewZoomLevels) - 1
	settle(tt)
	fitEdge, _ := handleCenter(t, tt, nav)
	fitDetails, _ := handleCenter(t, tt, details)
	if !near(fitEdge-(seen[0]-1), baseEdge) || !near(fitDetails, baseDetails) || !near(a.mainLayoutWidth*seen[0]+seen[0]-1, baseMain) {
		t.Errorf("zoom consumed the main or details area in a narrow window: nav %v→%v, details %v→%v, main %v→%v",
			baseEdge, fitEdge, baseDetails, fitDetails, baseMain, a.mainLayoutWidth*seen[0])
	}
	if a.panes.Displayed() != group || !slices.Equal(group.Sizes, shares) || !near(a.NavWidth, saved.NavWidth+40) {
		t.Fatal("narrow zoom changed the saved width or split shares")
	}
}

// Appearance holds the interface size and the conversation's text size,
// which Conversation no longer shows.
func TestAppearanceSizeSettings(t *testing.T) {
	a, tt, seen := zoomTester(t)
	a.Navigate(SettingsPage)
	a.Settings.Open(settings.Conversation)
	tt.Frame()
	textSize := i18n.In("en", "settings_message_text_size")
	if tt.HasText(textSize) {
		t.Error("Conversation still shows the text size")
	}
	a.Settings.Open(settings.Appearance)
	tt.Frame()
	if !tt.HasText(i18n.In("en", "settings_view_size")) || !tt.HasText(textSize) || !tt.HasText("150%") {
		t.Fatalf("Appearance shows %q", tt.Texts())
	}
	if err := tt.Click("125%"); err != nil {
		t.Fatal(err)
	}
	settle(tt)
	if a.Settings.ViewZoom != 2 || seen[0] != 1.25 {
		t.Fatalf("choosing 125%% set level %d, zoom %v", a.Settings.ViewZoom, seen[0])
	}
}

// The keys run the commands as the window's: not while the Shortcuts page
// records a key, which takes it instead, nor behind a modal dialog. They
// need the dispatcher's hook (shortcuts.go).
func TestViewZoomKeys(t *testing.T) {
	a, tt, seen := zoomTester(t)
	m := a.Settings.Model
	key := func(mods ui.Modifiers, k ui.Key) {
		tt.Key(mods, k)
		settle(tt)
	}
	key(ui.Cmd, ui.KeyEqual)
	key(ui.Cmd|ui.Shift, ui.KeyEqual)
	if m.ViewZoom != 2 || seen[0] != 1.25 {
		t.Fatalf("two steps in: level %d, zoom %v", m.ViewZoom, seen[0])
	}
	key(ui.Cmd, ui.KeyEqual)
	key(ui.Cmd, ui.KeyEqual)
	if m.ViewZoom != 3 || m.TextSize != settings.DefaultTextSize+1 {
		t.Fatalf("past the largest size: level %d, text %v", m.ViewZoom, m.TextSize)
	}
	key(ui.Cmd, ui.KeyMinus)
	key(ui.Cmd, ui.KeyMinus)
	if m.ViewZoom != 2 || m.TextSize != settings.DefaultTextSize {
		t.Fatalf("two steps out: level %d, text %v", m.ViewZoom, m.TextSize)
	}
	key(ui.Cmd, ui.Key0)
	if m.ViewZoom != 0 || seen[0] != 1 {
		t.Fatalf("reset: level %d, zoom %v", m.ViewZoom, seen[0])
	}

	a.Navigate(SettingsPage)
	a.Settings.Open(settings.Shortcuts)
	a.Settings.Keys.Record(settings.Registry[0].ID)
	tt.Frame()
	key(ui.Cmd, ui.KeyEqual)
	if m.ViewZoom != 0 {
		t.Error("a key being recorded zoomed the window")
	}
	a.Settings.Keys.Cancel()

	assembly := TestHarnessAssembly()
	a.translations = i18n.New(assembly.Locales...)
	a.Settings.SetNativePages(assembly.SettingsPages)
	a.ProviderUI = assembly.ProviderUI(a.Settings)
	a.Welcome.Open = true
	tt.Frame()
	key(ui.Cmd, ui.KeyEqual)
	if m.ViewZoom != 0 {
		t.Error("a key behind the welcome dialog zoomed the window")
	}
}
