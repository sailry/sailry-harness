package shell

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/panel"
	"github.com/sailry/sailry-go/internal/panes"
	"github.com/sailry/sailry-go/internal/sidebar"
	"github.com/sailry/sailry-go/internal/theme"
)

// The split workspace in the main column (panes.rs, panes/skin.rs,
// shell/splits.rs): the active pane's group of sessions and terminals,
// laid out by its tree with Kit's split handles between them, each pane
// with its header, and the zones a session row, a terminal tab or a
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

// followPanes keeps the workspace in step with the window: a session
// chosen in the sidebar becomes the active pane (shell/splits.rs
// split_content opens the active session's pane), and panes whose
// session or terminal tab is gone leave their group.
func (a *App) followPanes() {
	w := &a.panes
	if a.Sidebar.Selected != w.seen {
		w.seen = a.Sidebar.Selected
		w.Active = session(w.seen)
	}
	if len(w.Groups) > 0 {
		w.Prune(a.paneExists)
		a.standAlone()
	}
}

// paneExists reports whether t's session or terminal tab is still there.
func (a *App) paneExists(t panes.Target) bool {
	if t.Kind == panes.Terminal {
		return slices.Contains(a.Panel.Tabs, panel.Tab{Kind: panel.Terminal, ID: t.ID})
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
	a.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: w.Active.ID})
	a.PanelOpen = true
	w.Active = session(a.Sidebar.Selected)
}

// focusPane makes t the active pane (panes.rs focus): a session's becomes
// the sidebar's selection, as focus_pane opens its session scope.
func (a *App) focusPane(t panes.Target) {
	w := &a.panes
	w.Active = t
	if t.Kind == panes.Session {
		a.Sidebar.Selected, w.seen = t.ID, t.ID
	}
}

// dropPane puts t on dest at p and gives it the focus (splits.rs
// split_event Drop, panes.rs drop_to).
func (a *App) dropPane(c *ui.Context, dest, t panes.Target, p panes.Placement) {
	if !a.panes.Drop(dest, t, p) {
		return
	}
	a.focusPane(t)
	a.standAlone()
	a.panes.handle(a.panes.Active).Focus(c)
}

// closePane takes t out of its split (resources/close.rs close_pane):
// it stands alone again and the focus goes to the group's next pane. A
// session keeps its history and a terminal its tab: the preview closes
// nothing else.
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

// requestClose is a pane's close button and ⌘W in it (skin.rs
// RequestClose, Close): a terminal asks first, as closing one is the
// Node's to confirm, then leaves the split.
func (a *App) requestClose(c *ui.Context, t panes.Target) {
	if t.Kind == panes.Terminal {
		a.panes.closing = t
		return
	}
	a.closePane(c, t)
}

// mainColumn builds the main column's content; while the conversation
// shows alone, the column takes drops that split it (panes.rs
// standalone: a workspace of one pane is a dock area too).
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

// split builds the displayed group in the main column, reporting false
// when the active pane stands alone. A transcript in a split has no
// navigation rail (live/navigation.rs set_split).
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

// paneView is one pane of a split: focused by a click in it, closed by
// ⌘W in it, its header over its content, and the zones that take drops.
// A terminal pane splits beside itself with a new terminal on ⌘D and ⌘⇧D
// (terminal.rs SplitTerminalRight, SplitTerminalDown).
func (a *App) paneView(c *ui.Context, t panes.Target) {
	w := &a.panes
	h := w.handle(t)
	col := ui.Column(c.Key(t)).Grow(1).MinWidth(0).MinHeight(0).AlignItems(ui.Stretch)
	a.pane(c, col, h, true, func() { a.requestClose(c, t) })
	if w.Active != t && c.FocusWithin(*h) {
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
			terminalPane(c)
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

// paneHeader is a pane's header in a split (skin.rs render_tab_bar): the
// title, then the handle that drags the pane to another place and the
// close button, on the header band's height.
func (a *App) paneHeader(c *ui.Context, t panes.Target) {
	p := theme.Of(c)
	title := a.paneTitle(c, t)
	ui.Row(c.Key("pane-header")).Height(HeaderHeight).Shrink(0).PaddingX(12).Gap(4).Children(func() {
		ui.Text(c, title).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0).TextColor(p.Foreground)
		drag := kit.IconButton(c.Key("pane-drag"), icons.Move, i18n.T(c, "pane_drag"), 28, false).Cursor(ui.CursorGrab)
		drag.Drag(panes.Drag{Target: t, Title: title})
		kit.DragChip(c, drag, title)
		kit.IconButton(c.Key("pane-close"), icons.Close, i18n.T(c, "close"), 28, false).
			OnClick(func() { a.requestClose(c, t) })
	})
}

// terminalPane is a terminal's pane: a static preview, marked as one, as
// the panel's terminal tab shows; no shell starts. Rendering the real
// terminal in a pane comes with B10.
func terminalPane(c *ui.Context) {
	p := theme.Of(c)
	ui.Column(c.Key("terminal")).Grow(1).MinHeight(0).Background(p.Muted).Center().Gap(8).Padding(24).Children(func() {
		ui.Icon(c, icons.Terminal).FontSize(32).TextColor(p.MutedForeground)
		kit.Tag(c, i18n.T(c, "preview"))
		ui.Text(c, i18n.T(c, "workspace_terminal_preview")).FontSize(14).TextAlign(ui.Center).TextColor(p.MutedForeground)
	})
}

// splitTerminal opens a new terminal beside terminal pane t (live/
// terminal.rs split_terminal), within the workspace's limit; the preview
// terminal is a tab of the right panel, as one the panel's + opens.
func (a *App) splitTerminal(c *ui.Context, t panes.Target, p panes.Placement) {
	if len(a.panes.Members(t)) >= panes.Limit {
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
