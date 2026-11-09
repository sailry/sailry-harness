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
