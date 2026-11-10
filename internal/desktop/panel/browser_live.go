package panel

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// BrowserPage is what a live browser tab's chrome shows of its page.
type BrowserPage struct {
	// Address is the page's address, "" before a first one.
	Address       string
	Back, Forward bool
}

// BrowserActions connects the browser tabs to their pages. View builds a
// tab's page and reports whether there is one to show.
type BrowserActions struct {
	View     func(*ui.Context, string) bool
	Page     func(string) BrowserPage
	Navigate func(id, address string)
	Back     func(string)
	Forward  func(string)
	Reload   func(string)

	shown map[string]string // the address each tab's field last took from its page
}

// AddBrowser opens another browser tab, shown or behind the active one, and
// returns it.
func (s *State) AddBrowser(show bool) Tab {
	s.pages++
	t := Tab{Browser, fmt.Sprint(s.pages)}
	if show {
		s.Show(t)
	} else {
		s.Tabs = Opened(s.Tabs, t)
	}
	return t
}

// liveBrowser is the browser tab's chrome over its page: Back and Forward
// follow the page's history, Reload and ⌘R reload it, and the address goes on
// Enter, as https:// when it names no scheme.
func (s *State) liveBrowser(c *ui.Context, t Tab) {
	a := s.BrowserActions
	b := s.pageOf(t.ID)
	page := a.Page(t.ID)
	if a.shown == nil {
		a.shown = map[string]string{}
	}
	if a.shown[t.ID] != page.Address {
		a.shown[t.ID], b.address = page.Address, page.Address
	}
	col := ui.Column(c.Key("browser")).Grow(1).MinHeight(0)
	if col.Shortcut(ui.Cmd, ui.KeyT) && !c.ShortcutRepeated() {
		s.New(Browser)
	}
	if col.Shortcut(ui.Cmd, ui.KeyR) && !c.ShortcutRepeated() && page.Address != "" {
		a.Reload(t.ID)
	}
	col.Children(func() {
		toolbar(c, "toolbar", func() {
			kit.Action(c.Key("back"), icons.ArrowLeft, i18n.T(c, "browser_back"), !page.Back).OnClick(func() { a.Back(t.ID) })
			kit.Action(c.Key("forward"), icons.ArrowRight, i18n.T(c, "browser_forward"), !page.Forward).OnClick(func() { a.Forward(t.ID) })
			kit.Action(c.Key("reload"), icons.Reload, i18n.T(c, "browser_reload"), page.Address == "").OnClick(func() { a.Reload(t.ID) })
			label := i18n.T(c, "browser_address")
			ui.TextInput(c.Key("address"), &b.address).Placeholder(label).Label(label).Grow(1).Height(28).
				OnSubmit(func() {
					address := strings.TrimSpace(b.address)
					if address == "" {
						return
					}
					if !strings.Contains(address, "://") {
						address = "https://" + address
					}
					b.address = address
					a.Navigate(t.ID, address)
				})
			kit.IconButtonHint(c.Key("new"), icons.Plus, i18n.T(c, "browser_new_tab"), kit.KeyText(ui.Cmd, ui.KeyT), 28, false).
				OnClick(func() { s.New(Browser) })
		})
		if !a.View(c, t.ID) {
			kit.Result(c, kit.ResultProps{Icon: icons.Web, Title: i18n.T(c, "unavailable")}).Grow(1).Justify(ui.Center)
		}
	})
}
