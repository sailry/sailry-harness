package sidebar

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Terminal is a running terminal in one of a project's worktrees.
type Terminal struct {
	ID, Title, Path string
}

// terminals are a project's terminal rows after its sessions, in the order
// chosen. Terminals are never archived, so the archived view has none.
func (s *State) terminals(c *ui.Context, pr *preview.Project) {
	if s.Archived || s.Actions == nil || s.Actions.Terminals == nil {
		return
	}
	list := SortedFunc(s.Actions.Terminals(pr), s.sort(), func(t Terminal) Entry { return Entry{ID: t.ID, Name: t.Title} })
	for _, t := range list {
		s.terminal(c, t)
	}
}

// terminal is a terminal's row, a session row's look with the terminal's
// mark in the status slot. Clicking it shows the terminal; its right-click
// menu is the system's.
func (s *State) terminal(c *ui.Context, t Terminal) {
	p := theme.Of(c)
	title := t.Title
	if title == "" {
		title = i18n.T(c, "terminal")
	}
	b := ui.ButtonBase(c.Key("terminal/" + t.ID)).Label(title).Height(rowHeight).PaddingStart(indent).PaddingEnd(8).Gap(8).Radius(p.Radius)
	hovered := b.Hovered()
	b.Background(p.SidebarItem(false, hovered))
	text := p.MutedForeground
	if hovered {
		text = p.SidebarForeground
	}
	b.ContextMenu(func(m *ui.Menu) { s.terminalMenu(c, m, t, title) }).OnClick(func() { s.openTerminal(t.ID) })
	b.TextColor(text).Children(func() {
		ui.Icon(c, icons.Terminal).FontSize(12).TextColor(p.MutedForeground).
			Absolute().Left(guideX - 6).Top(rowHeight/2 - 6).PassThrough()
		kit.Level(c, ui.Text(c, title).FontSize(14).SingleLine().Grow(1).Shrink(1), 14, title)
	})
}

func (s *State) openTerminal(id string) {
	if s.Actions != nil && s.Actions.OpenTerminal != nil {
		s.Actions.OpenTerminal(id)
	}
}

// terminalMenu opens the terminal or closes it, which ends its shell on the
// Node once confirmed, as closing its tab does.
func (s *State) terminalMenu(c *ui.Context, m *ui.Menu, t Terminal, title string) {
	if m.Item(i18n.T(c, "directory_open")).Disabled(s.Actions.OpenTerminal == nil).Chosen() {
		s.openTerminal(t.ID)
	}
	m.Separator()
	closeTerminal := s.Actions.CloseTerminal
	if !m.Item(i18n.T(c, "workspace_close_terminal")).Disabled(closeTerminal == nil).Chosen() || s.confirm.open {
		return
	}
	id := t.ID
	s.confirm = confirm{
		open: true, title: i18n.T(c, "workspace_close_terminal"), message: title, action: i18n.T(c, "close"),
		run: func() { closeTerminal(id) },
	}
}
