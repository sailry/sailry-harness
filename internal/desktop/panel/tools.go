package panel

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// page is a browser tab's chrome state: the address as typed and the
// addresses visited, with the one shown.
type page struct {
	address string
	history []string
	at      int // the index in history shown, -1 for none
	// field is the live tab's address field, for its focus.
	field ui.Handle
}

// Go visits an address: the history after the one shown is dropped, as a
// browser does. An empty address does nothing.
func (b *page) Go(address string) bool {
	address = strings.TrimSpace(address)
	if address == "" {
		return false
	}
	if !strings.Contains(address, "://") {
		address = "https://" + address
	}
	b.history = append(b.history[:b.at+1], address)
	b.at = len(b.history) - 1
	b.address = address
	return true
}

// Step goes back (-1) or forward (1) in the history.
func (b *page) Step(d int) {
	if i := b.at + d; i >= 0 && i < len(b.history) {
		b.at, b.address = i, b.history[i]
	}
}

func (s *State) pageOf(id string) *page {
	if s.pagesOf == nil {
		s.pagesOf = map[string]*page{}
	}
	b := s.pagesOf[id]
	if b == nil {
		b = &page{at: -1}
		s.pagesOf[id] = b
	}
	return b
}

// browser is a browser tab: its toolbar, Back and Forward through the tab's
// history, Reload and the address, which goes on Enter, over the page's
// placeholder. ⌘T opens another tab and ⌘R reloads while the focus is in it;
// the panel's "+" is the one button that opens a tab. The preview loads nothing: going and reloading toast so.
func (s *State) browser(c *ui.Context, t Tab) {
	if s.Live && s.BrowserActions != nil {
		s.liveBrowser(c, t)
		return
	}
	p := theme.Of(c)
	b := s.pageOf(t.ID)
	col := ui.Column(c.Key("browser")).Grow(1).MinHeight(0)
	if col.Shortcut(ui.Cmd, ui.KeyT) && !c.ShortcutRepeated() {
		s.New(Browser)
	}
	if col.Shortcut(ui.Cmd, ui.KeyR) && !c.ShortcutRepeated() {
		notice(c)
	}
	col.Children(func() {
		toolbar(c, "toolbar", func() {
			kit.Action(c.Key("back"), icons.ArrowLeft, i18n.T(c, "browser_back"), b.at <= 0).OnClick(func() { b.Step(-1) })
			kit.Action(c.Key("forward"), icons.ArrowRight, i18n.T(c, "browser_forward"), b.at >= len(b.history)-1).OnClick(func() { b.Step(1) })
			kit.Action(c.Key("reload"), icons.Reload, i18n.T(c, "browser_reload"), b.at < 0).OnClick(func() { notice(c) })
			label := i18n.T(c, "browser_address")
			ui.TextInput(c.Key("address"), &b.address).Placeholder(label).Label(label).Grow(1).Height(28).
				OnSubmit(func() {
					if b.Go(b.address) {
						notice(c)
					}
				})
		})
		text := i18n.T(c, "resource_browser_preview")
		if b.at >= 0 {
			text = b.history[b.at] + "\n" + text
		}
		placeholder(c, icons.Web, text, p.Background)
	})
}

// term is a terminal tab's chrome state: its folder and find bar.
type term struct {
	find  bool
	query string
	match int
	cwd   string
}

func (s *State) termOf(id string) *term {
	if s.terms == nil {
		s.terms = map[string]*term{}
	}
	x := s.terms[id]
	if x == nil {
		x = &term{cwd: s.Cwd}
		s.terms[id] = x
	}
	return x
}

// Find opens the active terminal's find bar, reporting whether the active tab
// is a terminal. The window calls it for ⌘F with the focus in the panel, as
// the native menu takes the key first.
func (s *State) Find() bool {
	if s.Active.Kind != Terminal {
		return false
	}
	s.termOf(s.Active.ID).find = true
	return true
}

// terminal is a terminal tab: the folder it runs in, Close and New terminal
// over the screen, and MyGo's find bar at its top right, ⌘F opening it while
// the focus is in the tab. The preview starts no shell and shows the
// screen's placeholder: there is nothing to match. A live tab shows its Node
// terminal through TerminalActions.
func (s *State) terminal(c *ui.Context, t Tab) {
	p := theme.Of(c)
	x := s.termOf(t.ID)
	col := ui.Column(c.Key("terminal")).Grow(1).MinHeight(0)
	if col.Shortcut(ui.Cmd, ui.KeyF) && !c.ShortcutRepeated() {
		x.find = true
	}
	if s.TerminalActions != nil && s.TerminalActions.Folder != nil {
		if folder, known := s.TerminalActions.Folder(t.ID); known {
			x.cwd = folder
		}
	}
	col.Children(func() {
		toolbar(c, "toolbar", func() {
			ui.Icon(c, icons.Terminal).FontSize(14).TextColor(p.MutedForeground)
			ui.Text(c, x.cwd).FontSize(14).SingleLine().Grow(1).Shrink(1).TextColor(p.MutedForeground)
			kit.Action(c.Key("find"), icons.Search, i18n.T(c, "terminal_search_placeholder"), false).OnClick(func() { x.find = true })
			kit.Action(c.Key("close"), icons.Close, i18n.T(c, "workspace_close_terminal"), false).OnClick(func() {
				if s.TerminalActions != nil && s.TerminalActions.Close != nil {
					s.TerminalActions.Close(t.ID)
					return
				}
				s.Close(t)
			})
			kit.IconButtonHint(c.Key("new"), icons.Plus, i18n.T(c, "workspace_new_terminal"), "", 28, false).OnClick(func() { s.New(Terminal) })
		})
		ui.Column(c.Key("screen")).Grow(1).MinHeight(0).Children(func() {
			if s.TerminalActions != nil && s.TerminalActions.View != nil {
				s.TerminalActions.View(c, t.ID)
			} else {
				placeholder(c, icons.Terminal, i18n.T(c, "workspace_terminal_preview"), p.Muted)
			}
			count := 0
			if s.TerminalActions != nil && s.TerminalActions.Search != nil {
				count, x.match = s.TerminalActions.Search(t.ID, x.query, x.find, x.match)
			}
			w, _ := c.Size()
			bar := kit.Floating(c, ui.FindBar(c.Key("find-bar"), &x.find, &x.query, count, &x.match)).
				Absolute().Top(4).Right(8).Width(min(420, w-32)).Radius(p.Radius)
			if bar.OverlayShortcut(0, ui.KeyEscape) {
				x.find = false
			}
		})
	})
}
