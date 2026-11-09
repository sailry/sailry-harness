package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/theme"
)

// The extension details page (settings/navigation.rs OpenDetails,
// market/details.rs panel, plugins/details/*): a plugin's, skill's or
// theme's details in the window's right column, over the settings or the
// market. Close, opening another section or another details closes it
// (CloseDetails, clear_extension_details).

// DetailKind is what a details page shows.
type DetailKind int

const (
	DetailPlugin DetailKind = iota
	DetailSkill
	DetailTheme
)

// Detail is the details page shown: its kind, the item's ID and the tab
// (0 overview, 1 contents, 2 permissions, 3 versions).
type Detail struct {
	Kind DetailKind
	ID   string
	Tab  int
}

// ShowDetails opens an item's details, replacing those shown.
func (s *State) ShowDetails(kind DetailKind, id string) { s.Details = &Detail{Kind: kind, ID: id} }

// CloseDetails closes the details page (CloseDetails).
func (s *State) CloseDetails() { s.Details = nil }

// DetailsPanel draws the details page in the right column: the header
// with the title and Close, then the item's summary and content
// (market/details.rs panel).
func DetailsPanel(c *ui.Context, s *State) {
	d := s.Details
	if d == nil {
		return
	}
	p := theme.Of(c)
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	title := map[DetailKind]string{DetailPlugin: "market_plugin_details", DetailSkill: "market_skill_details", DetailTheme: "market_theme_details"}[d.Kind]
	ui.Column(c.Key("extension-details")).Fill().MinWidth(0).Children(func() {
		ui.Row(c.Key("header")).Height(44).PaddingX(12).Gap(8).Shrink(0).Children(func() {
			ui.Text(c, t(title)).FontSize(14).Grow(1).SingleLine()
			kit.Action(c.Key("close"), icons.Close, t("close"), false).OnClick(s.CloseDetails)
		})
		ui.Scroll(c.Key(d.ID)).Grow(1).Shrink(1).Children(func() {
			ui.Column(c).Padding(20, 28, 28, 28).Gap(20).MinWidth(0).Children(func() {
				switch d.Kind {
				case DetailPlugin:
					if pk := s.pkg(d.ID); pk != nil {
						pluginDetails(c, s, pk, d)
					}
				case DetailSkill:
					if sk := s.skill(d.ID); sk != nil {
						summary(c, sk.Name, sk.Description, func() { kit.Tile(c, sk.Name, icons.Bot) }, nil)
						facts(c, [][2]string{{t("market_details_source"), skillSource(c, sk)}})
					}
				case DetailTheme:
					if th := s.theme(d.ID); th != nil {
						summary(c, th.Name, th.Description, func() { kit.Tile(c, th.Name, icons.Palette) }, func() { themeAction(c, s.Model, th) })
						modes := []string{}
						if th.Light {
							modes = append(modes, t("market_theme_light"))
						}
						if th.Dark {
							modes = append(modes, t("market_theme_dark"))
						}
						facts(c, [][2]string{{t("market_details_source"), t("market_local")}, {t("market_details_formats"), join(modes)}})
					}
				}
			})
		})
	}).TextColor(p.Foreground)
}

func join(xs []string) string {
	out := ""
	for i, x := range xs {
		if i > 0 {
			out += " · "
		}
		out += x
	}
	return out
}

// summary is market/details.rs summary: a 56 DIP tile, the title, the
// primary action, and the description under them.
func summary(c *ui.Context, title, description string, tile, action func()) {
	p := theme.Of(c)
	ui.Column(c.Key("summary")).Gap(12).Children(func() {
		ui.Row(c).Gap(12).MinWidth(0).Children(func() {
			ui.Row(c).Size(56, 56).Shrink(0).Center().Children(tile)
			ui.Text(c, title).FontSize(18).FontWeight(600).SingleLine().Grow(1).Shrink(1)
			if action != nil {
				ui.Row(c).Shrink(0).Children(action)
			}
		})
		if description != "" {
			ui.Text(c, description).FontSize(14).TextColor(p.MutedForeground)
		}
	})
}

// facts are details rows of a label and a value in a bordered box.
func facts(c *ui.Context, rows [][2]string) {
	p := theme.Of(c)
	ui.Column(c.Key("facts")).Border(1, p.Border).Radius(p.RadiusLg).Dividers(1, p.Border).Children(func() {
		for i, r := range rows {
			ui.Row(c.Key(i)).Gap(12).Padding(10, 12).Children(func() {
				ui.Text(c, r[0]).FontSize(14).Width(96).Shrink(0).TextColor(p.MutedForeground)
				ui.Text(c, r[1]).FontSize(14).Grow(1).Shrink(1).Selectable()
			})
		}
	})
}

// pluginDetails is plugins/details/view.rs: the summary with Install or
// Update, what the plugin needs, its tabs and their content.
func pluginDetails(c *ui.Context, s *State, pk *preview.Package, d *Detail) {
	p := theme.Of(c)
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	var action func()
	switch {
	case !pk.Installed && pk.Requires == "":
		action = func() {
			kit.Primary(c.Key("primary"), t("market_install"), false).Height(28).Children(func() { ui.Text(c, t("market_install")).FontSize(14) }).OnClick(func() { notice(c) })
		}
	case Update(pk) && pk.Requires == "":
		action = func() {
			kit.Primary(c.Key("primary"), t("market_update"), false).Height(28).Children(func() { ui.Text(c, t("market_update")).FontSize(14) }).OnClick(func() { notice(c) })
		}
	}
	summary(c, pk.Name, pk.Description, func() { kit.Tile(c, pk.Name, icons.Puzzle) }, action)
	if msg := compatibility(c, pk); msg != "" {
		ui.Row(c.Key("required")).Gap(8).Children(func() {
			ui.Text(c, msg).FontSize(14).Grow(1).TextColor(p.MutedForeground)
			kit.Secondary(c.Key("sailry"), t("plugins_update_sailry")).Height(28).OnClick(func() { notice(c) })
		})
	}
	tabs := []string{t("market_details_overview"), t("market_details_contents"), t("market_details_permissions"), t("market_details_versions")}
	ui.Tabs(c.Key("tabs"), &d.Tab, tabs...).Label(t("market_plugin_details"))
	switch d.Tab {
	case 0:
		installed := t("market_details_latest")
		if pk.Installed {
			installed = pk.Version
		}
		facts(c, [][2]string{{t("market_details_version"), installed}, {t("market_details_source"), t(pk.Source)}, {t("market_details_host"), t("composer_host_local")}})
		includes := [][3]any{{"market_details_tools", icons.Settings2, len(pk.Tools)}, {"market_details_skills", icons.BookOpen, len(pk.Skills)}, {"market_details_mcp", icons.Network, len(pk.MCP)}}
		ui.Column(c.Key("includes")).Gap(8).Children(func() {
			ui.Text(c, t("market_details_includes")).FontSize(14).FontWeight(500)
			for _, x := range includes {
				if x[2].(int) == 0 {
					continue
				}
				b := kit.GhostLook(c, ui.ButtonBase(c.Key(x[0])).Label(t(x[0].(string))), false).Justify(ui.Start).Gap(12).Padding(8, 12).
					Border(1, p.Border).Radius(p.RadiusLg).OnClick(func() { d.Tab = 1 })
				b.Children(func() {
					ui.Icon(c, x[1].(*ui.SVG)).FontSize(16)
					ui.Text(c, t(x[0].(string))).FontSize(14).Grow(1)
					ui.Textf(c, "%d", x[2].(int)).FontSize(14).TextColor(p.MutedForeground)
					ui.Icon(c, icons.ChevronRight).FontSize(16).TextColor(p.MutedForeground)
				})
			}
		})
	case 1:
		section := func(key string, items []string, empty string) {
			ui.Column(c.Key(key)).Gap(6).Children(func() {
				ui.Text(c, t(key)).FontSize(14).FontWeight(500)
				if len(items) == 0 {
					ui.Text(c, t(empty)).FontSize(14).TextColor(p.MutedForeground)
				}
				for _, x := range items {
					ui.Text(c.Key(x), x).FontSize(14).Font("monospace").Padding(6, 12).Border(1, p.Border).Radius(p.Radius)
				}
			})
		}
		section("market_details_tools", pk.Tools, "connector_tools_empty")
		section("market_details_skills", pk.Skills, "plugins_no_skills")
		section("market_details_mcp", pk.MCP, "plugins_no_mcp")
	case 2:
		if len(pk.Permissions) == 0 {
			ui.Text(c, t("market_details_permission_none")).FontSize(14).TextColor(p.MutedForeground)
		}
		for _, k := range pk.Permissions {
			ui.Row(c.Key(k)).Gap(8).Children(func() {
				ui.Icon(c, icons.Shield).FontSize(16).TextColor(p.MutedForeground)
				ui.Text(c, t(k)).FontSize(14)
			})
		}
	case 3:
		ui.Column(c.Key("versions")).Border(1, p.Border).Radius(p.RadiusLg).Dividers(1, p.Border).Children(func() {
			for i, v := range pk.Versions {
				ui.Column(c.Key(v)).Children(func() {
					ui.Row(c).Gap(8).Padding(10, 12).Children(func() {
						ui.Text(c, v).FontSize(14).Font("monospace").Grow(1)
						if i == 0 {
							kit.Badge(c, t("market_details_latest"), kit.Info)
						}
						if pk.Installed && v == pk.Version {
							kit.Badge(c, t("market_installed"), kit.Positive)
						}
					})
					if i == 0 {
						releaseNotes(c, pk.Notes)
					}
				})
			}
		})
	}
}
