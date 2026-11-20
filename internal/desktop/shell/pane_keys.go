package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The focused conversation's keys (settings.Listed). They belong to the
// conversation pane the keyboard focus is in: the conversation standing
// alone in the main column, a split's session pane or the enlarged pane.
// Each such pane takes them as Element shortcuts (paneKeys), so a text input
// inside keeps its editing keys (⌘⇧ and an arrow select), and with the focus
// in the sidebar, the panel, a terminal or a page they go on to the window:
// ⌘T then opens the panel's browser as before. ⌘N and Minimize come from the
// native menu, which takes their keys first, and decide by the same focus
// (focusedConversationPane).

// WindowMinimize is the Window menu's Minimize: its key enlarges the focused
// conversation pane, and minimizes the window anywhere else.
const WindowMinimize = "window.minimize"

// splitKeys put a new conversation on a side of the focused one, with ⌘⇧.
var splitKeys = []struct {
	key ui.Key
	at  panes.Placement
}{{ui.KeyRight, panes.Right}, {ui.KeyLeft, panes.Left}, {ui.KeyUp, panes.Top}, {ui.KeyDown, panes.Bottom}}

// enlargedInset is the enlarged pane's margin inside the window's body.
const enlargedInset = 12

// focusedConversationPane is the conversation pane the keyboard focus is in,
// false when it is anywhere else or nowhere.
func (a *App) focusedConversationPane(c *ui.Context) (panes.Target, bool) {
	w := &a.panes
	t := w.Active
	if a.Page != ConversationPage || t.Kind != panes.Session || a.Data.Find(t.ID) == nil {
		return panes.Target{}, false
	}
	if c.FocusWithin(*w.handle(t)) {
		return t, true
	}
	return t, w.Displayed() == nil && c.FocusWithin(a.mainPane)
}

// paneKeys has conversation pane e, showing t, take the focused
// conversation's keys while the focus is inside it.
func (a *App) paneKeys(c *ui.Context, e ui.Element, t panes.Target) {
	if e.Shortcut(ui.Cmd, ui.KeyT) && !c.ShortcutRepeated() {
		a.newConversationTab(t)
	}
	if e.Shortcut(ui.Cmd, ui.KeyM) && !c.ShortcutRepeated() {
		a.toggleEnlarged(t)
	}
	for _, k := range splitKeys {
		if e.Shortcut(ui.Cmd|ui.Shift, k.key) && !c.ShortcutRepeated() {
			a.newConversationBeside(c, t, k.at)
		}
	}
}

// newConversationFrom opens a new conversation where from is, in its project
// or without one, by the sidebar's New session: the location's unsent draft
// when it has one. It reports the conversation, false when none other than
// the one shown opened.
func (a *App) newConversationFrom(from panes.Target) (panes.Target, bool) {
	before := a.Sidebar.Selected
	if pr := a.Data.ProjectOf(from.ID); pr != nil {
		a.Sidebar.NewSession(pr)
	} else {
		a.Sidebar.NewChat()
	}
	a.Page = ConversationPage
	t := session(a.Sidebar.Selected)
	return t, t != from && a.Sidebar.Selected != before && a.Data.Find(t.ID) != nil
}

// newConversationBeside opens a new conversation split at p of from, which
// keeps its place and shares. A full workspace shows it alone instead.
func (a *App) newConversationBeside(c *ui.Context, from panes.Target, p panes.Placement) {
	a.panes.enlarged = panes.Target{}
	if created, ok := a.newConversationFrom(from); ok {
		a.dropPane(c, from, created, p)
	}
}

// newConversationTab opens a new conversation in a tab of its own: what
// showed alone is kept as a tab, as a split group is one already.
func (a *App) newConversationTab(from panes.Target) {
	w := &a.panes
	w.enlarged = panes.Target{}
	if w.Group(from) < 0 {
		w.Keep(from)
	}
	created, ok := a.newConversationFrom(from)
	if !ok {
		return
	}
	if w.Group(created) < 0 {
		w.Keep(created)
	}
	a.focusPane(created)
}

// toggleEnlarged floats pane t over the window's body, or puts it back. The
// layout under it stays as it was, so its shares, subscriptions and the
// sessions' tasks go on; the composer keeps the focus.
func (a *App) toggleEnlarged(t panes.Target) {
	w := &a.panes
	if w.enlarged == t {
		w.enlarged = panes.Target{}
	} else {
		w.enlarged = t
	}
	a.Composer(t.ID).Focus()
}

// followEnlarged puts the enlarged pane back once it is no longer the active
// conversation, as another one is chosen or the page changes.
func (a *App) followEnlarged() {
	w := &a.panes
	if e := w.enlarged; e != (panes.Target{}) && (a.Page != ConversationPage || w.Active != e || !a.paneExists(e)) {
		w.enlarged = panes.Target{}
	}
}

// minimize is the Window menu's Minimize: in the focused conversation its
// key enlarges the pane; anywhere else, and under a dialog, the window
// minimizes as the system item would.
func (a *App) minimize(c *ui.Context) {
	if !a.dialog() {
		if t, ok := a.focusedConversationPane(c); ok {
			a.toggleEnlarged(t)
			return
		}
	}
	if a.minimizeWindow != nil {
		a.minimizeWindow()
	}
}

// enlargedSlot is the place the enlarged pane leaves in its layout, which
// keeps its size there.
func (a *App) enlargedSlot(c *ui.Context, t panes.Target) {
	p := theme.Of(c)
	ui.Column(c.Key("enlarged-slot")).Grow(1).MinWidth(0).MinHeight(0).Center().Gap(8).Padding(16).Background(p.Muted).Children(func() {
		ui.Icon(c, icons.Maximize).FontSize(24).TextColor(p.MutedForeground)
		ui.Text(c, a.paneTitle(c, t)).FontSize(14).SingleLine().MaxWidth(320).TextColor(p.MutedForeground)
	})
}

// enlargedPane floats the enlarged conversation over the window's body in
// Kit's floating surface, over a veil as a dialog's: ⌘M, its restore button
// or a click on the veil puts it back, and ⌘W closes it as in its place.
func (a *App) enlargedPane(c *ui.Context) {
	w := &a.panes
	t := w.enlarged
	cv := a.Data.Find(t.ID)
	if t == (panes.Target{}) || cv == nil {
		return
	}
	p := theme.Of(c)
	veil := float32(0.4)
	if !p.Dark {
		veil = 0.25
	}
	restore := func() { a.toggleEnlarged(t) }
	ui.Box(c.Key("enlarged-veil")).Absolute().Top(0).Left(0).Fill().ZIndex(2).Background(p.Backdrop().Alpha(veil)).
		Label(i18n.T(c, "pane_restore")).OnClick(restore)
	h := w.handle(t)
	col := ui.Column(c.Key("enlarged")).Absolute().Top(enlargedInset).Left(enlargedInset).Right(enlargedInset).Bottom(enlargedInset).
		ZIndex(3).MinWidth(0).MinHeight(0).AlignItems(ui.Stretch).Clip()
	kit.Floating(c, col)
	a.pane(c, col, h, true, func() {
		if w.Group(t) >= 0 {
			a.closePane(c, t)
		} else {
			a.closeShownTab(c)
		}
	})
	a.paneKeys(c, col, t)
	col.Children(func() {
		ui.Row(c.Key("pane-header")).Height(HeaderHeight).Shrink(0).PaddingX(12).Gap(4).Children(func() {
			ui.Text(c, a.paneTitle(c, t)).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
			a.conversationActions(c, cv, 28)
			kit.IconButton(c.Key("pane-restore"), icons.Minimize, i18n.T(c, "pane_restore"), 28, false).OnClick(restore)
		})
		a.conversation(c, cv, *h)
	})
}
