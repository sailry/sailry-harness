package sidebar

import (
	"cmp"
	"regexp"
	"slices"
	"time"

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
	Created         uint64
	Updated         time.Time
}

type projectRow struct {
	conversation *preview.Conversation
	terminal     *Terminal
	title        string
}

func (row projectRow) entry() Entry {
	if cv := row.conversation; cv != nil {
		return Entry{Name: cv.Title, Updated: cv.Updated}
	}
	return Entry{Name: row.title, Updated: row.terminal.Updated}
}

func (s *State) projectRows(pr *preview.Project) []projectRow {
	var terminals []Terminal
	if !s.Archived && s.Actions != nil && s.Actions.Terminals != nil {
		terminals = s.Actions.Terminals(pr)
	}
	// The Node can also supply a manual session order. Merge terminals into
	// that sequence without moving one session past another.
	terminals = slices.Clone(terminals)
	slices.SortStableFunc(terminals, func(a, b Terminal) int { return cmp.Compare(b.Created, a.Created) })
	rows := make([]projectRow, 0, len(pr.Conversations)+len(terminals))
	appendTerminal := func(t *Terminal) {
		title := TerminalTitle(t.Title)
		if title == "" {
			title = s.terminalName
		}
		rows = append(rows, projectRow{terminal: t, title: title})
	}
	for _, cv := range pr.Conversations {
		if cv.Archived != s.Archived {
			continue
		}
		for len(terminals) > 0 && terminals[0].Created > uint64(cv.Created) {
			appendTerminal(&terminals[0])
			terminals = terminals[1:]
		}
		rows = append(rows, projectRow{conversation: cv})
	}
	for i := range terminals {
		appendTerminal(&terminals[i])
	}
	sortEntries(rows, s.sort(), projectRow.entry)
	return rows
}

// terminal is a terminal's row, a session row's look: its title without the
// shell's user@host prefix, and the terminal's mark where a session shows its
// age. It is selected while the workspace shows the terminal. Clicking it
// opens the terminal's kept tab; its right-click menu is the system's.
func (s *State) terminal(c *ui.Context, t Terminal, title string) {
	p := theme.Of(c)
	b := ui.ButtonBase(c.Key("terminal/" + t.ID)).Label(title).Height(rowHeight).PaddingStart(indent).PaddingEnd(8).Gap(8).Radius(p.Radius)
	hovered, selected := b.Hovered(), s.Terminal == t.ID
	b.Background(p.SidebarItem(selected, hovered))
	text := p.MutedForeground
	switch {
	case selected:
		b.AccessSelected(true)
		text = p.SidebarAccentForeground
	case hovered:
		text = p.SidebarForeground
	}
	b.ContextMenu(func(m *ui.Menu) { s.terminalMenu(c, m, t, title) }).OnClick(func() { s.openTerminal(t.ID) })
	b.TextColor(text).Children(func() {
		kit.Level(c, ui.Text(c, title).FontSize(14).SingleLine().Grow(1).Shrink(1), 14, title)
		ui.Icon(c, icons.Terminal).FontSize(12).TextColor(p.MutedForeground.Alpha(0.6)).PassThrough()
	})
}

// shellPrompt is the user@host: a shell's default window title leads with.
var shellPrompt = regexp.MustCompile(`^[^\s@:]+@[^\s:]+:\s*`)

// TerminalTitle is a terminal's title without its shell's user@host prefix.
func TerminalTitle(title string) string {
	return shellPrompt.ReplaceAllString(title, "")
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
