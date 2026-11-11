package panel

import (
	"fmt"
	"net/url"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// BrowserPage is what a live browser tab's chrome shows of its page.
type BrowserPage struct {
	// Address is the page's address, "" before a first one.
	Address string
	// Title is the page's own title, "" until it gives one.
	Title         string
	Back, Forward bool
	// Loading is set from the start of a navigation until it finishes, fails
	// or is stopped.
	Loading bool
	// Error is the i18n key of what failed, "" while the page is fine. The
	// body shows it in place of the page.
	Error string
	// Picking is set while the page takes the element selection.
	Picking bool
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
	Stop     func(string)
	// Pick starts the element selection in a tab's page, or cancels the one
	// under way.
	Pick func(string)
	// Inspect opens or closes the page's web inspector.
	Inspect func(string)
	// Settings opens the browser's settings.
	Settings func()

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

// BrowserDestination is where text typed in the address field goes: an
// address with a scheme as typed, a host as https:// (http:// on this
// machine), and anything else as a web search. It reports false for nothing
// and for the schemes a browser tab never opens.
func BrowserDestination(text string) (string, bool) {
	text = strings.TrimSpace(text)
	lower := strings.ToLower(text)
	if text == "" || strings.HasPrefix(lower, "javascript:") || strings.HasPrefix(lower, "file:") || strings.HasPrefix(lower, "data:") {
		return "", false
	}
	if strings.Contains(text, "://") {
		return text, true
	}
	local := strings.HasPrefix(lower, "localhost") || strings.HasPrefix(lower, "127.") || strings.HasPrefix(lower, "[::1]")
	if !strings.ContainsAny(text, " \t\r\n") && (strings.Contains(text, ".") || local || strings.HasPrefix(text, "[")) {
		if local {
			return "http://" + text, true
		}
		return "https://" + text, true
	}
	return "https://www.google.com/search?q=" + url.QueryEscape(text), true
}

// liveBrowser is the browser tab's chrome over its page. Back and Forward
// follow the page's history; Reload is Stop while the page loads, under an
// indeterminate bar; the address goes on Enter to BrowserDestination; Select
// element starts or cancels the element selection; the menu holds the web
// inspector and the browser's settings. ⌘T opens another tab, ⌘R reloads,
// ⌘⇧C selects an element and ⌘⇧I opens the web inspector while the focus is
// in the tab. Another tab opens from the panel's "+" alone.
func (s *State) liveBrowser(c *ui.Context, t Tab) {
	a := s.BrowserActions
	b := s.pageOf(t.ID)
	page := a.Page(t.ID)
	if a.shown == nil {
		a.shown = map[string]string{}
	}
	// The field follows the page, but never under the user's typing.
	if a.shown[t.ID] != page.Address && !c.Focused(b.field) {
		a.shown[t.ID], b.address = page.Address, page.Address
	}
	if s.focusAddress && t == s.Active {
		s.focusAddress = false
		b.field.Focus(c)
	}
	loaded := page.Address != "" && !page.Loading && page.Error == ""
	pickable := a.Pick != nil && (loaded || page.Picking)
	reload := func() {
		if page.Loading {
			a.Stop(t.ID)
		} else if page.Address != "" {
			a.Reload(t.ID)
		}
	}
	col := ui.Column(c.Key("browser")).Grow(1).MinHeight(0)
	if col.Shortcut(ui.Cmd, ui.KeyT) && !c.ShortcutRepeated() {
		s.New(Browser)
	}
	if col.Shortcut(ui.Cmd, ui.KeyR) && !c.ShortcutRepeated() && page.Address != "" {
		a.Reload(t.ID)
	}
	if col.Shortcut(ui.Cmd|ui.Shift, ui.KeyC) && !c.ShortcutRepeated() && pickable {
		a.Pick(t.ID)
	}
	if col.Shortcut(ui.Cmd|ui.Shift, ui.KeyI) && !c.ShortcutRepeated() && page.Address != "" && a.Inspect != nil {
		a.Inspect(t.ID)
	}
	col.Children(func() {
		toolbar(c, "toolbar", func() {
			kit.Action(c.Key("back"), icons.ArrowLeft, i18n.T(c, "browser_back"), !page.Back).OnClick(func() { a.Back(t.ID) })
			kit.Action(c.Key("forward"), icons.ArrowRight, i18n.T(c, "browser_forward"), !page.Forward).OnClick(func() { a.Forward(t.ID) })
			if page.Loading {
				kit.Action(c.Key("reload"), icons.Close, i18n.T(c, "browser_stop"), false).OnClick(reload)
			} else {
				kit.Action(c.Key("reload"), icons.Reload, i18n.T(c, "browser_reload"), page.Address == "").OnClick(reload)
			}
			label := i18n.T(c, "browser_address")
			ui.TextInput(c.Key("address"), &b.address).Placeholder(label).Label(label).Grow(1).Height(28).Bind(&b.field).
				OnSubmit(func() {
					address, ok := BrowserDestination(b.address)
					if !ok {
						if strings.TrimSpace(b.address) != "" {
							kit.Toast(c, kit.Failed, i18n.T(c, "browser_invalid_address"))
						}
						return
					}
					b.address = address
					a.Navigate(t.ID, address)
				})
			if a.Pick != nil {
				pick := "browser_annotate"
				if page.Picking {
					pick = "browser_cancel_selection"
				}
				hint := kit.KeyText(ui.Cmd|ui.Shift, ui.KeyC)
				x := kit.IconButtonHint(c.Key("pick"), icons.Target, i18n.T(c, pick), hint, 28, page.Picking).Disabled(!pickable)
				if !pickable {
					x.Opacity(0.5)
				}
				x.OnClick(func() { a.Pick(t.ID) })
			}
			kit.MenuButton(c.Key("more"), "browser-more", func(x ui.Element) {
				kit.IconLook(c, x, icons.Ellipsis, i18n.T(c, "more"), "", 28, false)
			}, func(m *kit.Menu) {
				if a.Inspect != nil && m.Item(i18n.T(c, "browser_inspect")).Icon(icons.Code).
					Shortcut(kit.KeyText(ui.Cmd|ui.Shift, ui.KeyI)).Disabled(page.Address == "").Chosen() {
					a.Inspect(t.ID)
				}
				if a.Settings != nil && m.Item(i18n.T(c, "settings")).Icon(icons.Settings).Chosen() {
					a.Settings()
				}
			})
		})
		if page.Loading {
			ui.Progress(c.Key("loading"), -1).FillWidth().Height(2).Shrink(0)
		}
		if page.Error != "" {
			// A page that failed never stays a blank area: the body says so
			// and offers the reload.
			kit.Result(c, kit.ResultProps{Status: kit.ResultError, Title: i18n.T(c, page.Error), SubTitle: page.Address, Extra: func() {
				kit.Secondary(c.Key("retry"), i18n.T(c, "browser_reload")).OnClick(func() { a.Reload(t.ID) })
			}}).Grow(1).Justify(ui.Center)
			return
		}
		if !a.View(c, t.ID) {
			kit.Result(c, kit.ResultProps{Icon: icons.Web, Title: i18n.T(c, "browser_unavailable")}).Grow(1).Justify(ui.Center)
		}
	})
}
