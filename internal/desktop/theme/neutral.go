package theme

import "github.com/egoist/mygo/ui"

// neutral is the built-in default theme's Kit colors, by the keys of
// theme.json's theme.colors. A package overlays them key by key, so what it
// leaves out keeps these values.
func neutral(dark bool) map[string]string {
	base, fg, mfg := "#ffffff", "#000000e0", "#00000099"
	surface, inset, hover, active, border := "#00000006", "#00000006", "#00000010", "#00000008", "#00000010"
	box, button := "#00000000", "#00000000" // light group boxes and buttons are clear
	m := map[string]string{
		"sidebar.background":               "#fafafa",
		"sidebar.accent.background":        active,
		"border":                           border,
		"sidebar.border":                   "#00000014",
		"popover.background":               base,
		"primary.background":               "#2563eb", // blue-600
		"primary.hover.background":         "#3b82f6", // blue-500
		"primary.active.background":        "#1d4ed8", // blue-700
		"button.primary.hover.background":  "#000000f0",
		"button.primary.active.background": "#000000ff",
		"list.active.background":           active,
		"list.active.border":               "#00000040",
		"selection.background":             "#00000018",
		"tab.active.background":            "#ffffff",
		"tab.active.foreground":            "#181818",
		"tab_bar.segmented.background":     "#00000008",
		"switch.background":                "#0000001f",
	}
	if dark {
		base, fg, mfg = "#181818", "#ffffffd9", "#ffffff99"
		surface, inset, hover, active, border = "#ffffff0a", "#ffffff12", "#ffffff0f", "#ffffff1f", "#ffffff10"
		box, button = surface, surface
		m = map[string]string{
			"sidebar.background":               "#1d1d1d",
			"sidebar.accent.background":        "#ffffff0f",
			"border":                           "#ffffff0c",
			"sidebar.border":                   "#ffffff14",
			"popover.background":               "#242424",
			"primary.background":               "#60a5fa", // blue-400
			"primary.hover.background":         "#93c5fd", // blue-300
			"primary.active.background":        "#3b82f6", // blue-500
			"button.primary.hover.background":  "#fffffff0",
			"button.primary.active.background": "#ffffffff",
			"list.active.background":           "#ffffff33",
			"list.active.border":               mfg,
			"selection.background":             "#ffffff4d",
			"tab.active.background":            "#454545",
			"tab.active.foreground":            "#ffffff",
			"tab_bar.segmented.background":     "#ffffff12",
			"switch.background":                "#ffffff1f",
		}
	}
	for k, v := range map[string]string{
		"background": base, "title_bar.background": base,
		"foreground": fg, "accent.foreground": fg, "group_box.foreground": fg,
		"group_box.title.foreground": fg, "popover.foreground": fg, "secondary.foreground": fg,
		"button.foreground": fg, "button.secondary.foreground": fg, "sidebar.foreground": fg,
		"sidebar.accent.foreground": fg, "table.head.foreground": fg, "caret": fg,
		"button.primary.background": fg, "sidebar.primary.background": fg, "progress.bar.background": fg,
		"primary.foreground": base, "button.primary.foreground": base, "sidebar.primary.foreground": base,
		"muted.foreground": mfg, "tab.foreground": mfg, "ring": mfg, "slider.background": mfg,
		"scrollbar.thumb.hover.background": mfg,
		"group_box.background":             box, "button.background": button, "button.secondary.background": button,
		"list.background": surface, "table.background": surface, "table.head.background": surface,
		"tab_bar.background": surface, "secondary.background": surface,
		"muted.background":  inset,
		"accent.background": hover, "list.hover.background": hover, "table.hover.background": hover,
		"secondary.hover.background": hover, "button.hover.background": hover,
		"button.secondary.hover.background": hover,
		"secondary.active.background":       active, "button.active.background": active,
		"button.secondary.active.background": active,
		"input.border":                       border, "title_bar.border": border, "table.row.border": border,
		"scrollbar.thumb.background": border,
		"tab.background":             "#00000000", "scrollbar.background": "#00000000",
		"slider.thumb.background": "#ffffff",
	} {
		m[k] = v
	}
	m["table.active.background"] = m["list.active.background"]
	m["table.active.border"] = m["list.active.border"]
	return m
}

// fallbacks are the keys Kit resolves from another key when neither the
// package nor the neutral theme sets them: links take the emphasis color
// and the switch thumb the background.
var fallbacks = [][2]string{
	{"link", "primary.background"},
	{"link.hover", "primary.hover.background"},
	{"link.active", "primary.active.background"},
	{"switch.thumb.background", "background"},
}

// status colors are Kit's own, not package keys: Tailwind's blue, red,
// green and amber 500.
var (
	info    = ui.Hex("#3b82f6")
	danger  = ui.Hex("#ef4444")
	success = ui.Hex("#22c55e")
	warning = ui.Hex("#f59e0b")
	white   = ui.Hex("#ffffff")
	tooltip = ui.Hex("#3a3a3a") // the built-in dark theme's tooltips
)

// base are Kit's base colors (default-theme.json): Tailwind 600 in light,
// 400 in dark, magenta being purple. Orange marks the selected theme
// (500 dark, 600 light).
var base = map[bool][5]ui.Color{
	false: {ui.Hex("#2563eb"), ui.Hex("#16a34a"), ui.Hex("#ca8a04"), ui.Hex("#9333ea"), ui.Hex("#ea580c")},
	true:  {ui.Hex("#60a5fa"), ui.Hex("#4ade80"), ui.Hex("#facc15"), ui.Hex("#c084fc"), ui.Hex("#f97316")},
}

// Tints are the emblem gradients, top to bottom, under OnTint glyphs.
var Tints = [][2]ui.Color{
	{ui.Hex("#3da9fc"), ui.Hex("#2580f4")}, // blue
	{ui.Hex("#9b7cf8"), ui.Hex("#7250e3")}, // violet
	{ui.Hex("#35c5a9"), ui.Hex("#199b87")}, // teal
	{ui.Hex("#f9b74e"), ui.Hex("#e68926")}, // amber
	{ui.Hex("#9398a3"), ui.Hex("#656a75")}, // gray
}

// OnTint is the glyph color over Tints.
var OnTint = white

// Identicon are an agent mark's colors, in its order: Kit's cyan, blue, pink,
// amber, teal, violet, orange and indigo at 400 (dark) and 600 (light).
var Identicon = [8][2]ui.Color{
	{ui.Hex("#22d3ee"), ui.Hex("#0891b2")}, {ui.Hex("#60a5fa"), ui.Hex("#2563eb")},
	{ui.Hex("#f472b6"), ui.Hex("#db2777")}, {ui.Hex("#fbbf24"), ui.Hex("#d97706")},
	{ui.Hex("#2dd4bf"), ui.Hex("#0d9488")}, {ui.Hex("#a78bfa"), ui.Hex("#7c3aed")},
	{ui.Hex("#fb923c"), ui.Hex("#ea580c")}, {ui.Hex("#818cf8"), ui.Hex("#4f46e5")},
}

// Opacity is white at alpha a, the tint that fades a picture.
func Opacity(a float32) ui.Color { return white.Alpha(a) }

// fields maps every theme.colors key onto its Palette field.
var fields = map[string]func(*Palette) *ui.Color{
	"background":                         func(p *Palette) *ui.Color { return &p.Background },
	"foreground":                         func(p *Palette) *ui.Color { return &p.Foreground },
	"muted.foreground":                   func(p *Palette) *ui.Color { return &p.MutedForeground },
	"muted.background":                   func(p *Palette) *ui.Color { return &p.Muted },
	"title_bar.background":               func(p *Palette) *ui.Color { return &p.TitleBar },
	"title_bar.border":                   func(p *Palette) *ui.Color { return &p.TitleBarBorder },
	"sidebar.background":                 func(p *Palette) *ui.Color { return &p.Sidebar },
	"sidebar.foreground":                 func(p *Palette) *ui.Color { return &p.SidebarForeground },
	"sidebar.border":                     func(p *Palette) *ui.Color { return &p.SidebarBorder },
	"sidebar.accent.background":          func(p *Palette) *ui.Color { return &p.SidebarAccent },
	"sidebar.accent.foreground":          func(p *Palette) *ui.Color { return &p.SidebarAccentForeground },
	"sidebar.primary.background":         func(p *Palette) *ui.Color { return &p.SidebarPrimary },
	"sidebar.primary.foreground":         func(p *Palette) *ui.Color { return &p.SidebarPrimaryForeground },
	"group_box.background":               func(p *Palette) *ui.Color { return &p.Surface },
	"group_box.foreground":               func(p *Palette) *ui.Color { return &p.SurfaceForeground },
	"group_box.title.foreground":         func(p *Palette) *ui.Color { return &p.SurfaceTitle },
	"popover.background":                 func(p *Palette) *ui.Color { return &p.Popover },
	"popover.foreground":                 func(p *Palette) *ui.Color { return &p.PopoverForeground },
	"border":                             func(p *Palette) *ui.Color { return &p.Border },
	"input.border":                       func(p *Palette) *ui.Color { return &p.Input },
	"ring":                               func(p *Palette) *ui.Color { return &p.Ring },
	"link":                               func(p *Palette) *ui.Color { return &p.Link },
	"link.hover":                         func(p *Palette) *ui.Color { return &p.LinkHover },
	"link.active":                        func(p *Palette) *ui.Color { return &p.LinkActive },
	"primary.background":                 func(p *Palette) *ui.Color { return &p.Primary },
	"primary.foreground":                 func(p *Palette) *ui.Color { return &p.PrimaryForeground },
	"primary.hover.background":           func(p *Palette) *ui.Color { return &p.PrimaryHover },
	"primary.active.background":          func(p *Palette) *ui.Color { return &p.PrimaryActive },
	"button.primary.background":          func(p *Palette) *ui.Color { return &p.ButtonPrimary },
	"button.primary.foreground":          func(p *Palette) *ui.Color { return &p.OnPrimary },
	"button.primary.hover.background":    func(p *Palette) *ui.Color { return &p.ButtonPrimaryHi },
	"button.primary.active.background":   func(p *Palette) *ui.Color { return &p.ButtonPrimaryActive },
	"accent.background":                  func(p *Palette) *ui.Color { return &p.Hover },
	"accent.foreground":                  func(p *Palette) *ui.Color { return &p.HoverForeground },
	"secondary.background":               func(p *Palette) *ui.Color { return &p.Secondary },
	"secondary.foreground":               func(p *Palette) *ui.Color { return &p.SecondaryForeground },
	"secondary.hover.background":         func(p *Palette) *ui.Color { return &p.SecondaryHover },
	"secondary.active.background":        func(p *Palette) *ui.Color { return &p.Active },
	"button.background":                  func(p *Palette) *ui.Color { return &p.Button },
	"button.foreground":                  func(p *Palette) *ui.Color { return &p.ButtonForeground },
	"button.hover.background":            func(p *Palette) *ui.Color { return &p.ButtonHover },
	"button.active.background":           func(p *Palette) *ui.Color { return &p.ButtonActive },
	"button.secondary.background":        func(p *Palette) *ui.Color { return &p.ButtonSecondary },
	"button.secondary.foreground":        func(p *Palette) *ui.Color { return &p.ButtonSecondaryForeground },
	"button.secondary.hover.background":  func(p *Palette) *ui.Color { return &p.ButtonSecondaryHover },
	"button.secondary.active.background": func(p *Palette) *ui.Color { return &p.ButtonSecondaryActive },
	"tab.background":                     func(p *Palette) *ui.Color { return &p.Tab },
	"tab.foreground":                     func(p *Palette) *ui.Color { return &p.TabForeground },
	"tab.active.background":              func(p *Palette) *ui.Color { return &p.TabActive },
	"tab.active.foreground":              func(p *Palette) *ui.Color { return &p.TabActiveText },
	"tab_bar.background":                 func(p *Palette) *ui.Color { return &p.TabBarBackground },
	"tab_bar.segmented.background":       func(p *Palette) *ui.Color { return &p.TabBar },
	"switch.background":                  func(p *Palette) *ui.Color { return &p.Switch },
	"switch.thumb.background":            func(p *Palette) *ui.Color { return &p.SwitchThumb },
	"slider.background":                  func(p *Palette) *ui.Color { return &p.Slider },
	"slider.thumb.background":            func(p *Palette) *ui.Color { return &p.SliderThumb },
	"progress.bar.background":            func(p *Palette) *ui.Color { return &p.Progress },
	"scrollbar.background":               func(p *Palette) *ui.Color { return &p.ScrollbarTrack },
	"scrollbar.thumb.background":         func(p *Palette) *ui.Color { return &p.ScrollbarThumb },
	"scrollbar.thumb.hover.background":   func(p *Palette) *ui.Color { return &p.ScrollbarThumbHover },
	"selection.background":               func(p *Palette) *ui.Color { return &p.Selection },
	"caret":                              func(p *Palette) *ui.Color { return &p.Caret },
	"list.background":                    func(p *Palette) *ui.Color { return &p.List },
	"list.hover.background":              func(p *Palette) *ui.Color { return &p.ListHover },
	"list.active.background":             func(p *Palette) *ui.Color { return &p.ListActive },
	"list.active.border":                 func(p *Palette) *ui.Color { return &p.ListActiveBorder },
	"table.background":                   func(p *Palette) *ui.Color { return &p.Table },
	"table.head.background":              func(p *Palette) *ui.Color { return &p.TableHead },
	"table.head.foreground":              func(p *Palette) *ui.Color { return &p.TableHeadForeground },
	"table.hover.background":             func(p *Palette) *ui.Color { return &p.TableHover },
	"table.active.background":            func(p *Palette) *ui.Color { return &p.TableActive },
	"table.active.border":                func(p *Palette) *ui.Color { return &p.TableActiveBorder },
	"table.row.border":                   func(p *Palette) *ui.Color { return &p.TableRowBorder },
}
