package shell

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The split workspace in the main column: the active pane's group of sessions
// and terminals, laid out by its tree with Kit's split handles between them,
// each pane with its header, and the zones a session row, a terminal tab or a
// pane's header drops on. With the active pane standing alone, the main
// column shows the selected conversation as before, and takes drops that
// start a split.

// workspace is the split workspace's state in the window.
type workspace struct {
	panes.Workspace
	// focus is each pane's focus handle, which a click in it takes.
	focus map[panes.Target]*ui.Handle
	// seen is the sidebar's selection the active pane last followed.
	seen string
	// closing is the terminal pane whose closing waits to be confirmed.
	closing panes.Target
	// splits counts the panes split off (Usage).
	splits int
}

// handle returns pane t's focus handle.
func (w *workspace) handle(t panes.Target) *ui.Handle {
	if w.focus == nil {
		w.focus = map[panes.Target]*ui.Handle{}
	}
	h := w.focus[t]
	if h == nil {
		h = new(ui.Handle)
		w.focus[t] = h
	}
	return h
}

// session is a session's pane target.
func session(id string) panes.Target { return panes.Target{Kind: panes.Session, ID: id} }

// placement is the model's placement of a dock zone.
func placement(z kit.Zone) panes.Placement {
	return [...]panes.Placement{kit.ZoneCenter: panes.Center, kit.ZoneLeft: panes.Left, kit.ZoneRight: panes.Right,
		kit.ZoneTop: panes.Top, kit.ZoneBottom: panes.Bottom}[z]
}

// followPanes keeps the workspace in step with the window: a session chosen
// in the sidebar becomes the active pane, and panes whose session or terminal
// tab is gone leave their group.
func (a *App) followPanes() {
	w := &a.panes
	if a.Sidebar.Selected != w.seen {
		w.seen = a.Sidebar.Selected
		w.Active = session(w.seen)
	}
	a.prunePanes()
	if len(w.Groups) > 0 {
		a.standAlone()
	}
}

// prunePanes takes the panes that are not there out of their groups. A
// terminal without its tab was closed. A session that is not there may not be
// loaded yet, as a remote host's before it connects: its pane is held and
// comes back with it (panes.Follow).
func (a *App) prunePanes() {
	w := &a.panes
	w.Follow(func(t panes.Target) bool { return t.Kind != panes.Session || a.paneExists(t) })
	if len(w.Groups) > 0 {
		w.Prune(a.paneExists)
	}
}

// paneExists reports whether t's session or terminal tab is still there.
func (a *App) paneExists(t panes.Target) bool {
	if t.Kind == panes.Terminal {
		// A terminal of a conversation that is not shown keeps its pane.
		return a.HasPanelTab(panel.Tab{Kind: panel.Terminal, ID: t.ID})
	}
	return a.Data.Find(t.ID) != nil
}

// standAlone puts the focus back on the selected session when the active
// pane is a terminal left standing alone: a terminal shows alone in the
// right panel's tab, not in the main column, so its tab comes forward.
func (a *App) standAlone() {
	w := &a.panes
	if w.Active.Kind != panes.Terminal || w.Group(w.Active) >= 0 {
		return
	}
	// Only in its own conversation's panel: another conversation's terminal
	// waits in its tab there.
	if tab := (panel.Tab{Kind: panel.Terminal, ID: w.Active.ID}); slices.Contains(a.Panel.Tabs, tab) {
		a.Panel.Show(tab)
		a.PanelOpen = true
	}
	w.Active = session(a.Sidebar.Selected)
}

// focusPane makes t the active pane: a session's becomes the sidebar's
// selection, as focus_pane opens its session scope.
func (a *App) focusPane(t panes.Target) {
	w := &a.panes
	w.Active = t
	if t.Kind == panes.Session {
		a.Sidebar.Selected, w.seen = t.ID, t.ID
	}
}

// findTarget is the conversation Find in conversation searches: the selected
// one alone, the focused pane's in a split, and none while that pane is a
// terminal.
func (a *App) findTarget() string {
	w := &a.panes
	if w.Displayed() == nil {
		return a.Sidebar.Selected
	}
	if w.Active.Kind != panes.Session {
		return ""
	}
	return w.Active.ID
}

// dropPane puts t on dest at p and gives it the focus.
func (a *App) dropPane(c *ui.Context, dest, t panes.Target, p panes.Placement) {
	if !a.panes.Drop(dest, t, p) {
		return
	}
	if p != panes.Center {
		a.panes.splits++
	}
	a.focusPane(t)
	a.standAlone()
	a.panes.handle(a.panes.Active).Focus(c)
}

// closePane takes t out of its split: it stands alone again and the focus
// goes to the group's next pane. A session keeps its history and a terminal
// keeps running in its tab: closing a pane closes nothing else.
func (a *App) closePane(c *ui.Context, t panes.Target) {
	w := &a.panes
	if !w.Detach(t) {
		return
	}
	a.focusPane(w.Active)
	a.standAlone()
	if w.Group(w.Active) >= 0 {
		w.handle(w.Active).Focus(c)
	} else {
		a.mainPane.Focus(c)
	}
}

// requestClose is a pane's close button and ⌘W in it: a terminal asks first,
// as closing one is the Node's to confirm, then leaves the split.
func (a *App) requestClose(c *ui.Context, t panes.Target) {
	if t.Kind == panes.Terminal {
		a.panes.closing = t
		return
	}
	a.closePane(c, t)
}

// mainColumn builds the main column's content; while the conversation shows
// alone, the column takes drops that split it.
func (a *App) mainColumn(c *ui.Context, main ui.Element) {
	if a.Page != ConversationPage || a.panes.Displayed() != nil || a.Data.Find(a.Sidebar.Selected) == nil {
		main.Children(func() { a.page(c) })
		return
	}
	dest := session(a.Sidebar.Selected)
	d, z, ok := kit.DockDrop(c, main, func(d panes.Drag, z kit.Zone) bool {
		return a.panes.CanDrop(dest, d.Target, placement(z))
	}, func() { a.page(c) })
	if ok {
		a.dropPane(c, dest, d.Target, placement(z))
	}
}

// split builds the displayed group in the main column, reporting false when
// the active pane stands alone. A transcript in a split has no navigation
// rail.
func (a *App) split(c *ui.Context) bool {
	g := a.panes.Displayed()
	a.Messages.Split = g != nil
	if g == nil {
		return false
	}
	a.paneNode(c, g)
	a.confirmClose(c)
	return true
}

// paneNode builds a tree node: a pane, or Kit's dock split of its
// children, each keyed by its first pane so it keeps its state as the
// tree changes around it.
func (a *App) paneNode(c *ui.Context, n *panes.Node) {
	if n.Leaf() {
		a.paneView(c, n.Target)
		return
	}
	keys := make([]any, len(n.Children))
	for i, ch := range n.Children {
		keys[i] = ch.Targets()[0]
	}
	kit.DockSplit(c, n.Axis == panes.Column, n.Sizes, keys, func(i int) {
		a.paneNode(c, n.Children[i])
	})
}

// paneTitle is what a pane's header and its drag preview show: the
// session's title or the terminal tab's.
func (a *App) paneTitle(c *ui.Context, t panes.Target) string {
	if t.Kind == panes.Terminal {
		return a.Panel.Title(c, panel.Tab{Kind: panel.Terminal, ID: t.ID})
	}
	if cv := a.Data.Find(t.ID); cv != nil {
		return sidebar.Title(c, cv)
	}
	return ""
}

// paneView is one pane of a split: focused by a click in it, closed by ⌘W in
// it, its header over its content, and the zones that take drops. A terminal
// pane splits beside itself with a new terminal on ⌘D and ⌘⇧D.
func (a *App) paneView(c *ui.Context, t panes.Target) {
	w := &a.panes
	h := w.handle(t)
	col := ui.Column(c.Key(t)).Grow(1).MinWidth(0).MinHeight(0).AlignItems(ui.Stretch)
	a.pane(c, col, h, true, func() { a.requestClose(c, t) })
	// A pane ⌘W just closed still has the focus in this build; the focus
	// goes to the group's next pane, not back to it.
	if w.Active != t && w.Group(t) >= 0 && c.FocusWithin(*h) {
		a.focusPane(t)
	}
	if t.Kind == panes.Terminal {
		switch {
		case col.Shortcut(ui.Cmd|ui.Shift, ui.KeyD):
			a.splitTerminal(c, t, panes.Bottom)
		case col.Shortcut(ui.Cmd, ui.KeyD):
			a.splitTerminal(c, t, panes.Right)
		}
	}
	d, z, ok := kit.DockDrop(c, col, func(d panes.Drag, z kit.Zone) bool {
		return w.CanDrop(t, d.Target, placement(z))
	}, func() {
		a.paneHeader(c, t)
		if t.Kind == panes.Terminal {
			a.terminalPane(c, t.ID)
			return
		}
		if cv := a.Data.Find(t.ID); cv != nil {
			a.conversation(c, cv, *h)
		}
	})
	if ok {
		a.dropPane(c, t, d.Target, placement(z))
	}
}

// paneHeader is a pane's header in a split: the title, a conversation's own
// Find and overview, then the handle that drags the pane to another place and
// the close button, on the header band's height.
func (a *App) paneHeader(c *ui.Context, t panes.Target) {
	p := theme.Of(c)
	title := a.paneTitle(c, t)
	ui.Row(c.Key("pane-header")).Height(HeaderHeight).Shrink(0).PaddingX(12).Gap(4).Children(func() {
		ui.Text(c, title).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0).TextColor(p.Foreground)
		// The band has no Find or overview over a split, where they would
		// name no pane: each conversation's are here, acting on it alone. A
		// terminal has none.
		if t.Kind == panes.Session {
			a.conversationActions(c, a.Data.Find(t.ID), 28)
		}
		drag := kit.IconButton(c.Key("pane-drag"), icons.Move, i18n.T(c, "pane_drag"), 28, false).Cursor(ui.CursorGrab)
		drag.Drag(panes.Drag{Target: t, Title: title})
		kit.DragChip(c, drag, title)
		kit.IconButton(c.Key("pane-close"), icons.Close, i18n.T(c, "close"), 28, false).
			OnClick(func() { a.requestClose(c, t) })
	})
}

// terminalPane is a terminal's pane: the screen of the tab's Node terminal,
// which shows here instead of in its tab while the pane is displayed. The
// preview shows a static placeholder, marked as one; no shell starts.
func (a *App) terminalPane(c *ui.Context, id string) {
	p := theme.Of(c)
	if actions := a.Panel.TerminalActions; actions != nil && actions.Pane != nil {
		ui.Column(c.Key("terminal")).Grow(1).MinHeight(0).Background(p.Background).Children(func() { actions.Pane(c, id) })
		return
	}
	ui.Column(c.Key("terminal")).Grow(1).MinHeight(0).Background(p.Muted).Center().Gap(8).Padding(24).Children(func() {
		ui.Icon(c, icons.Terminal).FontSize(32).TextColor(p.MutedForeground)
		kit.Tag(c, i18n.T(c, "preview"))
		ui.Text(c, i18n.T(c, "workspace_terminal_preview")).FontSize(14).TextAlign(ui.Center).TextColor(p.MutedForeground)
	})
}

// splitTerminal opens a new terminal beside terminal pane t, within the
// workspace's limit: a new Node terminal in t's worktree, or the preview's
// terminal, a tab of the right panel as one the panel's + opens.
func (a *App) splitTerminal(c *ui.Context, t panes.Target, p panes.Placement) {
	if len(a.panes.Members(t)) >= panes.Limit {
		return
	}
	if actions := a.Panel.TerminalActions; actions != nil && actions.Split != nil {
		if id, ok := actions.Split(t.ID); ok {
			a.dropPane(c, t, panes.Target{Kind: panes.Terminal, ID: id}, p)
		}
		return
	}
	a.Panel.New(panel.Terminal)
	a.dropPane(c, t, panes.Target{Kind: panes.Terminal, ID: a.Panel.Active.ID}, p)
}

// confirmClose asks before a terminal pane closes.
func (a *App) confirmClose(c *ui.Context) {
	t := a.panes.closing
	if t == (panes.Target{}) {
		return
	}
	open := true
	kit.Dialog(c, &open, func() {
		ui.Text(c, i18n.T(c, "workspace_close_terminal")).FontSize(16).FontWeight(600)
		ui.Text(c, a.paneTitle(c, t)).FontSize(14).MaxWidth(400).TextColor(theme.Of(c).MutedForeground)
		ui.Row(c).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(func() { a.panes.closing = panes.Target{} })
			kit.Danger(c.Key("close"), i18n.T(c, "close")).OnClick(func() {
				a.panes.closing = panes.Target{}
				a.closePane(c, t)
			})
		})
	})
	if !open {
		a.panes.closing = panes.Target{}
	}
}
