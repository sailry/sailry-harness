package settings

import (
	"fmt"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Conversation text sizes in DIPs, a pixel a step.
const (
	MinTextSize = 12
	MaxTextSize = 18
)

// Look is the appearance the Appearance page edits: UI state of the app,
// which a backend would persist as the reference's preferences.
type Look struct {
	Live    bool
	Busy    bool // the update check is running
	Actions *AppearanceActions
	Theme   *string // the theme package's ID; empty is the built-in theme
	Mode    *theme.Mode
	Effects *bool // the theme's animated effects
	// Surfaces are the window materials' opacities the transparency
	// sliders set.
	Surfaces *theme.Surfaces
	// Mark is the new session page's custom lettering; blank falls back to
	// DefaultMark or the default welcome lettering. Nil hides its row.
	Mark        *string
	DefaultMark string
}

// ThemePreviewHeight is a theme card's preview (h_24).
const ThemePreviewHeight = 96

// appearance shows interface sizing, color mode and effects, transparency,
// installed themes with their update and import actions, and the new
// session page's lettering.
func appearance(c *ui.Context, look Look, m *Model) {
	kit.Group(c, "interface", i18n.T(c, "settings_interface"), "", nil, func() {
		kit.Card(c, func() {
			chosen := int(*look.Mode)
			if kit.SegmentedRow(c, "mode", i18n.T(c, "appearance_mode"), "", &chosen,
				i18n.T(c, "appearance_system"), i18n.T(c, "appearance_light"), i18n.T(c, "appearance_dark")) {
				*look.Mode = theme.Mode(chosen)
			}
			levels := make([]string, len(preferences.ViewZoomLevels))
			for i, level := range preferences.ViewZoomLevels {
				levels[i] = strconv.Itoa(int(level)) + "%"
			}
			kit.SegmentedRow(c, "view-size", i18n.T(c, "settings_view_size"), "", &m.ViewZoom, levels...)
			title := i18n.T(c, "settings_message_text_size")
			kit.WideRow(c, "text-size", title, "", func() {
				ui.StepSlider(c.Key("slider"), &m.TextSize, MinTextSize, MaxTextSize, 1).Label(title).Grow(1).Shrink(1).MinWidth(0)
			})
			kit.SwitchRow(c, "effects", i18n.T(c, "appearance_effects"), "", look.Effects)
			welcomeMark(c, look.Mark, look.DefaultMark)
		})
	})
	transparency(c, look.Surfaces)
	kit.Group(c, "packages", i18n.T(c, "appearance_packages"), "", func() {
		// Checking for updates asks the network; importing reads a file.
		kit.SecondaryBusy(c.Key("updates"), i18n.T(c, "plugins_check_updates"), look.Busy).OnClick(func() {
			if look.Actions != nil && look.Actions.CheckUpdates != nil {
				look.Actions.CheckUpdates()
				return
			}
			appearanceNotice(c, look.Live)
		})
		label := i18n.T(c, "appearance_import")
		kit.Primary(c.Key("import"), label, false).Children(func() { ui.Text(c, label).FontSize(14) }).OnClick(func() {
			if look.Actions != nil && look.Actions.Import != nil {
				look.Actions.Import()
				return
			}
			appearanceNotice(c, look.Live)
		})
	}, func() {
		gallery := ui.Grid(c.Key("gallery")).Columns(3).Gap(12).FillWidth()
		d := ui.Local(gallery, themeDetails{}, func() themeDetails { return themeDetails{} })
		d.live = look.Live
		d.actions = look.Actions
		gallery.Children(func() {
			for _, pkg := range theme.Packages() {
				themeCard(c, pkg, look, d)
			}
		})
		details(c, d, look)
		if pkg := d.removing; pkg != nil {
			open := true
			if Confirm(c, "remove-theme", &open, i18n.T(c, "appearance_uninstall"), i18n.T(c, "appearance_remove", "name", pkg.Name), i18n.T(c, "appearance_uninstall")) {
				if d.actions != nil && d.actions.Uninstall != nil {
					d.actions.Uninstall(pkg, func(ok bool) {
						if ok && *look.Theme == pkg.ID {
							apply(theme.Builtin, look)
						}
					})
				}
				open = false
			}
			if !open {
				d.removing = nil
			}
		}
		if look.Actions != nil && look.Actions.Preview != nil {
			imported := look.Actions.Preview
			if imported.Open && imported.Package != nil {
				if imported.details == nil {
					imported.details = &themeDetails{pkg: imported.Package, open: true, importing: imported}
				}
				details(c, imported.details, look)
				imported.Open = imported.details.open
			}
			if !imported.Open {
				look.Actions.Preview = nil
			}
		}
	})
}

func welcomeMark(c *ui.Context, mark *string, fallback string) {
	if mark == nil {
		return
	}
	placeholder := strings.TrimSpace(fallback)
	if placeholder == "" {
		placeholder = i18n.T(c, "welcome_name")
	}
	title := i18n.T(c, "appearance_welcome_mark")
	kit.WideRow(c, "welcome-mark", title, "", func() {
		if ui.TextInput(c.Key("input"), mark).Label(title).Placeholder(placeholder).
			MaxLength(preferences.WelcomeMarkMax).FillWidth().Changed() {
			*mark = preferences.CleanWelcomeMark(*mark)
		}
	})
}

// themeDetails is the theme whose details dialog shows, if any.
type themeDetails struct {
	live      bool
	actions   *AppearanceActions
	removing  *theme.Package
	pkg       *theme.Package
	open      bool
	license   bool // the license text unfolded
	importing *ThemeImport
}

func appearanceNotice(c *ui.Context, live bool) {
	if live {
		kit.Notify(c, "", i18n.T(c, "unavailable"))
	} else {
		notice(c)
	}
}

// apply makes pkg the theme.
func apply(pkg *theme.Package, look Look) {
	*look.Theme = pkg.ID
	if pkg == theme.Builtin {
		*look.Theme = ""
	}
	// Choosing a theme starts from its transparency preset; the sliders
	// adjust it from there.
	if look.Surfaces != nil {
		*look.Surfaces = pkg.Preset()
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
func themeCard(c *ui.Context, pkg *theme.Package, look Look, d *themeDetails) {
	p := theme.Of(c)
	selected := theme.Find(*look.Theme) == pkg
	title := name(c, pkg)
	b := ui.ButtonBase(c.Key(pkg.ID)).Label(title).MinWidth(0).Radius(p.RadiusLg).Border(1, p.Border).Clip().
		Cursor(ui.CursorPointer).AccessSelected(selected)
	if b.Hovered() || b.Pressed() || selected {
		b.Background(p.Active)
	}
	b.Children(func() {
		ui.Column(c).FillWidth().MinWidth(0).Children(func() {
			ThemeMiniature(c, pkg, p.Dark)
			ui.Row(c).FillWidth().Padding(12).Gap(8).Background(p.Surface).Children(func() {
				ui.Text(c, title).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0).TextColor(p.SurfaceForeground)
				if selected {
					ui.Icon(c, icons.CircleCheck).FontSize(16).TextColor(p.Orange).Label(i18n.T(c, "appearance_selected"))
				} else {
					ui.Icon(c, icons.Palette).FontSize(16).TextColor(p.MutedForeground)
				}
			})
		})
	}).OnClick(func() { apply(pkg, look) })
	installed := pkg != theme.Builtin
	if d.live {
		installed = d.actions != nil && d.actions.Installed != nil && d.actions.Installed(pkg.ID)
	}
	b.ContextMenu(func(m *ui.Menu) {
		if m.Item(i18n.T(c, "appearance_select")).Checked(selected).Chosen() {
			apply(pkg, look)
		}
		if m.Item(i18n.T(c, "appearance_details")).Disabled(pkg == theme.Builtin).Chosen() {
			d.pkg, d.open, d.license = pkg, true, false
		}
		if m.Item(i18n.T(c, "appearance_homepage")).Disabled(pkg.Homepage == "").Chosen() {
			c.OpenURL(pkg.Homepage)
		}
		// Exporting writes a file and uninstalling removes the package.
		if m.Item(i18n.T(c, "appearance_export")).Disabled(!installed).Chosen() {
			if d.actions != nil && d.actions.Export != nil {
				d.actions.Export(pkg)
			} else {
				appearanceNotice(c, d.live)
			}
		}
		if m.Item(i18n.T(c, "appearance_uninstall")).Disabled(!installed).Chosen() {
			if d.actions != nil && d.actions.Uninstall != nil {
				d.removing = pkg
			} else {
				appearanceNotice(c, d.live)
			}
		}
	})
}

// ThemeMiniature is a theme in miniature in the current mode: its background and
// window art, a sidebar column with the emphasis dot and three rows, and a
// page with a title, a line and a composer.
func ThemeMiniature(c *ui.Context, pkg *theme.Package, dark bool) {
	t := theme.Preview(pkg, dark)
	art := t.Variant.Asset("background.window")
	if art == nil {
		art = t.Variant.Asset("background.content")
	}
	box := ui.Row(c).FillWidth().Height(ThemePreviewHeight).Background(t.Background).AlignItems(ui.Stretch).PassThrough()
	if art != nil {
		box.Draw(func(pt *ui.Painter, r ui.Rect) { kit.PreviewPicture(pt, art, r) })
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
func details(c *ui.Context, d *themeDetails, look Look) {
	if !d.open || d.pkg == nil {
		return
	}
	p := theme.Of(c)
	pkg := d.pkg
	title := name(c, pkg)
	w := min(width(c)-48, 640)
	kit.Dialog(c.Key("theme-details"), &d.open, func() {
		kit.DialogHeader(c, title)
		ui.Scroll(c.Key("body")).Shrink(1).MinHeight(0).Gap(16).Padding(4).Children(func() {
			cols := 2
			if w < 520 {
				cols = 1
			}
			ui.Grid(c.Key("previews")).Columns(cols).Gap(12).FillWidth().Children(func() {
				for _, dark := range []bool{false, true} {
					label := i18n.T(c, map[bool]string{false: "appearance_light", true: "appearance_dark"}[dark])
					ui.Column(c.Key(dark)).MinWidth(0).Radius(p.RadiusLg).Border(1, p.Border).Clip().Children(func() {
						ThemeMiniature(c, pkg, dark)
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
		kit.DialogFooter(c, func() {
			kit.Secondary(c.Key("close"), i18n.T(c, "close")).OnClick(func() { d.open = false })
			if d.importing != nil {
				kit.SecondaryBusy(c.Key("install"), i18n.T(c, "plugins_install"), d.importing.Pending).OnClick(func() {
					d.importing.Pending = true
					d.importing.Install(func(ok bool) {
						d.importing.Pending = false
						if ok {
							d.importing.Open = false
						}
					})
				})
			} else {
				kit.Secondary(c.Key("apply"), i18n.T(c, "appearance_select")).OnClick(func() { apply(pkg, look) })
			}
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
