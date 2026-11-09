// Package theme holds Sailry's one application theme: the semantic tokens of
// the built-in default theme (sailry-public apps/desktop/src/theme/neutral.rs)
// and the MyGo widget theme derived from them.
package theme

import "github.com/egoist/mygo/ui"

// Mode is the appearance the user chose.
type Mode int

const (
	System Mode = iota
	Light
	Dark
)

// Palette is the semantic tokens views paint with. Names follow the Kit
// tokens of the reference app so values can be compared one to one.
type Palette struct {
	Dark bool

	Background      ui.Color // window and main panel
	Sidebar         ui.Color // navigation surfaces
	Foreground      ui.Color
	MutedForeground ui.Color
	Surface         ui.Color // group boxes, secondary buttons
	Muted           ui.Color // inset fills such as the composer
	Hover           ui.Color // Kit accent: hover surface
	Active          ui.Color // Kit secondary_active: pressed and selected
	SidebarAccent   ui.Color
	Border          ui.Color
	Input           ui.Color
	SidebarBorder   ui.Color
	Popover         ui.Color
	Primary         ui.Color // emphasis and links
	PrimaryHover    ui.Color
	ButtonPrimary   ui.Color // filled buttons are neutral
	ButtonPrimaryHi ui.Color
	OnPrimary       ui.Color
	Selection       ui.Color
	Switch          ui.Color
	TabBar          ui.Color // Kit tab_bar_segmented
	TabActive       ui.Color
	TabActiveText   ui.Color
	Danger          ui.Color
	Success         ui.Color
	Warning         ui.Color
}

// Window material opacities: Surfaces::default in preferences.rs keeps the
// main panel at 83.5% and the outer sidebar material at 45% of the base.
const (
	PanelOpacity   = 0.835
	SidebarOpacity = 0.45
)

var light = &Palette{
	Background:      ui.Hex("#ffffff"),
	Sidebar:         ui.Hex("#fafafa"),
	Foreground:      ui.Hex("#000000e0"),
	MutedForeground: ui.Hex("#00000099"),
	Surface:         ui.Hex("#00000000"),
	Muted:           ui.Hex("#00000006"),
	Hover:           ui.Hex("#00000010"),
	Active:          ui.Hex("#00000008"),
	SidebarAccent:   ui.Hex("#00000008"),
	Border:          ui.Hex("#00000010"),
	Input:           ui.Hex("#00000010"),
	SidebarBorder:   ui.Hex("#00000014"),
	Popover:         ui.Hex("#fffffff2"),
	Primary:         ui.Hex("#2563eb"), // blue-600
	PrimaryHover:    ui.Hex("#3b82f6"), // blue-500
	ButtonPrimary:   ui.Hex("#000000e0"),
	ButtonPrimaryHi: ui.Hex("#000000f0"),
	OnPrimary:       ui.Hex("#ffffff"),
	Selection:       ui.Hex("#00000018"),
	Switch:          ui.Hex("#0000001f"),
	TabBar:          ui.Hex("#00000008"),
	TabActive:       ui.Hex("#ffffff"),
	TabActiveText:   ui.Hex("#181818"),
	Danger:          ui.Hex("#ef4444"),
	Success:         ui.Hex("#22c55e"),
	Warning:         ui.Hex("#f59e0b"),
}

var dark = &Palette{
	Dark:            true,
	Background:      ui.Hex("#181818"),
	Sidebar:         ui.Hex("#1d1d1d"),
	Foreground:      ui.Hex("#ffffffd9"),
	MutedForeground: ui.Hex("#ffffff99"),
	Surface:         ui.Hex("#ffffff0a"),
	Muted:           ui.Hex("#ffffff12"),
	Hover:           ui.Hex("#ffffff0f"),
	Active:          ui.Hex("#ffffff1f"),
	SidebarAccent:   ui.Hex("#ffffff0f"),
	Border:          ui.Hex("#ffffff0c"),
	Input:           ui.Hex("#ffffff10"),
	SidebarBorder:   ui.Hex("#ffffff14"),
	Popover:         ui.Hex("#242424f2"),
	Primary:         ui.Hex("#60a5fa"), // blue-400
	PrimaryHover:    ui.Hex("#93c5fd"), // blue-300
	ButtonPrimary:   ui.Hex("#ffffffd9"),
	ButtonPrimaryHi: ui.Hex("#fffffff0"),
	OnPrimary:       ui.Hex("#181818"),
	Selection:       ui.Hex("#ffffff4d"),
	Switch:          ui.Hex("#ffffff1f"),
	TabBar:          ui.Hex("#ffffff12"),
	TabActive:       ui.Hex("#454545"),
	TabActiveText:   ui.Hex("#ffffff"),
	Danger:          ui.Hex("#ef4444"),
	Success:         ui.Hex("#22c55e"),
	Warning:         ui.Hex("#f59e0b"),
}

var widgets = map[bool]*ui.Theme{false: build(light), true: build(dark)}

// Of returns the palette of the frame's theme.
func Of(c *ui.Context) *Palette {
	if c.Theme().Dark {
		return dark
	}
	return light
}

// Apply sets the window's theme for the mode. It runs first in the view,
// where c.Theme() is still the system's appearance.
func Apply(c *ui.Context, m Mode) {
	isDark := c.Theme().Dark
	switch m {
	case Light:
		isDark = false
	case Dark:
		isDark = true
	}
	c.SetTheme(widgets[isDark])
}

// SidebarItem is the row fill of sidebar_item in theme.rs.
func (p *Palette) SidebarItem(selected, hovered bool) ui.Color {
	if !p.Dark {
		switch {
		case selected && hovered:
			return p.Active.MixOklab(p.Hover, 0.75)
		case selected:
			return p.Active
		case hovered:
			return p.Hover
		}
		return ui.Transparent
	}
	a := float32(0)
	switch {
	case selected && hovered:
		a = 0.12
	case selected:
		a = 0.06
	case hovered:
		a = 0.05
	}
	return ui.Hex("#ffffff").Alpha(a)
}

// Description is the receding secondary text of media rows and the sidebar.
func (p *Palette) Description() ui.Color { return p.MutedForeground.Alpha(0.7) }

// build maps the palette onto MyGo's widget tokens, so built-in controls
// (inputs, menus, popovers, switches, scroll bars) follow the same theme.
func build(p *Palette) *ui.Theme {
	base := ui.LightTheme()
	if p.Dark {
		base = ui.DarkTheme()
	}
	t := *base
	t.Background = p.Background
	t.Surface = p.Surface
	t.SurfaceHover = p.Hover
	t.SurfacePressed = p.Active
	t.Border = p.Input
	t.Text = p.Foreground
	t.TextMuted = p.MutedForeground
	// MyGo fills checked switches, sliders and selected rows with Accent:
	// Kit's emphasis (primary) colors those. Sailry's neutral filled
	// buttons are theme.Primary, which does not use it.
	t.Accent = p.Primary
	t.AccentHover = p.PrimaryHover
	t.AccentPressed = p.PrimaryHover
	t.AccentText = ui.Hex("#ffffff")
	t.InactiveAccent = p.Primary
	t.Danger, t.Success, t.Warning = p.Danger, p.Success, p.Warning
	t.Selection = p.Selection
	t.Focus = p.MutedForeground
	t.Scrollbar = p.MutedForeground.Alpha(0.5)
	// Kit: radius 6, body text 14 (text_sm on a 16px rem).
	t.Radius = 6
	t.FontSize = 14
	if p.Dark {
		t.Inverse, t.InverseText = ui.Hex("#3a3a3a"), ui.Hex("#ffffff")
	}
	return &t
}

// ContentWidth is the transcript's and the composer's maximum width
// (preview.rs).
const ContentWidth = 800
