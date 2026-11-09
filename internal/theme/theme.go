// Package theme holds Sailry's one application theme: the semantic tokens of
// the built-in default theme (sailry-public apps/desktop/src/theme/neutral.rs)
// overlaid by the selected theme package (package.go), and the MyGo widget
// theme derived from them.
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
// tokens of the reference app so values can be compared one to one; the
// comments name the theme.json keys that differ.
type Palette struct {
	Dark bool

	// Package is the theme the palette belongs to; Variant is its half for
	// the mode, nil for the built-in theme, which has no art.
	Package *Package
	Variant *Variant

	// Radius is Kit's base radius (6) and RadiusLg its large one (8).
	Radius, RadiusLg float32

	Background      ui.Color // window and main panel
	Foreground      ui.Color
	MutedForeground ui.Color
	Muted           ui.Color // inset fills such as the composer

	TitleBar       ui.Color // the outer material of the title bar and rail
	TitleBarBorder ui.Color

	Sidebar                  ui.Color // navigation surfaces
	SidebarForeground        ui.Color
	SidebarBorder            ui.Color
	SidebarAccent            ui.Color
	SidebarAccentForeground  ui.Color
	SidebarPrimary           ui.Color
	SidebarPrimaryForeground ui.Color

	Surface           ui.Color // group_box: settings groups, cards
	SurfaceForeground ui.Color
	SurfaceTitle      ui.Color

	Popover           ui.Color // 95% of the package's, as floating.rs
	PopoverForeground ui.Color

	Border ui.Color
	Input  ui.Color // input.border
	Ring   ui.Color

	Link, LinkHover, LinkActive ui.Color

	Primary           ui.Color // emphasis: tags, checked controls
	PrimaryHover      ui.Color
	PrimaryActive     ui.Color
	PrimaryForeground ui.Color

	ButtonPrimary       ui.Color // filled buttons
	ButtonPrimaryHi     ui.Color // button.primary.hover.background
	ButtonPrimaryActive ui.Color
	OnPrimary           ui.Color // button.primary.foreground

	Hover           ui.Color // accent.background: hover surface
	HoverForeground ui.Color // accent.foreground
	Active          ui.Color // secondary.active.background: pressed, selected

	Secondary           ui.Color
	SecondaryForeground ui.Color
	SecondaryHover      ui.Color

	Button, ButtonForeground, ButtonHover, ButtonActive ui.Color // ghost

	ButtonSecondary, ButtonSecondaryForeground  ui.Color // outline
	ButtonSecondaryHover, ButtonSecondaryActive ui.Color

	Tab              ui.Color
	TabForeground    ui.Color
	TabActive        ui.Color
	TabActiveText    ui.Color
	TabBarBackground ui.Color // tab_bar.background
	TabBar           ui.Color // tab_bar.segmented.background

	Switch, SwitchThumb ui.Color
	Slider, SliderThumb ui.Color
	Progress            ui.Color
	ScrollbarTrack      ui.Color
	ScrollbarThumb      ui.Color
	ScrollbarThumbHover ui.Color
	Selection, Caret    ui.Color
	List, ListHover     ui.Color
	ListActive          ui.Color
	ListActiveBorder    ui.Color
	Table, TableHead    ui.Color
	TableHeadForeground ui.Color
	TableHover          ui.Color
	TableActive         ui.Color
	TableActiveBorder   ui.Color
	TableRowBorder      ui.Color

	// Kit's status and base colors; packages do not set them.
	Info, Danger, DangerForeground, Success, Warning ui.Color
	Blue, Green, Yellow, Magenta, Orange             ui.Color
}

// Window material opacities: Surfaces::default in preferences.rs keeps the
// main panel at 83.5% and the outer sidebar material at 45% of the base.
const (
	PanelOpacity   = 0.835
	SidebarOpacity = 0.45
)

// Surfaces are the window materials' opacities, 0 to 1, that the
// Appearance page's transparency sliders set (preferences.rs Surfaces):
// the main panel's and the outer sidebar band's; and Blur, 0 to 1, how
// much the columns blur a theme's window art (0 none, 1 PaneBlurMax DIP).
type Surfaces struct{ Main, Sidebar, Blur float32 }

// PaneBlurMax is the columns' blur radius at Surfaces.Blur 1, in DIP.
const PaneBlurMax = 36

// DefaultSurfaces are Surfaces::default.
var DefaultSurfaces = Surfaces{Main: PanelOpacity, Sidebar: SidebarOpacity, Blur: 0.1}

// Radius2xl is Kit's radius_2xl: the composer's and the mode cards'.
func (p *Palette) Radius2xl() float32 { return p.Radius * 2.5 }

// palette overlays a package's colors on the neutral theme, key by key.
func palette(pkg *Package, dark bool) *Palette {
	p := &Palette{Dark: dark, Package: pkg, Variant: pkg.Variant(dark), Radius: 6, RadiusLg: 8}
	set := map[string]bool{}
	for k, v := range neutral(dark) {
		*fields[k](p) = ui.Hex(v)
		set[k] = true
	}
	if v := p.Variant; v != nil {
		for k, c := range v.Theme.Colors {
			if f := fields[k]; f != nil && c.OK {
				*f(p) = c.V
				set[k] = true
			}
		}
		if v.Theme.Radius != nil {
			p.Radius = *v.Theme.Radius
		}
		if v.Theme.RadiusLg != nil {
			p.RadiusLg = *v.Theme.RadiusLg
		}
	}
	for _, f := range fallbacks {
		if !set[f[0]] {
			*fields[f[0]](p) = *fields[f[1]](p)
		}
	}
	p.Popover = p.Popover.Alpha(0.95)
	p.Info, p.Danger, p.DangerForeground, p.Success, p.Warning = info, danger, white, success, warning
	b := base[dark]
	p.Blue, p.Green, p.Yellow, p.Magenta, p.Orange = b[0], b[1], b[2], b[3], b[4]
	return p
}

type key struct {
	id   string
	dark bool
}

// look is a package's palette and widget theme for a mode.
type look struct {
	palette *Palette
	widget  *ui.Theme
}

var (
	looks    = map[key]look{}
	byWidget = map[*ui.Theme]*Palette{}
)

func init() {
	for _, pkg := range Packages {
		for _, dark := range []bool{false, true} {
			p := palette(pkg, dark)
			w := build(p)
			looks[key{pkg.ID, dark}] = look{p, w}
			byWidget[w] = p
		}
	}
}

// Of returns the palette of the frame's theme, or of the Themed subtree
// being built.
func Of(c *ui.Context) *Palette {
	if p := byWidget[c.Theme()]; p != nil {
		return p
	}
	return looks[key{BuiltinID, c.Theme().Dark}].palette
}

// Look returns the palette of a package in a mode, as previews show it.
func Look(id string, dark bool) *Palette {
	return looks[key{Find(id).ID, dark}].palette
}

// Widget returns the MyGo theme of a package in a mode, for ui.Themed.
func Widget(id string, dark bool) *ui.Theme {
	return looks[key{Find(id).ID, dark}].widget
}

// IsDark resolves the mode against the system's appearance.
func IsDark(c *ui.Context, m Mode) bool {
	switch m {
	case Light:
		return false
	case Dark:
		return true
	}
	return c.Theme().Dark
}

// Apply sets the window's theme to the package id in the mode. It runs
// first in the view, where c.Theme() is still the system's appearance, so
// switching either repaints the whole window.
func Apply(c *ui.Context, id string, m Mode) {
	c.SetTheme(Widget(id, IsDark(c, m)))
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
	return p.SidebarForeground.Alpha(a)
}

// Floating is the opaque fill of popovers and dialogs, the package's
// popover.background at full opacity (DialogStyle::floating_surface).
func (p *Palette) Floating() ui.Color { return p.Popover.Alpha(1) }

// Backdrop dims the window under a dialog, as MyGo's Modal.
func (p *Palette) Backdrop() ui.Color { return ui.RGBA(0, 0, 0, 0.4) }

// Description is the receding secondary text of media rows and the sidebar.
func (p *Palette) Description() ui.Color { return p.MutedForeground.Alpha(0.7) }

// build maps the palette onto MyGo's widget tokens, so built-in controls
// (inputs, menus, popovers, switches, segmented controls, scroll bars)
// follow the same theme.
func build(p *Palette) *ui.Theme {
	base := ui.LightTheme()
	if p.Dark {
		base = ui.DarkTheme()
	}
	t := *base
	t.Dark = p.Dark
	t.Background = p.Background
	t.Surface = p.Surface
	t.SurfaceHover = p.Hover
	t.SurfacePressed = p.Active
	t.Border = p.Input
	t.Text = p.Foreground
	t.TextMuted = p.MutedForeground
	// MyGo fills checked switches, sliders and selected rows with Accent:
	// Kit's emphasis (primary) colors those. Sailry's filled buttons are
	// ButtonPrimary, which does not use it.
	t.Accent = p.Primary
	t.AccentHover = p.PrimaryHover
	t.AccentPressed = p.PrimaryActive
	t.AccentText = p.PrimaryForeground
	t.InactiveAccent = p.Primary
	t.Danger, t.Success, t.Warning = p.Danger, p.Success, p.Warning
	t.Selection = p.Selection
	t.Focus = p.Ring
	t.Scrollbar = p.ScrollbarThumbHover.Alpha(0.5)
	t.Radius = p.Radius
	// Kit: body text 14 (text_sm on a 16px rem).
	t.FontSize = 14
	switch {
	case p.Dark && p.Variant == nil:
		t.Inverse, t.InverseText = tooltip, white
	case p.Dark:
		t.Inverse, t.InverseText = p.Popover.Over(p.Background), p.PopoverForeground
	}
	return &t
}

// ContentWidth is the transcript's and the composer's maximum width
// (preview.rs).
const ContentWidth = 800

// Welcome is the welcome guide's approved peach-to-coral brand banner and
// the white over it (onboarding.rs BANNER_FROM, BANNER_TO, BANNER_ANGLE).
// Themes have no brand banner token; the reference paints these two stops
// for the design's four, and so does this.
var Welcome = struct {
	Banner ui.LinearGradient
	Text   ui.Color
}{
	Banner: ui.LinearGradient{From: ui.RGB(0xff, 0xd9, 0xcc), To: ui.RGB(0xe8, 0x68, 0x7f), Angle: 135},
	Text:   ui.RGB(0xff, 0xff, 0xff),
}
