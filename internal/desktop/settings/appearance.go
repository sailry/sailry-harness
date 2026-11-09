package settings

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Look is the appearance the Appearance page edits: UI state of the app,
// which a backend would persist as the reference's preferences.
type Look struct {
	Live    bool
	Theme   *string // the theme package's ID; empty is the built-in theme
	Mode    *theme.Mode
	Effects *bool // the theme's animated effects
	// Surfaces are the window materials' opacities the transparency
	// sliders set.
	Surfaces *theme.Surfaces
}

// previewHeight is a theme card's preview (h_24).
const previewHeight = 96

// appearance is the interface group (mode and effects), the transparency
// sliders and the gallery of theme packages with its update check and import.
// A card's right-click menu opens a theme's details in a dialog.
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
		kit.Secondary(c.Key("updates"), i18n.T(c, "plugins_check_updates")).OnClick(func() { appearanceNotice(c, look.Live) })
		label := i18n.T(c, "appearance_import")
		kit.Primary(c.Key("import"), label, false).Children(func() { ui.Text(c, label).FontSize(14) }).OnClick(func() { appearanceNotice(c, look.Live) })
	}, func() {
		gallery := ui.Grid(c.Key("gallery")).Columns(3).Gap(12).FillWidth()
		d := ui.Local(gallery, themeDetails{}, func() themeDetails { return themeDetails{} })
		d.live = look.Live
		gallery.Children(func() {
			for _, pkg := range theme.Packages {
				themeCard(c, pkg, look.Theme, d)
			}
		})
		details(c, d, look.Theme)
	})
}

// themeDetails is the theme whose details dialog shows, if any.
type themeDetails struct {
	live    bool
	pkg     *theme.Package
	open    bool
	license bool // the license text unfolded
}

func appearanceNotice(c *ui.Context, live bool) {
	if live {
		kit.Notify(c, "", i18n.T(c, "unavailable"))
	} else {
		notice(c)
	}
}

// apply makes pkg the theme.
func apply(pkg *theme.Package, chosen *string) {
	*chosen = pkg.ID
	if pkg == theme.Builtin {
		*chosen = ""
	}
}

// name is a package's name; the built-in theme's is translated.
func name(c *ui.Context, pkg *theme.Package) string {
	if pkg == theme.Builtin {
		return i18n.T(c, "settings_theme_builtin")
	}
	return pkg.Name
}

// themeCard is a gallery card: a ghost button with a 1 DIP border holding the
// theme's preview over a label row naming it, marked when selected. Clicking
// it applies the theme; its right-click menu is the system's: Apply, Details,
// Homepage, Export and Uninstall, the last two for installed packages (the
// built-in theme is not one).
func themeCard(c *ui.Context, pkg *theme.Package, chosen *string, d *themeDetails) {
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
	}).OnClick(func() { apply(pkg, chosen) })
	installed := pkg != theme.Builtin
	b.ContextMenu(func(m *ui.Menu) {
		if m.Item(i18n.T(c, "appearance_select")).Checked(selected).Chosen() {
			apply(pkg, chosen)
		}
		if m.Item(i18n.T(c, "appearance_details")).Disabled(!installed).Chosen() {
			*d = themeDetails{pkg: pkg, open: true}
		}
		if m.Item(i18n.T(c, "appearance_homepage")).Disabled(pkg.Homepage == "").Chosen() {
			c.OpenURL(pkg.Homepage)
		}
		// Exporting writes a file and uninstalling removes the package.
		if m.Item(i18n.T(c, "appearance_export")).Disabled(!installed).Chosen() {
			appearanceNotice(c, d.live)
		}
		if m.Item(i18n.T(c, "appearance_uninstall")).Disabled(!installed).Chosen() {
			appearanceNotice(c, d.live)
		}
	})
}

// miniature is a theme in miniature in the current mode: its background and
// window art, a sidebar column with the emphasis dot and three rows, and a
// page with a title, a line and a composer.
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

// details is a theme's details dialog: up to 640 wide, the name as its title,
// the theme in miniature light and dark side by side (stacked under 520), the
// description, the author, version and identifier, and the license folding
// its text, over Close and Apply.
func details(c *ui.Context, d *themeDetails, chosen *string) {
	if !d.open || d.pkg == nil {
		return
	}
	p := theme.Of(c)
	pkg := d.pkg
	title := name(c, pkg)
	w := min(width(c)-48, 640)
	kit.Dialog(c.Key("theme-details"), &d.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600).SingleLine().PaddingEnd(24)
		ui.Scroll(c.Key("body")).Shrink(1).MinHeight(0).Gap(16).Padding(4).Children(func() {
			cols := 2
			if w < 520 {
				cols = 1
			}
			ui.Grid(c.Key("previews")).Columns(cols).Gap(12).FillWidth().Children(func() {
				for _, dark := range []bool{false, true} {
					label := i18n.T(c, map[bool]string{false: "appearance_light", true: "appearance_dark"}[dark])
					ui.Column(c.Key(dark)).MinWidth(0).Radius(p.RadiusLg).Border(1, p.Border).Clip().Children(func() {
						miniature(c, pkg, dark)
						ui.Text(c, label).FontSize(14).Padding(8, 12).TextColor(p.MutedForeground)
					})
				}
			})
			if pkg.Description != "" {
				ui.Text(c, pkg.Description).FontSize(14).TextColor(p.MutedForeground)
			}
			kit.Group(c, "information", i18n.T(c, "appearance_information"), "", nil, func() {
				kit.Card(c, func() {
					info := func(key, label, value string) {
						kit.Row(c, key, label, "", func() {
							ui.Text(c, value).FontSize(14).Selectable().TextAlign(ui.End).MaxWidth((w - 88) * 0.55).
								TextColor(p.MutedForeground)
						})
					}
					info("author", i18n.T(c, "appearance_author"), pkg.License.Author)
					if pkg.Release != "" {
						info("version", i18n.T(c, "settings_version"), pkg.Release)
					} else {
						info("version", i18n.T(c, "appearance_format_version"), fmt.Sprint(pkg.Version))
					}
					info("id", i18n.T(c, "appearance_identifier"), pkg.ID)
				})
			})
			kit.Card(c, func() {
				parts := ui.CollapsibleBase(c, &d.license)
				parts.Trigger.FillWidth().MinHeight(48).Gap(12).Cursor(ui.CursorPointer).Children(func() {
					ui.Text(c, i18n.T(c, "settings_license")).FontSize(14).TextColor(p.SurfaceForeground)
					ui.Spacer(c)
					if pkg.License.Name != "" {
						ui.Text(c, pkg.License.Name).FontSize(14).TextColor(p.MutedForeground)
					}
					ui.Icon(c, icons.ChevronDown).FontSize(14).TextColor(p.MutedForeground).Rotate(-90 * (1 - parts.Progress()))
				})
				parts.Panel(func() {
					ui.Text(c, pkg.License.Text).FontSize(12).Selectable().Padding(0, 0, 16, 0).TextColor(p.MutedForeground)
				})
			})
		})
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("close"), i18n.T(c, "close")).OnClick(func() { d.open = false })
			kit.Secondary(c.Key("apply"), i18n.T(c, "appearance_select")).OnClick(func() { apply(pkg, chosen) })
		})
	}).Width(w).Label(title)
}

// transparency is the container's and the window's opacity as sliders of
// transparency, 0 opaque to 100 at 45% opacity, their value after them.
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
			// How much the columns blur a theme's window art, 0 to 100.
			title := i18n.T(c, "appearance_blur")
			kit.Row(c, "appearance_blur", title, "", func() {
				v := float64(s.Blur) * 100
				ui.Row(c).Width(240).Gap(12).Children(func() {
					if ui.Slider(c.Key("slider"), &v, 0, 100).Label(title).Grow(1).Changed() {
						s.Blur = float32(v / 100)
					}
					ui.Textf(c, "%d%%", int(v+0.5)).FontSize(14).Width(40).TextAlign(ui.End).TextColor(p.SurfaceForeground)
				})
			})
		})
	})
}
