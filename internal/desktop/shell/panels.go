package shell

import (
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
)

// The right panel belongs to the pane it shows beside, as the reference's
// session_scope/panels.rs: each conversation, and each main terminal pane,
// has its own tabs, its shown tab and whether its column is open and how
// wide. The panel's State holds the active pane's; the others wait here and
// come back with their pane. Files and diffs stay the project's
// (panel.State.Sync).

// ConversationPanel is a pane's panel while another pane is shown, and in the
// saved window state.
type ConversationPanel struct {
	Tabs   []panel.Tab `json:"tabs,omitempty"`
	Active panel.Tab   `json:"active"`
	Open   bool        `json:"open,omitempty"`
	// Width is the column's width when the conversation was left, 0 for the
	// window's.
	Width   float32        `json:"width,omitempty"`
	project *panel.Project // mounted workspace, when known
}

// conversationPanels are the window's panels by owner: a conversation's ID,
// or a main terminal pane's terminalPanel key.
type conversationPanels struct {
	// mounted is the owner whose panel the State holds, "" before the first
	// one.
	mounted   string
	parked    map[string]*ConversationPanel
	project   *panel.Project
	focusMain bool
	// terminalCloses holds the origin of each confirmed close waiting on the Node.
	terminalCloses map[panel.Tab]string
}

// terminalPanelPrefix marks the panel owner of a main terminal pane. A
// session ID never starts with it.
const terminalPanelPrefix = "terminal-pane/"

// terminalPanel is the panel owner of main terminal pane id.
func terminalPanel(id string) string { return terminalPanelPrefix + id }

// panelOwner is the pane whose panel shows: the active main terminal pane's
// own, or the selected conversation's. A terminal pane's column never borrows
// the conversation selected before it.
func (a *App) panelOwner() string {
	if a.panes.Active.Kind == panes.Terminal {
		return terminalPanel(a.panes.Active.ID)
	}
	return a.Sidebar.Selected
}

// RevealConversationPanel shows the active pane's right panel.
// Explicit open actions call it before changing the mounted panel's tabs.
func (a *App) RevealConversationPanel() {
	a.MountPanel()
	if a.Page != ConversationPage {
		a.Navigate(ConversationPage)
	}
	a.syncPanel()
	a.Panel.Sync(a.panels.project)
	a.PanelOpen = true
}

// MountPanel gives the panel to the active pane: the tabs of the one left are
// kept for its return, and the active one's come back, or the closed panel of
// a pane that never opened it. The first pane takes what the window opened
// before it. A terminal has one tab in the window: the one in the shown panel.
func (a *App) MountPanel() {
	p := &a.panels
	if a.Panel.OnEmpty == nil {
		a.Panel.OnEmpty = a.foldConversationPanel
		a.Panel.SharedItem = a.sharedPanelItem
		a.Panel.OnTerminalClose = a.noteTerminalClose
	}
	selected := a.panelOwner()
	if selected != p.mounted {
		kept := p.parked[selected]
		delete(p.parked, selected)
		if p.mounted == "" {
			// Nothing was mounted: what is open joins the conversation's own.
			if kept != nil {
				tabs, active := kept.Tabs, a.Panel.Active
				for _, t := range a.Panel.Tabs {
					tabs = panel.Opened(tabs, t)
				}
				if active == (panel.Tab{}) {
					active = kept.Active
				}
				a.Panel.Mount(tabs, active)
				a.PanelOpen = a.PanelOpen || kept.Open
				if kept.Width > 0 {
					a.PanelWidth = max(kept.Width, PanelMin)
				}
			}
		} else {
			if len(a.Panel.Tabs) > 0 || a.PanelOpen {
				a.park(p.mounted, &ConversationPanel{Tabs: slices.Clone(a.Panel.Tabs), Active: a.Panel.Active, Open: a.PanelOpen, Width: a.PanelWidth, project: p.project})
			}
			if kept == nil {
				kept = &ConversationPanel{}
			}
			a.Panel.Mount(kept.Tabs, kept.Active)
			a.PanelOpen = kept.Open
			if kept.Width > 0 {
				a.PanelWidth = max(kept.Width, PanelMin)
			}
		}
		p.mounted, p.project, p.focusMain = selected, nil, false
	}
	for id, other := range p.parked {
		other.Tabs = slices.DeleteFunc(other.Tabs, func(t panel.Tab) bool {
			return t.Kind == panel.Terminal && slices.Contains(a.Panel.Tabs, t)
		})
		if len(other.Tabs) == 0 && !other.Open {
			delete(p.parked, id)
		}
	}
}

func (a *App) foldConversationPanel() {
	if a.PanelOpen {
		a.PanelOpen = false
		a.panels.focusMain = true
	}
}

func (a *App) sharedPanelItem(pr *panel.Project, t panel.Tab) bool {
	for _, kept := range a.panels.parked {
		if (kept.project == nil || kept.project == pr) && slices.Contains(kept.Tabs, t) {
			return true
		}
	}
	return false
}

func (a *App) park(id string, kept *ConversationPanel) {
	if a.panels.parked == nil {
		a.panels.parked = map[string]*ConversationPanel{}
	}
	a.panels.parked[id] = kept
}

// renamePanel gives a draft's panel to the session it became, as the
// reference's State.promote.
func (a *App) renamePanel(before, after string) {
	p := &a.panels
	if p.mounted == before {
		p.mounted = after
	}
	for tab, owner := range p.terminalCloses {
		if owner == before {
			p.terminalCloses[tab] = after
		}
	}
	if kept := p.parked[before]; kept != nil {
		delete(p.parked, before)
		if p.parked[after] == nil && p.mounted != after {
			p.parked[after] = kept
		}
	}
}

// PanelConversations are the conversations that have a panel: the shown one
// first, then the others in order. A main terminal pane's panel is not a
// conversation's: it leaves with its pane (DropWorkspaceTerminal).
func (a *App) PanelConversations() []string {
	var ids []string
	for id := range a.panels.parked {
		if !strings.HasPrefix(id, terminalPanelPrefix) {
			ids = append(ids, id)
		}
	}
	slices.Sort(ids)
	if a.panels.mounted != "" && !strings.HasPrefix(a.panels.mounted, terminalPanelPrefix) {
		ids = append([]string{a.panels.mounted}, ids...)
	}
	return ids
}

// ConversationTabs are a conversation's panel tabs, shown or not.
func (a *App) ConversationTabs(id string) []panel.Tab {
	if id == a.panels.mounted {
		return a.Panel.Tabs
	}
	if kept := a.panels.parked[id]; kept != nil {
		return kept.Tabs
	}
	return nil
}

// ConversationActiveTab is the tab a conversation's panel shows, on screen or
// not.
func (a *App) ConversationActiveTab(id string) panel.Tab {
	if id == a.panels.mounted {
		return a.Panel.Active
	}
	if kept := a.panels.parked[id]; kept != nil {
		return kept.Active
	}
	return panel.Tab{}
}

// AllPanelTabs are the panel tabs of every pane, the shown one's first.
func (a *App) AllPanelTabs() []panel.Tab {
	tabs := slices.Clone(a.Panel.Tabs)
	ids := make([]string, 0, len(a.panels.parked))
	for id := range a.panels.parked {
		ids = append(ids, id)
	}
	slices.Sort(ids)
	for _, id := range ids {
		tabs = append(tabs, a.panels.parked[id].Tabs...)
	}
	return tabs
}

// HasPanelTab reports whether any conversation's panel has t.
func (a *App) HasPanelTab(t panel.Tab) bool {
	if slices.Contains(a.Panel.Tabs, t) {
		return true
	}
	for _, kept := range a.panels.parked {
		if slices.Contains(kept.Tabs, t) {
			return true
		}
	}
	return false
}

func (a *App) noteTerminalClose(t panel.Tab) {
	if a.panels.terminalCloses == nil {
		a.panels.terminalCloses = map[panel.Tab]string{}
	}
	a.panels.terminalCloses[t] = a.panels.mounted
}

// CancelTerminalClose clears a failed close so a later shell exit stays passive.
func (a *App) CancelTerminalClose(id string) {
	delete(a.panels.terminalCloses, panel.Tab{Kind: panel.Terminal, ID: id})
}

// DropPanelTab takes t out of the panel that has it, shown or not. A Node
// acknowledgment of an explicit terminal close can also fold its panel.
func (a *App) DropPanelTab(t panel.Tab) {
	owner, confirmed := a.panels.terminalCloses[t]
	delete(a.panels.terminalCloses, t)
	if confirmed && slices.Contains(a.ConversationTabs(owner), t) && len(a.ConversationTabs(owner)) == 1 {
		if owner == a.panels.mounted {
			a.foldConversationPanel()
		} else {
			a.panels.parked[owner].Open = false
		}
	}
	a.dropPanelTab(t)
}

func (a *App) dropPanelTab(t panel.Tab) {
	if slices.Contains(a.Panel.Tabs, t) {
		a.Panel.Tabs, a.Panel.Active = panel.Closed(a.Panel.Tabs, t, a.Panel.Active)
	}
	for _, kept := range a.panels.parked {
		if slices.Contains(kept.Tabs, t) {
			kept.Tabs, kept.Active = panel.Closed(kept.Tabs, t, kept.Active)
		}
	}
}

// MovePanelTab gives t to conversation id's panel, behind its shown tab: a
// tab opened for a conversation that is not on screen, as the page its agent
// opened.
func (a *App) MovePanelTab(t panel.Tab, id string) {
	if id == "" || slices.Contains(a.ConversationTabs(id), t) {
		return
	}
	a.dropPanelTab(t)
	if id == a.panels.mounted {
		a.Panel.Tabs = panel.Opened(a.Panel.Tabs, t)
		return
	}
	kept := a.panels.parked[id]
	if kept == nil {
		kept = &ConversationPanel{}
		a.park(id, kept)
	}
	kept.Tabs = panel.Opened(kept.Tabs, t)
	if kept.Active == (panel.Tab{}) {
		kept.Active = t
	}
}

// DropPanel forgets a conversation's panel, as when the conversation is
// deleted or archived. The owner of what its tabs show closes that first.
func (a *App) DropPanel(id string) {
	delete(a.panels.parked, id)
	if id == a.panels.mounted {
		a.Panel.Mount(nil, panel.Tab{})
	}
}

// dropTerminalPanel forgets main terminal pane id's panel with its surface.
// The processes of its terminal tabs stay in their Node's inventory.
func (a *App) dropTerminalPanel(id string) {
	owner := terminalPanel(id)
	a.DropPanel(owner)
	if owner == a.panels.mounted {
		a.PanelOpen = false
	}
}

// savedPanels are the panels that come back after a relaunch: every
// conversation's but the shown one's, which the window state's own fields
// keep. A live window's unsent chats are not kept: their IDs do not outlive
// the window.
func (a *App) savedPanels() map[string]ConversationPanel {
	var saved map[string]ConversationPanel
	for id, kept := range a.panels.parked {
		if !a.keepsPanel(id) {
			continue
		}
		tabs := keptTabs(kept.Tabs)
		if len(tabs) == 0 && !kept.Open {
			continue
		}
		if saved == nil {
			saved = map[string]ConversationPanel{}
		}
		saved[id] = ConversationPanel{Tabs: tabs, Active: savedPanelTab(tabs, kept.Active), Open: kept.Open, Width: kept.Width}
	}
	return saved
}

// keepsPanel reports whether a conversation's panel is saved.
func (a *App) keepsPanel(id string) bool {
	if terminal, ok := strings.CutPrefix(id, terminalPanelPrefix); ok {
		return a.HasWorkspaceTerminal(terminal)
	}
	return id != "" && (a.Preview || !strings.HasPrefix(id, "draft/"))
}

// restorePanels takes the saved panels at start-up. The tabs of the
// conversation the file names wait for it; a file from before panels belonged
// to conversations names none, and the first conversation shown takes its
// tabs.
func (a *App) restorePanels(p Prefs) {
	width := max(p.PanelWidth, PanelMin)
	if a.panels.mounted != "" || len(a.Panel.Tabs) != 0 || len(a.panels.parked) != 0 {
		// A reload can hide a live panel, but saved tabs cannot replace or
		// resurrect its in-memory tabs.
		a.MountPanel()
		if p.PanelOf == "" || p.PanelOf == a.panels.mounted {
			a.PanelWidth, a.PanelOpen = width, p.PanelOpen
		} else if kept := a.panels.parked[p.PanelOf]; kept != nil {
			kept.Width, kept.Open = width, p.PanelOpen
		}
		return
	}
	a.PanelWidth, a.PanelOpen = width, p.PanelOpen
	tabs := keptTabs(p.PanelTabs)
	if len(p.PanelTabs) == 0 && keeps(p.PanelTab) {
		tabs = []panel.Tab{p.PanelTab}
	}
	active := savedPanelTab(tabs, p.PanelTab)
	if p.PanelOf != "" && p.PanelOf != a.Sidebar.Selected {
		if len(tabs) > 0 || p.PanelOpen {
			a.park(p.PanelOf, &ConversationPanel{Tabs: tabs, Active: active, Open: p.PanelOpen, Width: width})
		}
		a.PanelOpen = false
	} else {
		a.Panel.Mount(tabs, active)
		a.panels.mounted = p.PanelOf
	}
	for id, saved := range p.Panels {
		kept := saved
		kept.Tabs = keptTabs(saved.Tabs)
		kept.Active = savedPanelTab(kept.Tabs, kept.Active)
		if id == p.PanelOf || len(kept.Tabs) == 0 && !kept.Open {
			continue
		}
		a.park(id, &kept)
		a.Panel.Reserve(kept.Tabs...)
	}
	a.Panel.Reserve(tabs...)
}
