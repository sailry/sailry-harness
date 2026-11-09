package shell

import (
	"encoding/json"
	"errors"
	"io/fs"
	"os"
	"path/filepath"

	"github.com/sailry/sailry-go/internal/onboarding"
	"github.com/sailry/sailry-go/internal/panel"
)

// Prefs are the window's UI preferences that come back after a relaunch:
// the columns' widths and openness, the panel's tabs, the projects closed
// in the sidebar, and the rail's features. They are UI state, not user
// data; the preview's conversations are never written.
type Prefs struct {
	NavWidth   float32     `json:"navWidth"`
	NavOpen    bool        `json:"navOpen"`
	PanelWidth float32     `json:"panelWidth"`
	PanelOpen  bool        `json:"panelOpen"`
	PanelTabs  []panel.Tab `json:"panelTabs,omitempty"`
	PanelTab   panel.Tab   `json:"panelTab"`
	Collapsed  []string    `json:"collapsed,omitempty"`
	Features   []string    `json:"features,omitempty"` // the rail's order
	Unpinned   []string    `json:"unpinned,omitempty"` // features off the rail
	// WelcomeSeen is whether the welcome guide was dismissed
	// (preferences.rs welcome_seen); nil in a file from before it counts
	// as seen, and no file at all as a first launch.
	WelcomeSeen *bool `json:"welcomeSeen,omitempty"`
}

// PrefsFile is the preferences' file name in the app's user data folder,
// beside MyGo's window-state.json, which keeps the window's bounds.
const PrefsFile = "ui-state.json"

// DefaultPrefs are the first launch's.
func DefaultPrefs() Prefs {
	return Prefs{NavWidth: NavWidth, NavOpen: true, PanelWidth: PanelWidth}
}

// LoadPrefs reads the preferences in dir, or the defaults when there are
// none or they do not parse. Widths out of range come back clamped.
func LoadPrefs(dir string) Prefs {
	p := DefaultPrefs()
	data, err := os.ReadFile(filepath.Join(dir, PrefsFile))
	if errors.Is(err, fs.ErrNotExist) {
		seen := false
		p.WelcomeSeen = &seen
		return p
	}
	if err != nil || json.Unmarshal(data, &p) != nil {
		return DefaultPrefs()
	}
	p.NavWidth = ClampNav(p.NavWidth)
	p.PanelWidth = max(p.PanelWidth, PanelMin)
	return p
}

// SavePrefs writes the preferences in dir, through a temporary file so a
// crash keeps the last ones.
func SavePrefs(dir string, p Prefs) error {
	if err := os.MkdirAll(dir, 0o755); err != nil && !errors.Is(err, fs.ErrExist) {
		return err
	}
	data, err := json.MarshalIndent(p, "", "  ")
	if err != nil {
		return err
	}
	tmp := filepath.Join(dir, PrefsFile+".tmp")
	if err := os.WriteFile(tmp, data, 0o644); err != nil {
		return err
	}
	return os.Rename(tmp, filepath.Join(dir, PrefsFile))
}

// Prefs returns the app's preferences as they are now.
func (a *App) Prefs() Prefs {
	return Prefs{
		NavWidth: a.NavWidth, NavOpen: a.NavOpen, PanelWidth: a.PanelWidth, PanelOpen: a.PanelOpen,
		PanelTabs: a.Panel.Tabs, PanelTab: a.Panel.Active, Collapsed: a.Sidebar.Collapsed(),
		Features: a.features, Unpinned: a.unpinnedList(), WelcomeSeen: &a.Welcome.Seen,
	}
}

// SetPrefs applies preferences, as at start-up.
func (a *App) SetPrefs(p Prefs) {
	a.NavWidth, a.NavOpen = ClampNav(p.NavWidth), p.NavOpen
	a.PanelWidth, a.PanelOpen = max(p.PanelWidth, PanelMin), p.PanelOpen
	for _, t := range p.PanelTabs {
		a.Panel.Show(t)
	}
	if p.PanelTab != (panel.Tab{}) {
		a.Panel.Show(p.PanelTab)
	}
	a.Sidebar.SetCollapsed(p.Collapsed)
	if len(p.Features) > 0 {
		a.features = p.Features
	}
	for _, f := range p.Unpinned {
		a.unpinned[f] = true
	}
	if p.WelcomeSeen != nil && !*p.WelcomeSeen {
		a.Welcome = onboarding.Guide{Open: true}
	}
}

// Width rules of shell/limits.rs and preview.rs.

// ClampNav keeps the sidebar's width in 200..320.
func ClampNav(w float32) float32 { return min(max(w, NavMin), NavMax) }

// PanelFits reports whether the right panel shows beside the main column
// in a window w wide: the rail, the sidebar and 320 for the conversation
// leave it its width. A narrow window hides it without closing it.
func PanelFits(w, nav, panel float32) bool {
	return w >= RailWidth+nav+MainMin+panel
}

// ClampPanel keeps the panel's width in 360 and what leaves the
// conversation 320.
func ClampPanel(width, w, nav float32) float32 {
	return max(min(width, w-RailWidth-nav-MainMin), PanelMin)
}
