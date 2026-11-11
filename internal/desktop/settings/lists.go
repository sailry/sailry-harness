package settings

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The dialogs' shared pieces: a bordered list, account authorization, buttons
// with icons and destructive confirmation.

// Heights of a modal list's rows.
const (
	rowSingle = 53
	rowDouble = 64
)

// modalList is a muted heading with a count and, at its end, the list's
// compact add action; then one bordered box of rows apart by dividers, or
// the empty text in a dashed box. n is the rows' count; action may be nil.
func modalList(c *ui.Context, key any, heading, count, empty string, n int, rows func(), action func()) {
	p := theme.Of(c)
	ui.Column(c.Key(key)).Gap(8).FillWidth().Children(func() {
		if heading != "" || action != nil {
			ui.Row(c.Key("heading")).Gap(8).MinHeight(24).AlignItems(ui.Center).Children(func() {
				ui.Text(c, heading).FontSize(12).Shrink(1).MinWidth(0).SingleLine().TextColor(p.MutedForeground)
				if count != "" {
					ui.Text(c, count).FontSize(12).Shrink(0).TextColor(p.MutedForeground)
				}
				ui.Spacer(c)
				if action != nil {
					action()
				}
			})
		}
		if n == 0 {
			if empty != "" {
				ui.Text(c, empty).FontSize(14).TextAlign(ui.Center).Padding(16).FillWidth().
					Border(1, p.Border).BorderStyle(ui.BorderDashed).Radius(p.RadiusLg).TextColor(p.MutedForeground)
			}
			return
		}
		ui.Column(c.Key("frame")).Border(1, p.Border).Radius(p.RadiusLg).Dividers(1, p.Border).ClipX().Children(rows)
	})
}

// listRow holds a modal list row's options.
type listRow struct {
	subtitle, meta string
	leading        func()
	trailing       func() // independent controls; their clicks do not open the row
	chevron        bool   // the row navigates
	selected       *bool  // single selection: the check of the chosen row; nil for none
	remove         func()
	removeLabel    string
	expanded       *bool  // a disclosure chevron; content shows under the row while true
	content        func() // the expanded content
	disabled       bool
	onClick        func()
}

// row draws a listRow titled title: 53 high, 64 with a subtitle, a ghost
// button of a leading disclosure, the leading slot, a medium title over a
// muted subtitle, a muted meta, then the trailing controls and the mark.
func (r listRow) row(c *ui.Context, key any, title string) {
	p := theme.Of(c)
	h := float32(rowSingle)
	if r.subtitle != "" {
		h = rowDouble
	}
	ui.Column(c.Key(key)).Children(func() {
		// A row with no action is not a button: it takes no hover style.
		var b ui.Element
		if r.onClick != nil {
			b = kit.GhostLook(c, ui.ButtonBase(c.Key("row")).Label(title).Disabled(r.disabled), false).Radius(0).OnClick(r.onClick)
		} else {
			b = ui.Row(c.Key("row")).Center().TextColor(p.Foreground)
		}
		b.Height(h).PaddingX(16).Gap(12).Justify(ui.Start).FillWidth()
		if r.disabled {
			b.Opacity(0.5)
		}
		b.Children(func() {
			if r.expanded != nil {
				chev := icons.ChevronRight
				if *r.expanded {
					chev = icons.ChevronDown
				}
				ui.Icon(c, chev).FontSize(14).Shrink(0).TextColor(p.MutedForeground)
			}
			if r.leading != nil {
				ui.Row(c).Shrink(0).Children(r.leading)
			}
			ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(kit.TitleGap).Children(func() {
				ui.Text(c, title).FontSize(14).FontWeight(500).SingleLine()
				if r.subtitle != "" {
					ui.Text(c, r.subtitle).FontSize(12).SingleLine().TextColor(p.MutedForeground)
				}
			})
			if r.meta != "" {
				ui.Text(c, r.meta).FontSize(12).Shrink(0).TextColor(p.MutedForeground)
			}
			if r.trailing != nil {
				ui.Row(c.Key("trailing")).Shrink(0).Gap(4).Children(r.trailing)
			}
			switch {
			case r.chevron:
				ui.Icon(c, icons.ChevronRight).FontSize(16).Shrink(0).TextColor(p.MutedForeground)
			case r.selected != nil && *r.selected:
				ui.Row(c).Size(20, 20).Shrink(0).Radius(10).Center().Background(p.Primary).Children(func() {
					ui.Icon(c, icons.Check).FontSize(12).TextColor(p.Background)
				})
			case r.remove != nil:
				kit.Action(c.Key("remove"), icons.Close, r.removeLabel, r.disabled).OnClick(r.remove)
			}
		})
		if r.expanded != nil && *r.expanded && r.content != nil {
			ui.Column(c.Key("content")).Padding(0, 16, 16, 40).Children(r.content)
		}
	})
}

// iconButton is a Kit button with a leading icon: primary, or outlined.
func iconButton(c *ui.Context, key any, icon *ui.SVG, label string, primary, disabled bool) ui.Element {
	p := theme.Of(c)
	var b ui.Element
	if primary {
		b = kit.Primary(c.Key(key), label, disabled)
	} else {
		b = ui.ButtonBase(c.Key(key)).Label(label).Disabled(disabled).Radius(p.Radius).Center().Gap(6).PaddingX(12).Height(32).
			Border(1, p.Border).Background(p.ButtonSecondary).TextColor(p.ButtonSecondaryForeground).Cursor(ui.CursorPointer)
		switch {
		case disabled:
			b.Opacity(0.5)
		case b.Pressed():
			b.Background(p.ButtonSecondaryActive)
		case b.Hovered():
			b.Background(p.ButtonSecondaryHover)
		}
	}
	return b.Children(func() {
		if icon != nil {
			ui.Icon(c, icon).FontSize(16)
		}
		ui.Text(c, label).FontSize(14).SingleLine()
	})
}

// confirm is the destructive prompt: target names what goes, consequence
// what follows, once. It reports the user's yes in the pass it is given.
func confirm(c *ui.Context, key any, open *bool, title, consequence, action string) bool {
	if !*open {
		return false
	}
	return kit.AlertDialog(c.Key(key), open, title, consequence, i18n.T(c, "settings_cancel"), action) == 1
}

// Flow is an account authorization's state.
type Flow int

const (
	FlowStarting Flow = iota // no code or link yet
	FlowDevice               // a code to enter at URL
	FlowBrowser              // a page opened in the browser
	FlowExpired              // the code or link can no longer complete
	FlowFailed               // the attempt stopped; the toast told why
)

// Authorization is an account sign-in shown in the shared dialog: the
// flow, its code and URL, and Callback for a pasted redirect.
type Authorization struct {
	Open       bool
	Title      string
	Flow       Flow
	Code, URL  string
	Expires    string // the device code's time left, "" unknown
	Callback   bool   // the flow takes a pasted code or redirect URL
	Pasted     string
	Busy       bool
	Cancelling bool
	OpenURL    func(string)
	Cancel     func()
}

// authorization is a 48 DIP tile, the title, a hint, the code in cells, a
// status line with a spinner, then the flow's actions over a full-width
// Cancel; 400 wide. refresh restarts the flow, submit takes the pasted
// callback.
func authorization(c *ui.Context, key any, a *Authorization, tile func(), refresh, submit func()) {
	if !a.Open {
		return
	}
	p := theme.Of(c)
	t := func(k string, args ...any) string { return i18n.T(c, k, args...) }
	open := a.Open
	kit.Dialog(c.Key(key), &open, func() {
		ui.Column(c.Key("authorization")).AlignItems(ui.Center).Padding(12, 8, 4, 8).Children(func() {
			ui.Row(c).Size(48, 48).Center().Children(tile)
			ui.Text(c, a.Title).FontSize(16).FontWeight(600).SingleLine().Margin(14, 0, 0, 0)
			hint := ""
			switch {
			case a.Code != "" && a.URL != "" && a.Flow != FlowBrowser && a.Flow != FlowStarting:
				hint = t("authorization_enter_code", "host", host(a.URL))
			case a.URL != "" && a.Flow != FlowStarting && a.Flow != FlowDevice:
				hint = t("authorization_browser")
			}
			if hint != "" {
				ui.Text(c, hint).FontSize(12).TextAlign(ui.Center).Margin(4, 0, 0, 0).TextColor(p.MutedForeground)
			}
			if a.Code != "" && a.Flow != FlowBrowser && a.Flow != FlowStarting {
				ui.Row(c.Key("code")).Gap(4).Margin(22, 0, 0, 0).Opacity(map[bool]float32{true: 0.5, false: 1}[a.Flow != FlowDevice]).Children(func() {
					for i, r := range a.Code {
						if r == '-' || r == ' ' {
							ui.Box(c.Key(i)).Width(14)
							continue
						}
						ui.Text(c.Key(i), string(r)).Size(28, 38).FontSize(18).FontWeight(600).Font("monospace").TextAlign(ui.Center).
							Padding(8, 0).Border(1, p.Border).Radius(p.Radius).Background(p.Background)
					}
					kit.Action(c.Key("copy"), icons.Copy, t("authorization_copy_code"), false).OnClick(func() {
						c.WriteClipboard(a.Code)
						kit.Toast(c, kit.Done, t("content_copied"))
					})
				})
			}
			status, spin, warn := "", false, false
			switch a.Flow {
			case FlowStarting:
				status, spin = t("authorization_starting"), true
			case FlowDevice:
				status, spin = t("authorization_waiting"), true
				if a.Expires != "" {
					status = t("authorization_waiting_time", "time", a.Expires)
				}
			case FlowBrowser:
				status, spin = t("authorization_waiting"), true
			case FlowExpired:
				status, warn = t("authorization_link_expired"), true
				if a.Code != "" {
					status = t("authorization_code_expired")
				}
			}
			if status != "" {
				ui.Row(c.Key("status")).Height(18).Gap(6).Margin(16, 0, 0, 0).Children(func() {
					if spin {
						kit.Busy(c)
					}
					col := p.MutedForeground
					if warn {
						col = p.Warning
					}
					ui.Text(c, status).FontSize(12).TextColor(col)
				})
			}
			callback := a.Callback && a.Flow == FlowBrowser && !a.Busy && !a.Cancelling
			if callback {
				ui.Row(c.Key("callback")).Gap(8).FillWidth().Margin(16, 0, 0, 0).Children(func() {
					ui.TextInput(c.Key("pasted"), &a.Pasted).Label(t("provider_login_callback")).
						Placeholder(t("provider_login_callback")).Grow(1)
					kit.Primary(c.Key("continue"), t("provider_login_continue"), false).
						Children(func() { ui.Text(c, t("provider_login_continue")).FontSize(14) }).OnClick(submit)
				})
			}
			open := func() {
				if a.OpenURL != nil {
					a.OpenURL(a.URL)
				} else {
					notice(c)
				}
			}
			var secondary, primary string
			var onSecondary, onPrimary func()
			switch a.Flow {
			case FlowDevice:
				primary, onPrimary = "authorization_open", open
			case FlowBrowser:
				secondary, onSecondary = "authorization_copy_link", func() {
					c.WriteClipboard(a.URL)
					kit.Toast(c, kit.Done, t("content_copied"))
				}
				primary, onPrimary = "authorization_reopen", open
			case FlowExpired:
				primary, onPrimary = "authorization_refresh", refresh
			case FlowFailed:
				primary, onPrimary = "authorization_retry", refresh
			}
			actions := primary != ""
			if actions {
				ui.Row(c.Key("actions")).Gap(8).FillWidth().Margin(22, 0, 0, 0).Children(func() {
					if secondary != "" {
						kit.Secondary(c.Key("secondary"), t(secondary)).Grow(1).Disabled(a.Cancelling).OnClick(onSecondary)
					}
					if !callback {
						kit.Primary(c.Key("primary"), t(primary), a.Busy || a.Cancelling).Grow(1).
							Children(func() { ui.Text(c, t(primary)).FontSize(14) }).OnClick(onPrimary)
					} else {
						kit.Secondary(c.Key("primary"), t(primary)).Grow(1).Disabled(a.Busy || a.Cancelling).OnClick(onPrimary)
					}
				})
			}
			gap := float32(22)
			if actions {
				gap = 8
			}
			kit.Secondary(c.Key("cancel"), t("authorization_cancel")).FillWidth().Margin(gap, 0, 0, 0).Disabled(a.Cancelling).
				OnClick(func() {
					if a.Cancel != nil {
						a.Cancel()
					} else {
						a.Open = false
					}
				})
		})
	}).Width(min(400, width(c)-48)).Label(a.Title)
	if !open && a.Open && !a.Cancelling {
		if a.Cancel != nil {
			a.Cancel()
		} else {
			a.Open = false
		}
	}
}

// host is a URL's host name, for the code hint.
func host(url string) string {
	_, s, ok := strings.Cut(url, "://")
	if !ok {
		s = url
	}
	s, _, _ = strings.Cut(s, "/")
	return s
}
