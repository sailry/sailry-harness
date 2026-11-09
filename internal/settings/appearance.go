package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// Look is the appearance the Appearance page edits: UI state of the app,
// which a backend would persist as the reference's preferences.
type Look struct {
	Theme   *string // the theme package's ID; empty is the built-in theme
	Mode    *theme.Mode
	Effects *bool // the theme's animated effects
	// Surfaces are the window materials' opacities the transparency
	// sliders set.
	Surfaces *theme.Surfaces
}

// previewHeight is a theme card's preview (h_24).
const previewHeight = 96

// appearance is settings/appearance.rs: the interface group (mode and
// effects), the transparency sliders, the gallery of theme packages with
// its update check and import, and the selected theme's details.
func appearance(c *ui.Context, look Look) {
	kit.Group(c, "interface", i18n.T(c, "settings_interface"), "", nil, func() {
		kit.Card(c, func() {
			chosen := int(*look.Mode)
			if kit.SegmentedRow(c, "mode", i18n.T(c, "appearance_mode"), "", &chosen,
				i18n.T(c, "appearance_system"), i18n.T(c, "appearance_light"), i18n.T(c, "appearance_dark")) {
				*look.Mode = theme.Mode(chosen)
			}
			kit.SwitchRow(c, "effects", i18n.T(c, "appearance_effects"), "", look.Effects)
		})
	})
	transparency(c, look.Surfaces)
	kit.Group(c, "packages", i18n.T(c, "appearance_packages"), "", func() {
		// Checking for updates asks the network; importing reads a file.
		kit.Secondary(c.Key("updates"), i18n.T(c, "plugins_check_updates")).OnClick(func() { notice(c) })
		label := i18n.T(c, "appearance_import")
		kit.Primary(c.Key("import"), label, false).Children(func() { ui.Text(c, label).FontSize(14) }).OnClick(func() { notice(c) })
	}, func() {
		ui.Grid(c.Key("gallery")).Columns(3).Gap(12).FillWidth().Children(func() {
			for _, pkg := range theme.Packages {
				themeCard(c, pkg, look.Theme)
			}
		})
	})
	selected := theme.Find(*look.Theme)
	kit.Group(c, "details", i18n.T(c, "appearance_details"), "", nil, func() { details(c, selected) })
}

// name is a package's name; the built-in theme's is translated.
func name(c *ui.Context, pkg *theme.Package) string {
	if pkg == theme.Builtin {
		return i18n.T(c, "settings_theme_builtin")
	}
	return pkg.Name
}

// themeCard is a gallery card (appearance/gallery.rs): a ghost button with
// a 1 DIP border holding the theme's preview over a label row naming it,
// marked when selected. Clicking it applies the theme.
func themeCard(c *ui.Context, pkg *theme.Package, chosen *string) {
	p := theme.Of(c)
	selected := theme.Find(*chosen) == pkg
	title := name(c, pkg)
	b := ui.ButtonBase(c.Key(pkg.ID)).Label(title).MinWidth(0).Radius(p.RadiusLg).Border(1, p.Border).Clip().
		Cursor(ui.CursorPointer).AccessSelected(selected)
	if b.Hovered() || b.Pressed() || selected {
		b.Background(p.Active)
	}
	b.Children(func() {
		ui.Column(c).FillWidth().MinWidth(0).Children(func() {
			miniature(c, pkg, p.Dark)
			ui.Row(c).FillWidth().Padding(12).Gap(8).Background(p.Surface).Children(func() {
				ui.Text(c, title).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0).TextColor(p.SurfaceForeground)
				if selected {
					ui.Icon(c, icons.CircleCheck).FontSize(16).TextColor(p.Orange).Label(i18n.T(c, "appearance_selected"))
				} else {
					ui.Icon(c, icons.Palette).FontSize(16).TextColor(p.MutedForeground)
				}
			})
		})
	}).OnClick(func() {
		*chosen = pkg.ID
		if pkg == theme.Builtin {
			*chosen = ""
		}
	})
}

// miniature is a theme in miniature in the current mode (theme/preview.rs):
// its background and window art, a sidebar column with the emphasis dot
// and three rows, and a page with a title, a line and a composer.
func miniature(c *ui.Context, pkg *theme.Package, dark bool) {
	t := theme.Look(pkg.ID, dark)
	art := t.Variant.Asset("background.window")
	if art == nil {
		art = t.Variant.Asset("background.content")
	}
	box := ui.Row(c).FillWidth().Height(previewHeight).Background(t.Background).AlignItems(ui.Stretch).PassThrough()
	if art != nil {
		box.Draw(func(pt *ui.Painter, r ui.Rect) { kit.Picture(pt, art, r) })
	}
	box.Children(func() {
		ui.Column(c).Width(48).Padding(8).Gap(8).Background(t.Sidebar).Children(func() {
			ui.Box(c).Size(12, 12).Radius(6).Background(t.Primary)
			for range 3 {
				ui.Box(c).FillWidth().Height(4).Background(t.Foreground.Alpha(0.12))
			}
		})
		ui.Column(c).Grow(1).MinWidth(0).Padding(12).Gap(8).Children(func() {
			ui.Box(c).Width(64).MaxWidthPercent(100).Height(8).Radius(4).Background(t.Foreground.Alpha(0.3))
			ui.Box(c).FillWidth().Height(4).Background(t.Foreground.Alpha(0.1))
			ui.Spacer(c)
			ui.Row(c).Height(24).Padding(4).Radius(t.Radius).Background(t.Surface).Children(func() {
				ui.Spacer(c)
				ui.Box(c).Size(16, 16).Radius(8).Background(t.Primary)
			})
		})
	})
}

// details lists the selected theme's information: its name and
// description, version, author and license.
func details(c *ui.Context, pkg *theme.Package) {
	p := theme.Of(c)
	kit.Card(c, func() {
		kit.Row(c, "name", name(c, pkg), pkg.Description, nil)
		info := func(key, label, value string) {
			if value == "" {
				return
			}
			kit.Row(c, key, label, "", func() {
				ui.Text(c, value).FontSize(14).SingleLine().Selectable().TextColor(p.MutedForeground)
			})
		}
		info("version", i18n.T(c, "settings_version"), pkg.Release)
		info("author", i18n.T(c, "appearance_author"), pkg.License.Author)
		info("license", i18n.T(c, "settings_license"), pkg.License.Name)
	})
}

// transparency is appearance/opacity.rs: the container's and the window's
// opacity as sliders of transparency, 0 opaque to 100 at 45% opacity,
// their value after them.
func transparency(c *ui.Context, s *theme.Surfaces) {
	p := theme.Of(c)
	kit.Group(c, "transparency", i18n.T(c, "appearance_transparency"), "", nil, func() {
		kit.Card(c, func() {
			for _, row := range []struct {
				key     string
				opacity *float32
			}{{"appearance_main_transparency", &s.Main}, {"appearance_sidebar_transparency", &s.Sidebar}} {
				title := i18n.T(c, row.key)
				kit.Row(c, row.key, title, "", func() {
					v := float64(1-*row.opacity) * 100 / 0.55
					ui.Row(c).Width(240).Gap(12).Children(func() {
						if ui.Slider(c.Key("slider"), &v, 0, 100).Label(title).Grow(1).Changed() {
							*row.opacity = float32(1 - v*0.55/100)
						}
						ui.Textf(c, "%d%%", int(v+0.5)).FontSize(14).Width(40).TextAlign(ui.End).TextColor(p.SurfaceForeground)
					})
				})
			}
		})
	})
}
