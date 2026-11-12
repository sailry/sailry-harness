package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// ProjectColors are the colors a project's appearance names, in the
// editor's order.
var ProjectColors = []string{"none", "blue", "indigo", "violet", "magenta", "red", "orange", "amber", "green", "teal", "cyan"}

// ProjectColor is the color an appearance names: none, or no name, is the muted text,
// the others their palette at 400 in dark and 600 in light, blue for a name
// it does not know.
func ProjectColor(c *ui.Context, name string) ui.Color {
	p := theme.Of(c)
	if name == "none" || name == "" {
		return p.MutedForeground
	}
	if name == "magenta" {
		name = "fuchsia"
	}
	scale := uint64(600)
	if p.Dark {
		scale = 400
	}
	if color, ok := theme.Named(name, scale); ok {
		return color
	}
	color, _ := theme.Named("blue", scale)
	return color
}

// ProjectGlyph is a project's icon alone, in its color.
func ProjectGlyph(c *ui.Context, icon, color string, size float32) ui.Element {
	return ui.Icon(c, icons.Project(icon)).FontSize(size).Shrink(0).TextColor(ProjectColor(c, color))
}

// appearanceOpen keys whether an AppearancePicker's popover shows.
type appearanceOpen struct{}

// AppearancePicker is the project appearance control: a button showing the
// glyph in its color, opening a popover with the colors as round swatches
// over the icons, six a row, drawn in the chosen color.
func AppearancePicker(c *ui.Context, icon, color *string, disabled bool) ui.Element {
	p := theme.Of(c)
	label := i18n.T(c, "project_appearance")
	b := GhostLook(c, ui.ButtonBase(c).Label(label), false).Size(28, 28).Disabled(disabled)
	open := ui.Local(b, appearanceOpen{}, func() bool { return false })
	Tooltip(c, b, label)
	b.HasPopup(ui.PopupDialog).Expanded(*open).Children(func() { ProjectGlyph(c, *icon, *color, 14) }).
		OnClick(func() { *open = !*open })
	if disabled {
		*open = false
	}
	Popover(c, b, open, func() {
		ui.Column(c.Key("appearance")).Width(248).Padding(4).Gap(8).Children(func() {
			// Ten swatches with room between them. Indigo, between blue and
			// violet, is no longer offered; a project that has it keeps it
			// and sees it here.
			ui.Row(c.Key("colors")).Height(28).PaddingX(4).Gap(6).Justify(ui.SpaceBetween).AlignItems(ui.Center).Children(func() {
				for _, name := range ProjectColors {
					if name == "indigo" && *color != name {
						continue
					}
					tint := ProjectColor(c, name)
					swatch := ui.ButtonBase(c.Key(name)).Label(i18n.T(c, "project_color_"+name)).Size(18, 18).Radius(9).
						Background(tint).Center().Selected(*color == name)
					if *color == name {
						swatch.Border(1, p.Foreground).Children(func() { ui.Icon(c, icons.Check).FontSize(10).TextColor(p.Background) })
					}
					swatch.OnClick(func() { *color = name })
				}
			})
			tint := ProjectColor(c, *color)
			for row := 0; row < len(icons.Projects); row += 6 {
				ui.Row(c.Key(row)).Gap(4).Children(func() {
					for _, name := range icons.Projects[row:min(row+6, len(icons.Projects))] {
						Ghost(c.Key(name), i18n.T(c, "project_icon_"+name), *icon == name).Grow(1).Basis(0).Height(32).
							Children(func() { ui.Icon(c, icons.Project(name)).FontSize(16).TextColor(tint) }).
							OnClick(func() { *icon = name })
					}
				})
			}
		})
	})
	return b
}
