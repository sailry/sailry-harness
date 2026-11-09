package settings

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/theme"
)

// The AI group's pages built so far: Providers and Plugins (settings/
// providers, settings/plugins). The rest of the group is phase 5b.

// card is the settings Group's card: a 1 DIP border, radius_lg, rows apart
// by separators.
func card(c *ui.Context, rows func()) {
	p := theme.Of(c)
	ui.Column(c).Border(1, p.Border).Radius(p.RadiusLg).PaddingX(16).Gap(1).Dividers(1, p.Border).
		Background(p.Surface).TextColor(p.SurfaceForeground).Children(rows)
}

func providers(c *ui.Context, s *State, d *preview.Data) {
	p := theme.Of(c)
	ui.Column(c).Gap(12).Children(func() {
		ui.Row(c).MinHeight(32).Gap(8).Children(func() {
			ui.Row(c).AlignItems(ui.Baseline).Gap(6).Grow(1).Children(func() {
				ui.Text(c, i18n.T(c, "provider_channels")).FontSize(14).FontWeight(600)
				ui.Textf(c, "%d", len(d.Providers)).FontSize(14).TextColor(p.MutedForeground)
			})
			ui.TextInput(c.Key("search"), &s.Query).Placeholder(i18n.T(c, "provider_list_search")).
				Label(i18n.T(c, "provider_list_search")).Width(220)
		})
		q := strings.ToLower(strings.TrimSpace(s.Query))
		card(c, func() {
			for i := range d.Providers {
				pr := &d.Providers[i]
				if q != "" && !strings.Contains(strings.ToLower(pr.Name), q) {
					continue
				}
				models := 0
				for _, m := range d.Models {
					if strings.EqualFold(m.Provider, pr.Name) {
						models++
					}
				}
				desc := i18n.T(c, "preview") + " · " + i18n.T(c, "provider_model_count", "count", models)
				row := ui.Row(c.Key(pr.ID)).Padding(12, 0).Gap(12)
				row.Children(func() {
					kit.MediaRow(c, pr.Name, desc, func() { kit.Tile(c, pr.Name, icons.Providers[pr.ID]) }, func() {
						if pr.Badge != "" {
							kit.Tag(c, i18n.T(c, pr.Badge))
						}
					}).Opacity(enabledOpacity(pr.Enabled))
					ui.Switch(c, &pr.Enabled).Label(i18n.T(c, "provider_enabled") + " " + pr.Name)
				})
			}
		})
	})
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
