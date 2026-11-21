package shell

import (
	"fmt"
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The focused conversation's keys (settings.Listed). They belong to the
// conversation pane the keyboard focus is in: the conversation standing
// alone in the main column, a split's session pane or the enlarged pane.
// Each such pane takes them as Element shortcuts (paneKeys), so a text input
// inside keeps its editing keys (Cmd+Shift and an arrow select), and with the
// focus in the sidebar, the panel, a terminal or a page they go on to the
// window: Cmd+T then opens the panel's browser as before. Cmd+N and Minimize
// come from the native menu, which takes their keys first, and decide by the
// same focus (focusedConversationPane). Cmd stands for Ctrl off macOS.

// WindowMinimize is the Window menu's Minimize: its key enlarges the focused
// conversation pane in a split, and minimizes the window anywhere else.
const WindowMinimize = "window.minimize"

// splitKeys put a new conversation on a side of the focused one, with
// Cmd+Shift.
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
	if a.panes.Group(t) >= 0 && e.Shortcut(ui.Cmd, ui.KeyM) && !c.ShortcutRepeated() {
		a.toggleEnlarged(c, t)
	}
	for _, k := range splitKeys {
		if e.Shortcut(ui.Cmd|ui.Shift, k.key) && !c.ShortcutRepeated() {
			a.newConversationBeside(c, t, k.at)
		}
	}
}

// newConversationFrom opens a fresh conversation where from is, in its
// project or without one, and selects it with the focus in its composer. It
// is never the location's unsent draft, which may be from itself: a new tab
// or split always adds one. Live, the controller creates it
// (Hooks.NewConversationDraft) with the location's composer defaults; the
// preview keeps it in memory as the sidebar's New session does. False when
// none was created.
func (a *App) newConversationFrom(from panes.Target) (panes.Target, bool) {
	pr := a.Data.ProjectOf(from.ID)
	before := a.Sidebar.Selected
	switch {
	case a.Hooks.NewConversationDraft != nil:
		a.Hooks.NewConversationDraft(pr)
	case a.Sidebar.Actions == nil:
		a.Sidebar.Selected = a.sampleConversation(pr)
		if pr != nil {
			a.Sidebar.Reveal(pr.ID)
		}
	}
	id := a.Sidebar.Selected
	if id == before || id == from.ID || a.Data.Find(id) == nil {
		return panes.Target{}, false
	}
	a.Page = ConversationPage
	a.Composer(id).Focus()
	return session(id), true
}

// sampleConversation adds an empty conversation to the preview's data, first
// in pr or among those without a project, and returns its ID.
func (a *App) sampleConversation(pr *preview.Project) string {
	w := &a.panes
	w.created++
	cv := &preview.Conversation{Updated: preview.Now, Created: 20000 + w.created}
	if pr != nil {
		cv.ID = fmt.Sprintf("%s/pane-%d", pr.ID, w.created)
		pr.Conversations = append([]*preview.Conversation{cv}, pr.Conversations...)
	} else {
		cv.ID = fmt.Sprintf("chat/pane-%d", w.created)
		a.Data.Unassigned = append([]*preview.Conversation{cv}, a.Data.Unassigned...)
	}
	return cv.ID
}

// newConversationBeside opens a fresh conversation split at p of from, which
// keeps its place and shares. A full workspace takes none: it says so, and
// nothing is created or selected.
func (a *App) newConversationBeside(c *ui.Context, from panes.Target, p panes.Placement) {
	if len(a.panes.Members(from)) >= panes.Limit {
		kit.Toast(c, kit.Failed, i18n.T(c, "pane_limit", "count", strconv.Itoa(panes.Limit)))
		return
	}
	a.panes.enlarged = panes.Target{}
	if created, ok := a.newConversationFrom(from); ok {
		a.dropPane(c, from, created, "", p)
	}
}

// newConversationTab opens a fresh conversation in a tab of its own: what
// showed alone is kept as a tab, as a split group is one already.
func (a *App) newConversationTab(from panes.Target) {
	w := &a.panes
	created, ok := a.newConversationFrom(from)
	if !ok {
		return
	}
	w.enlarged = panes.Target{}
	if w.Group(from) < 0 {
		w.Keep(from)
	}
	w.Keep(created)
	a.focusPane(created)
}

// toggleEnlarged floats pane t over the window's body, or puts it back. The
// layout under it stays as it was, so its shares, subscriptions and the
// sessions' tasks go on; the composer keeps the focus.
func (a *App) toggleEnlarged(c *ui.Context, t panes.Target) {
	w := &a.panes
	if w.enlarged == t {
		w.enlarged = panes.Target{}
	} else {
		if w.Active != t {
			a.focusPane(t)
		}
		w.enlarged = t
	}
	w.enlargedFocus = t
	c.Invalidate()
}

func (a *App) paneSizeButton(c *ui.Context, t panes.Target, enlarged bool) {
	icon, label := icons.Maximize, "shortcut_pane_enlarge"
	if enlarged {
		icon, label = icons.Minimize, "pane_restore"
	}
	kit.IconButton(c.Key("pane-size"), icon, i18n.T(c, label), 28, false).
		OnClick(func() { a.toggleEnlarged(c, t) })
}

// followEnlarged puts the enlarged pane back once it is no longer the active
// conversation, as another one is chosen or the page changes.
func (a *App) followEnlarged() {
	w := &a.panes
	if e := w.enlarged; e != (panes.Target{}) && (a.Page != ConversationPage || w.Active != e || w.Group(e) < 0 || !a.paneExists(e)) {
		w.enlarged = panes.Target{}
	}
}

// minimize is the Window menu's Minimize: in a focused split conversation its
// key enlarges the pane; anywhere else, and under a dialog, the window
// minimizes as the system item would.
func (a *App) minimize(c *ui.Context) {
	if !a.dialog() {
		if t, ok := a.focusedConversationPane(c); ok && a.panes.Group(t) >= 0 {
			a.toggleEnlarged(c, t)
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
// Kit's floating surface, over a veil as a dialog's: Cmd+M, its restore
// button or a click on the veil puts it back, and Cmd+W closes it as in its
// place.
func (a *App) enlargedPane(c *ui.Context) {
	w := &a.panes
	t := w.enlargedFrame
	cv := a.paneSession(t)
	if t == (panes.Target{}) || cv == nil {
		return
	}
	p := theme.Of(c)
	veil := float32(0.4)
	if !p.Dark {
		veil = 0.25
	}
	restore := func() { a.toggleEnlarged(c, t) }
	ui.Box(c.Key("enlarged-veil")).Absolute().Top(0).Left(0).Fill().ZIndex(2).Background(p.Backdrop().Alpha(veil)).
		Label(i18n.T(c, "pane_restore")).OnClick(restore)
	h := w.handle(t)
	col := ui.Column(c.Key("enlarged")).Absolute().Top(enlargedInset).Left(enlargedInset).Right(enlargedInset).Bottom(enlargedInset).
		ZIndex(3).MinWidth(0).MinHeight(0).AlignItems(ui.Stretch).Clip()
	kit.Frost(c, col.Radius(p.RadiusLg).Border(1, p.SidebarBorder), p.Background, kit.SurfaceTint)
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
			ui.Text(c, a.paneTitle(c, t)).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0).TextColor(p.Primary)
			a.conversationActions(c, cv, 28)
			a.paneSizeButton(c, t, true)
		})
		a.conversation(c, cv, *h)
	})
}
