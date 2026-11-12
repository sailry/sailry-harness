package shell

import (
	"encoding/json"
	"errors"
	"io/fs"
	"os"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/desktop/onboarding"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
)

// Prefs are the window's UI preferences that come back after a relaunch:
// the columns' widths and openness, the panel's tabs, the projects closed
// in the sidebar, and the preview's rail. They are UI state, not user
// data; the preview's conversations are never written.
//
// The file is one for the preview and every profile. A profile keeps its
// rail's order and pins in its own preferences (FeaturePreferences), so a
// live window neither reads Features and Unpinned nor changes them: it
// writes back the ones it loaded.
type Prefs struct {
	NavWidth   float32     `json:"navWidth"`
	NavOpen    bool        `json:"navOpen"`
	PanelWidth float32     `json:"panelWidth"`
	PanelOpen  bool        `json:"panelOpen"`
	PanelTabs  []panel.Tab `json:"panelTabs,omitempty"`
	PanelTab   panel.Tab   `json:"panelTab"`
	// PanelOf is the conversation PanelTabs, PanelTab and PanelOpen belong
	// to, the one shown last; empty in a file from before the panel belonged
	// to conversations, whose tabs the first conversation shown takes.
	PanelOf string `json:"panelOf,omitempty"`
	// Panels are the other conversations' panels (panels.go).
	Panels    map[string]ConversationPanel `json:"panels,omitempty"`
	Collapsed []string                     `json:"collapsed,omitempty"`
	// SessionOrder is the preview's sidebar order, a session_order value; a
	// profile keeps its own in its preferences.
	SessionOrder string   `json:"sessionOrder,omitempty"`
	Features     []string `json:"features,omitempty"` // the rail's order
	Unpinned     []string `json:"unpinned,omitempty"` // features off the rail
	// WelcomeSeen is whether the welcome guide was dismissed; nil in a file from
	// before it counts as seen, and no file at all as a first launch.
	WelcomeSeen *bool `json:"welcomeSeen,omitempty"`
	// Panes are the split workspace's groups, each a layout tree.
	Panes []*panes.Node `json:"panes,omitempty"`
	// DiffSplit is the window's diff layout: side by side when set, else
	// unified, as before the choice.
	DiffSplit bool `json:"diffSplit,omitempty"`
	// ListShare is the stacked list's share of the height (panel.pair);
	// absent, as in a file from before, is the first share.
	ListShare float32 `json:"listShare,omitempty"`
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

// prefs are the preferences of the stage on the window (Prefs).
func (a *App) prefs() Prefs {
	p := Prefs{
		NavWidth: a.NavWidth, NavOpen: a.NavOpen, PanelWidth: a.PanelWidth, PanelOpen: a.PanelOpen,
		PanelTabs: keptTabs(a.Panel.Tabs), Collapsed: a.Sidebar.Collapsed(),
		Panels:   a.savedPanels(),
		Features: a.previewRail.features, Unpinned: a.previewRail.unpinned, SessionOrder: a.previewOrder,
		Panes: a.panes.Saved(), DiffSplit: a.DiffSplit, ListShare: a.ListShare,
	}
	if keeps(a.Panel.Active) {
		p.PanelTab = a.Panel.Active
	} else if len(p.PanelTabs) > 0 {
		p.PanelTab = p.PanelTabs[len(p.PanelTabs)-1]
	}
	if a.keepsPanel(a.panels.mounted) {
		p.PanelOf = a.panels.mounted
	} else if a.panels.mounted != "" {
		// An unsent chat's panel does not outlive the window.
		p.PanelTabs, p.PanelTab, p.PanelOpen = nil, panel.Tab{}, false
	}
	if a.Preview {
		p.Features, p.Unpinned = a.features, a.unpinnedList()
		p.SessionOrder = sidebar.Sort(a.Settings.Model.SessionOrder).Preference()
		p.WelcomeSeen = &a.Welcome.Seen
	}
	return p
}

// SetPrefs applies preferences, as at start-up.
func (a *App) SetPrefs(p Prefs) {
	a.NavWidth, a.NavOpen = ClampNav(p.NavWidth), p.NavOpen
	a.PanelWidth, a.PanelOpen = max(p.PanelWidth, PanelMin), p.PanelOpen
	a.DiffSplit, a.ListShare = p.DiffSplit, p.ListShare
	a.restorePanels(p)
	a.Sidebar.SetCollapsed(p.Collapsed)
	if a.Preview {
		// Merged as a profile's are: a saved order keeps the fixed features
		// in place and gains the ones added since.
		pins := map[string]bool{}
		for _, f := range p.Unpinned {
			pins[f] = false
		}
		a.SetFeaturePreferences(p.Features, pins)
		a.Settings.Model.SessionOrder = int(sidebar.SortOf(p.SessionOrder))
	} else {
		a.previewRail.features, a.previewRail.unpinned = p.Features, p.Unpinned
		a.previewOrder = p.SessionOrder
	}
	// After the panel's tabs, which a terminal pane needs: a pane whose
	// terminal is gone leaves its group, and one whose session is not there
	// waits for it (prunePanes).
	a.panes.Restore(p.Panes)
	a.prunePanes()
	if a.Preview && p.WelcomeSeen != nil && !*p.WelcomeSeen {
		a.Welcome = onboarding.Guide{Open: true}
	}
}

// keeps reports whether a panel tab comes back after a relaunch. A file's
// preview and a subagent's session show what their conversation loaded, which
// a new window does not have: their tabs would come back empty.
func keeps(t panel.Tab) bool { return t.Kind != panel.Artifact && t.Kind != panel.Subagent }

func keptTabs(tabs []panel.Tab) []panel.Tab {
	var out []panel.Tab
	for _, t := range tabs {
		if keeps(t) {
			out = append(out, t)
		}
	}
	return out
}

// Shell width rules.

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
