package desktop

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func renderTab(c *ui.Context, state *panel.State, tab panel.Tab) {
	if !state.Live {
		panel.RenderBrowserSample(c, state, tab)
		return
	}
	a := state.BrowserActions
	if a == nil {
		kit.Result(c, kit.ResultProps{Icon: icons.Web, Title: i18n.T(c, "unavailable")}).Grow(1).Justify(ui.Center)
		return
	}
	b := state.BrowserAddress(c, tab)
	page := a.Page(tab.ID)
	loaded := page.Address != "" && !page.Loading && page.Error == ""
	pickable := a.Pick != nil && (loaded || page.Picking)
	reload := func() {
		if page.Loading {
			a.Stop(tab.ID)
		} else if page.Address != "" {
			a.Reload(tab.ID)
		}
	}
	col := ui.Column(c.Key("browser")).Grow(1).MinHeight(0)
	if col.Shortcut(ui.Cmd, ui.KeyR) && !c.ShortcutRepeated() && page.Address != "" {
		a.Reload(tab.ID)
	}
	if col.Shortcut(ui.Cmd|ui.Shift, ui.KeyC) && !c.ShortcutRepeated() && pickable {
		a.Pick(tab.ID)
	}
	if col.Shortcut(ui.Cmd|ui.Shift, ui.KeyI) && !c.ShortcutRepeated() && page.Address != "" && a.Inspect != nil {
		a.Inspect(tab.ID)
	}
	col.Children(func() {
		ui.Row(c.Key("toolbar")).Height(40).Shrink(0).PaddingX(8).Gap(4).BorderWidth(0, 0, 1, 0).BorderColor(theme.Of(c).Border).Children(func() {
			kit.Action(c.Key("back"), icons.ArrowLeft, i18n.T(c, "browser_back"), !page.Back).OnClick(func() { a.Back(tab.ID) })
			kit.Action(c.Key("forward"), icons.ArrowRight, i18n.T(c, "browser_forward"), !page.Forward).OnClick(func() { a.Forward(tab.ID) })
			if page.Loading {
				kit.Action(c.Key("reload"), icons.Close, i18n.T(c, "browser_stop"), false).OnClick(reload)
			} else {
				kit.Action(c.Key("reload"), icons.Reload, i18n.T(c, "browser_reload"), page.Address == "").OnClick(reload)
			}
			label := i18n.T(c, "browser_address")
			field := ui.TextInput(c.Key("address"), &b.Text).Placeholder(label).Label(label).Grow(1).Height(28).Bind(&b.Field)
			if b.Shown != page.Address && !field.Changed() && b.Text == b.Shown && !field.Composing() {
				b.Shown, b.Text = page.Address, page.Address
				c.Invalidate()
			}
			if b.Text != b.Shown {
				field.OnShortcut(0, ui.KeyEscape, func() { b.Shown, b.Text = page.Address, page.Address })
			}
			field.OnSubmit(func() {
				address, ok := panel.BrowserDestination(b.Text)
				if !ok {
					if strings.TrimSpace(b.Text) != "" {
						kit.Toast(c, kit.Failed, i18n.T(c, "browser_invalid_address"))
					}
					return
				}
				if a.Navigate(tab.ID, address) {
					b.Shown, b.Text = address, address
				}
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
				x.OnClick(func() { a.Pick(tab.ID) })
			}
			kit.MenuButton(c.Key("more"), "browser-more", func(x ui.Element) {
				kit.IconLook(c, x, icons.Ellipsis, i18n.T(c, "more"), "", 28, false)
			}, func(m *kit.Menu) {
				if a.Inspect != nil && m.Item(i18n.T(c, "browser_inspect")).Icon(icons.Code).
					Shortcut(kit.KeyText(ui.Cmd|ui.Shift, ui.KeyI)).Disabled(page.Address == "").Chosen() {
					a.Inspect(tab.ID)
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
			kit.Result(c, kit.ResultProps{Status: kit.ResultError, Title: i18n.T(c, page.Error), SubTitle: page.Address, Extra: func() {
				kit.Secondary(c.Key("retry"), i18n.T(c, "browser_reload")).OnClick(func() { a.Reload(tab.ID) })
			}}).Grow(1).Justify(ui.Center)
			return
		}
		if !a.View(c, tab.ID) {
			kit.Result(c, kit.ResultProps{Icon: icons.Web, Title: i18n.T(c, "browser_unavailable")}).Grow(1).Justify(ui.Center)
		}
	})
}
