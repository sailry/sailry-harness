// Package settings is the settings navigation and its pages: Appearance
// (the theme mode), Providers and Plugins (sailry-public settings/mod.rs,
// sidebar/settings.rs, ui/settings_page.rs, settings/providers).
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

// Section is a settings page, named by its i18n key.
type Section string

const (
	Appearance Section = "settings_appearance"
	Providers  Section = "settings_providers"
	Plugins    Section = "settings_plugins"
)

type item struct {
	key  Section
	icon *ui.SVG
}

// groups is the navigation of settings/mod.rs, in its order and icons.
var groups = []struct {
	title string
	items []item
}{
	{"settings_group_app", []item{
		{"settings_general", icons.Settings}, {Appearance, icons.Palette}, {"settings_conversation", icons.Chat},
		{"settings_shortcuts", icons.ALargeSmall}, {"settings_dictation", icons.Microphone},
	}},
	{"settings_group_ai", []item{
		{Providers, icons.Cpu}, {"settings_connectors", icons.Plug}, {"settings_teams", icons.Users},
		{Plugins, icons.Gallery}, {"settings_skills", icons.Bot}, {"settings_knowledge", icons.BookOpen},
	}},
	{"settings_group_tools", []item{
		{"settings_terminal", icons.Terminal}, {"settings_connections", icons.Network},
	}},
	{"settings_group_system", []item{
		{"settings_permissions", icons.Settings2}, {"settings_backup", icons.Upload}, {"settings_about", icons.Info},
	}},
}

// State is the settings' part of the app state.
type State struct {
	Section Section
	Query   string // the providers' search
}

func New() *State { return &State{Section: Providers} }

// Nav builds the settings navigation in place of the sessions.
func Nav(c *ui.Context, s *State) {
	p := theme.Of(c)
	ui.Scroll(c.Key("settings-nav")).Grow(1).Padding(8, 12).Children(func() {
		for _, g := range groups {
			ui.Column(c.Key(g.title)).Gap(2).Margin(0, 0, 12, 0).Children(func() {
				ui.Text(c, i18n.T(c, g.title)).FontSize(14).Height(32).PaddingX(8).AlignSelf(ui.Stretch).
					TextColor(p.MutedForeground.Alpha(0.7)).SingleLine()
				for _, it := range g.items {
					selected := it.key == s.Section
					label := i18n.T(c, string(it.key))
					b := ui.ButtonBase(c.Key(string(it.key))).Label(label).Height(28).PaddingX(8).Gap(8).Radius(6).Justify(ui.Start).TextColor(p.Foreground)
					b.Background(p.SidebarItem(selected, b.Hovered())).AccessSelected(selected)
					b.Children(func() {
						ui.Icon(c, it.icon).FontSize(16)
						ui.Text(c, label).FontSize(14).SingleLine()
					}).OnClick(func() { s.Section = it.key })
				}
			})
		}
	})
}

// Title is the header's title for the section.
func Title(c *ui.Context, s *State) string { return i18n.T(c, string(s.Section)) }

// Page builds the section's page: a 800 DIP column with its title and
// description, as SettingsPage lays it out.
func Page(c *ui.Context, s *State, d *preview.Data, mode *theme.Mode) {
	p := theme.Of(c)
	ui.Scroll(c.Key(string(s.Section))).Fill().Children(func() {
		ui.Column(c).FillWidth().MaxWidth(800).Margin(0, ui.Auto).Padding(24).Gap(24).Children(func() {
			ui.Column(c).Gap(8).Children(func() {
				ui.Text(c, i18n.T(c, string(s.Section))).FontSize(18).FontWeight(600)
				if desc := string(s.Section) + "_description"; i18n.In("en", desc) != desc {
					ui.Text(c, i18n.T(c, desc)).FontSize(14).TextColor(p.MutedForeground)
				}
			})
			switch s.Section {
			case Appearance:
				appearance(c, mode)
			case Providers:
				providers(c, s, d)
			case Plugins:
				plugins(c, d)
			}
		})
	})
}

// appearance is the theme mode, System, Light or Dark.
func appearance(c *ui.Context, mode *theme.Mode) {
	card(c, func() {
		ui.Row(c).Padding(12, 0).Gap(16).Children(func() {
			ui.Text(c, i18n.T(c, "appearance_mode")).FontSize(14).Grow(1)
			chosen := int(*mode)
			ui.Segmented(c.Key("mode"), &chosen, i18n.T(c, "appearance_system"), i18n.T(c, "appearance_light"), i18n.T(c, "appearance_dark")).
				Label(i18n.T(c, "appearance_mode"))
			*mode = theme.Mode(chosen)
		})
	})
}

// card is the settings Group's card: a 1 DIP border, radius_lg, rows apart
// by separators.
func card(c *ui.Context, rows func()) {
	p := theme.Of(c)
	ui.Column(c).Border(1, p.Border).Radius(8).PaddingX(16).Gap(1).Dividers(1, p.Border).Children(rows)
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
