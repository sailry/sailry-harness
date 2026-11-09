package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/theme"
)

// Dialog is Kit's dialog: a panel over the dimmed window, its title (16,
// semibold) and optional description, the body, and the footer's buttons
// right-aligned. width 0 is Kit's 420. footer may be nil.
func Dialog(c *ui.Context, open *bool, width float32, title, description string, body, footer func()) ui.Element {
	p := theme.Of(c)
	if width == 0 {
		width = 420
	}
	return ui.DialogBase(c, open, func(backdrop, panel ui.Element) {
		backdrop.Background(ui.RGBA(0, 0, 0, 0.4))
		panel.Width(width).MaxWidthPercent(92).MaxHeightPercent(86).Padding(24).Gap(16).Radius(12).
			Background(p.Popover).Border(1, p.Border).Shadow(0, 16, 48, 0, ui.RGBA(0, 0, 0, 0.24)).Label(title)
		ui.Column(c).Gap(6).Children(func() {
			ui.Text(c, title).FontSize(16).FontWeight(600).TextColor(p.Foreground)
			if description != "" {
				ui.Text(c, description).FontSize(14).TextColor(p.MutedForeground)
			}
		})
		if body != nil {
			ui.Scroll(c).Grow(1).Shrink(1).Gap(12).Children(body)
		}
		if footer != nil {
			ui.Row(c).Gap(8).Justify(ui.End).Children(footer)
		}
	})
}

// Confirm is the destructive confirmation: the message names the target
// and the consequence once; confirm runs the action, after which the
// dialog closes. Cancel closes it.
func Confirm(c *ui.Context, open *bool, title, message, cancel, action string, confirm func()) {
	Dialog(c, open, 380, title, message, nil, func() {
		Secondary(c.Key("cancel"), cancel).OnClick(func() { *open = false })
		Danger(c.Key("confirm"), action).OnClick(func() {
			*open = false
			confirm()
		})
	})
}

// Field is a dialog's labeled control: a 14 DIP label over it, a muted
// hint under it ("" for none).
func Field(c *ui.Context, key any, label, hint string, control func()) {
	p := theme.Of(c)
	ui.Column(c.Key(key)).Gap(6).Children(func() {
		ui.Text(c, label).FontSize(14).FontWeight(500).TextColor(p.Foreground)
		control()
		if hint != "" {
			ui.Text(c, hint).FontSize(12).TextColor(p.MutedForeground)
		}
	})
}
