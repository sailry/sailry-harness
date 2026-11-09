package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/theme"
)

// The AI group's Plugins page (settings/
// providers, settings/plugins). The rest of the group is phase 5b.

// card is the settings Group's card: a 1 DIP border, radius_lg, rows apart
// by separators.
func card(c *ui.Context, rows func()) {
	p := theme.Of(c)
	ui.Column(c).Border(1, p.Border).Radius(p.RadiusLg).PaddingX(16).Gap(1).Dividers(1, p.Border).
		Background(p.Surface).TextColor(p.SurfaceForeground).Children(rows)
}

func plugins(c *ui.Context, d *preview.Data) {
	p := theme.Of(c)
	ui.Column(c).Gap(12).Children(func() {
		ui.Text(c, i18n.T(c, "plugins_installed")).FontSize(14).FontWeight(600)
		card(c, func() {
			for i := range d.Plugins {
				pl := &d.Plugins[i]
				ui.Row(c.Key(pl.ID)).Padding(12, 0).Gap(12).Children(func() {
					kit.MediaRow(c, pl.Name, pl.Description, func() { kit.Tile(c, pl.Name, icons.Puzzle) }, func() {
						key := "plugins_third_party"
						if pl.Official {
							key = "plugins_official"
						}
						kit.Tag(c, i18n.T(c, key))
					})
					ui.Text(c, pl.Version).FontSize(14).TextColor(p.MutedForeground).Font("monospace")
					ui.Switch(c, &pl.Enabled).Label(i18n.T(c, "plugins_enabled") + " " + pl.Name)
				})
			}
		})
	})
}

// enabledOpacity fades disabled rows as the providers list does.
func enabledOpacity(on bool) float32 {
	if on {
		return 1
	}
	return 0.45
}
