package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The split workspace in the main column: the active pane's group of sessions
// and terminals, laid out by its tree with Kit's split handles between them,
// each pane with its header, and the zones a session row or a
// pane's header drops on. With the active pane standing alone, the main
// column shows the selected conversation as before, and takes drops that
// start a split. Independent terminals also stand alone in this column;
// the right panel's terminal tabs stay outside the workspace.

// workspace is the split workspace's state in the window.
type workspace struct {
	panes.Workspace
	// focus is each pane's focus handle, which a click in it takes.
	focus map[panes.Target]*ui.Handle
	// seen is the sidebar's selection the active pane last followed.
	seen string
	// terminals are the main workspace's terminal surfaces, independent of
	// conversation-owned right-panel tabs.
	terminals []WorkspaceTerminal
	// groupFocus remembers the last focused member of each split group.
	groupFocus map[panes.Target]panes.Target
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
// surface is gone leave their group.
func (a *App) followPanes() {
	w := &a.panes
	if a.Sidebar.Selected != w.seen {
		w.seen = a.Sidebar.Selected
		a.focusPane(session(w.seen))
	}
	a.prunePanes()
}

// prunePanes takes the panes that are not there out of their groups. A
// terminal without its workspace surface was closed. A session may not be loaded
// yet, as a remote host's before it connects: its pane is held and comes back
// with it (panes.Follow).
func (a *App) prunePanes() {
	w := &a.panes
	w.Follow(func(t panes.Target) bool { return t.Kind != panes.Session || a.paneExists(t) })
	if len(w.Groups) > 0 {
		w.Prune(a.paneExists)
	}
	if w.Active.Kind == panes.Terminal && !a.paneExists(w.Active) {
		a.focusPane(session(a.Sidebar.Selected))
	}
}

// paneExists reports whether t's session or main terminal surface is still there.
func (a *App) paneExists(t panes.Target) bool {
	if t.Kind == panes.Terminal {
		return a.HasWorkspaceTerminal(t.ID)
	}
	return a.Data.Find(t.ID) != nil
}

// focusPane makes t the active pane: a session's becomes the sidebar's
// selection, as focus_pane opens its session scope.
func (a *App) focusPane(t panes.Target) {
	w := &a.panes
	a.rememberGroupFocus(w.Active)
	w.Active = t
	a.rememberGroupFocus(t)
	if t.Kind == panes.Session {
		a.Sidebar.Selected, w.seen = t.ID, t.ID
	}
}

// findTarget is the conversation Find in conversation searches: the selected
// one alone, the focused pane's in a split, and none while that pane is a
// terminal.
func (a *App) findTarget() string { return a.FocusedConversation() }

// canDropPane excludes right-panel terminals, whose tabs never join a split.
func (a *App) canDropPane(dest, t panes.Target, p panes.Placement) bool {
	return (t.Kind != panes.Terminal || a.HasWorkspaceTerminal(t.ID)) &&
		(dest.Kind != panes.Terminal || a.HasWorkspaceTerminal(dest.ID)) && a.panes.CanDrop(dest, t, p)
}

// dropPane puts t on dest at p and gives it the focus.
func (a *App) dropPane(c *ui.Context, dest, t panes.Target, p panes.Placement) {
	if !a.canDropPane(dest, t, p) || !a.panes.Drop(dest, t, p) {
		return
	}
	if p != panes.Center {
		a.panes.splits++
	}
	a.focusPane(t)
	a.panes.handle(a.panes.Active).Focus(c)
}

// closePane takes t out of its split: it stands alone again and the focus
// goes to the group's next pane. A terminal's standalone surface closes too;
// its Node process keeps running and can be reopened from the project.
func (a *App) closePane(c *ui.Context, t panes.Target) {
	w := &a.panes
	if t.Kind == panes.Terminal {
		a.DropWorkspaceTerminal(t.ID)
	} else if !w.Detach(t) {
		return
	}
	a.focusPane(w.Active)
	if w.Group(w.Active) >= 0 {
		w.handle(w.Active).Focus(c)
	} else {
		a.mainPane.Focus(c)
	}
}

// mainColumn builds the main column's content; while the conversation shows
// alone, the column takes drops that split it.
func (a *App) mainColumn(c *ui.Context, main ui.Element, f frame) {
	if a.Page != ConversationPage || a.panes.Displayed() != nil || a.panes.Active.Kind == panes.Terminal || a.Data.Find(a.Sidebar.Selected) == nil {
		main.Children(func() { a.page(c) })
		return
	}
	dest := session(a.Sidebar.Selected)
	d, z, ok := kit.DockDrop(c, main, func(d panes.Drag, z kit.Zone) bool {
		return a.canDropPane(dest, d.Target, placement(z))
	}, func() {
		// Layout once at the destination width. The surrounding columns
		// slide, while the transcript moves without invalidating row heights.
		ui.Column(c.Key("conversation-page")).Width(f.mainWidth).Shrink(0).Grow(1).MinHeight(0).
			Translate(f.mainShift, 0).Children(func() { a.page(c) })
	})
	if ok {
		a.dropPane(c, dest, d.Target, placement(z))
	}
}

// split builds the displayed group or independent terminal in the main column.
// A transcript in a split has no navigation rail.
func (a *App) split(c *ui.Context) bool {
	g := a.panes.Displayed()
	a.Messages.Split = g != nil
	if g == nil {
		if t := a.panes.Active; t.Kind == panes.Terminal && a.HasWorkspaceTerminal(t.ID) {
			a.paneView(c, t)
			return true
		}
		return false
	}
	a.paneNode(c, g)
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
// session's title or the independent terminal's.
func (a *App) paneTitle(c *ui.Context, t panes.Target) string {
	if t.Kind == panes.Terminal {
		if terminal := a.workspaceTerminal(t.ID); terminal != nil && terminal.Title != "" {
			return terminal.Title
		}
		return i18n.T(c, "terminal")
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
	a.pane(c, col, h, true, func() { a.closePane(c, t) })
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
		return a.canDropPane(t, d.Target, placement(z))
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
			OnClick(func() { a.closePane(c, t) })
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
	if a.Demo() {
		a.Panel.TerminalView(c, id)
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
// terminal. The new terminal is another independent main surface.
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
	project := ""
	if terminal := a.workspaceTerminal(t.ID); terminal != nil {
		project = terminal.project
	}
	id := a.sampleWorkspaceTerminal(project)
	a.dropPane(c, t, panes.Target{Kind: panes.Terminal, ID: id}, p)
}
