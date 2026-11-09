package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/theme"
)

// Field is a dialog's labeled control: a 14 DIP label over it, a muted
// hint under it ("" for none).
func Field(c *ui.Context, key any, label, hint string, control func()) {
	p := theme.Of(c)
	ui.Column(c.Key(key)).Gap(6).Children(func() {
		ui.Text(c, label).FontSize(14).FontWeight(500).TextColor(p.PopoverForeground)
		control()
		if hint != "" {
			ui.Text(c, hint).FontSize(12).TextColor(p.MutedForeground)
		}
	})
}

// Dialog is MyGo's Modal placed as Kit places its dialogs: a tenth of the
// window down rather than centered (dialog.rs margin_top), on DialogBase
// with Modal's backdrop, padding, gap, corners and shadow, in the popover
// color. It returns the panel, or nil while closed.
func Dialog(c *ui.Context, open *bool, fn func()) ui.Element {
	p := theme.Of(c)
	w, h := c.Size()
	return ui.DialogBase(c, open, func(back, panel ui.Element) {
		back.Justify(ui.Start).Padding(h/10, 0, 0, 0).Background(p.Backdrop())
		panel.Padding(20).Gap(12).Radius(10).Background(p.Floating()).TextColor(p.PopoverForeground).
			MaxWidth(w-40).MaxHeight(h*0.9-20).Shadow(0, 10, 30, 0, p.Backdrop().Alpha(0.3))
		fn()
	})
}
