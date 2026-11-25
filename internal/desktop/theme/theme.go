// Package theme holds Sailry's one application theme: the semantic tokens of
// the built-in default theme overlaid by the selected theme package
// (package.go), and the MyGo widget theme derived from them.
package theme

import (
	"runtime"
	"sync"
	"sync/atomic"
	"weak"

	"github.com/egoist/mygo/ui"
)

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

	// Radius is the base radius (8) and RadiusLg the large one (12), a
	// little rounder than Kit's 6 and 8; a theme may set its own.
	Radius, RadiusLg float32

	Background      ui.Color // window and main panel
	Foreground      ui.Color
	MutedForeground ui.Color
	Muted           ui.Color // inset fills such as the composer

	TitleBar       ui.Color // the outer material of the title bar and rail
	TitleBarBorder ui.Color

	Sidebar                 ui.Color // navigation surfaces
	SidebarForeground       ui.Color
	SidebarBorder           ui.Color
	SidebarAccentForeground ui.Color
	// SidebarAccent is a package's sidebar.accent.background, the fill of
	// the chosen sidebar row; zero keeps the mode's neutral fill.
	SidebarAccent ui.Color

	Surface           ui.Color // group_box: settings groups, cards
	SurfaceForeground ui.Color
	SurfaceTitle      ui.Color

	Popover           ui.Color
	PopoverForeground ui.Color

	Border ui.Color
	Input  ui.Color // input.border
	Ring   ui.Color

	Link ui.Color

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

	Button ui.Color // ghost

	ButtonSecondary, ButtonSecondaryForeground  ui.Color // outline
	ButtonSecondaryHover, ButtonSecondaryActive ui.Color

	Tab           ui.Color
	TabActive     ui.Color
	TabActiveText ui.Color

	SwitchThumb         ui.Color
	Slider, SliderThumb ui.Color
	Progress            ui.Color
	ScrollbarThumbHover ui.Color
	Selection, Caret    ui.Color
	List, ListHover     ui.Color
	Table               ui.Color

	// Kit's status and base colors; packages do not set them.
	Info, Danger, DangerForeground, Success, Warning ui.Color
	Blue, Green, Yellow, Magenta, Orange             ui.Color

	// Chart is the categorical chart colors, chart.1 to chart.8: distinct
	// hues for the series of one chart or the rows of one legend. A
	// package sets the first five; the others keep the built-in values.
	Chart [ChartColors]ui.Color
}

// ChartColors is how many chart colors a palette has.
const ChartColors = 8

// Series is the color of the i-th series or slice of a chart the
// application draws: the chart colors from the blue on and, from the ninth
// series, a step toward the background each lap.
func (p *Palette) Series(i int) ui.Color {
	c := p.Chart[series[i%ChartColors]]
	if lap := i / ChartColors % 3; lap > 0 {
		ground := p.Background
		ground.A = 255
		c = c.Mix(ground, 0.35*float32(lap))
	}
	return c
}

// Window material opacities: defaults keep the main panel at 83.5% and the
// outer sidebar material at 45% of the base.
const (
	PanelOpacity   = 0.835
	SidebarOpacity = 0.45
)

// Surfaces are the window materials' opacities, 0 to 1, that the Appearance
// page's transparency sliders set: the main panel's and the outer sidebar
// band's; and Blur, 0 to 1, how much the columns blur a theme's window art (0
// none, 1 PaneBlurMax DIP).
type Surfaces struct{ Main, Sidebar, Blur float32 }

// PaneBlurMax is the columns' blur radius at Surfaces.Blur 1, in DIP.
const PaneBlurMax = 36

// DefaultSurfaces are Surfaces::default.
var DefaultSurfaces = Surfaces{Main: PanelOpacity, Sidebar: SidebarOpacity, Blur: 0.1}

// Radius2xl is Kit's radius_2xl: the composer's and the mode cards'.
func (p *Palette) Radius2xl() float32 { return p.Radius * 2.5 }

// palette overlays a package's colors on the neutral theme, key by key.
func palette(pkg *Package, dark bool) *Palette {
	p := &Palette{Dark: dark, Package: pkg, Variant: pkg.Variant(dark), Radius: 8, RadiusLg: 12}
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
		if c := v.Theme.Colors["sidebar.accent.background"]; c.OK {
			p.SidebarAccent = c.V
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
	p.Chart = charts[dark]
	if v := p.Variant; v != nil {
		for i, k := range chartKeys {
			if c := v.Theme.Colors[k]; c.OK {
				p.Chart[i] = c.V
			}
		}
	}
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

type retiredLook struct {
	palette *Palette
	neutral *ui.Theme
}

// registry is the themes in use. It is replaced whole, never changed, so
// every window's frame reads one consistent set.
type registry struct {
	packages []*Package
	looks    map[key]look
	byWidget map[*ui.Theme]*Palette
	// neutral is, by a look's widget theme, the one Neutral returns for it.
	neutral map[*ui.Theme]*ui.Theme
}

func (r *registry) appearance(id string, dark bool) look {
	if selected, ok := r.looks[key{id, dark}]; ok {
		return selected
	}
	return r.looks[key{BuiltinID, dark}]
}

var (
	current  atomic.Pointer[registry]
	changing sync.Mutex
	retired  sync.Map // weak.Pointer[ui.Theme] to retiredLook
)

func init() { SetInstalled(nil) }

// Packages are the themes in use: the built-in default first, then the
// installed packages.
func Packages() []*Package { return current.Load().packages }

// SetInstalled replaces the installed packages. A package that stays keeps
// its looks, so a window showing it keeps its theme; the looks of replaced
// packages stay known to the windows still showing them.
func SetInstalled(installed []*Package) {
	changing.Lock()
	defer changing.Unlock()
	next := &registry{packages: []*Package{Builtin}, looks: map[key]look{}, byWidget: map[*ui.Theme]*Palette{}, neutral: map[*ui.Theme]*ui.Theme{}}
	for _, pkg := range installed {
		replaced := false
		for index, existing := range next.packages {
			if existing.ID == pkg.ID {
				next.packages[index] = pkg
				replaced = true
				break
			}
		}
		if !replaced {
			next.packages = append(next.packages, pkg)
		}
	}
	previous := current.Load()
	for _, pkg := range next.packages {
		for _, dark := range []bool{false, true} {
			id := key{pkg.ID, dark}
			var styles look
			var neutral *ui.Theme
			if previous != nil {
				styles = previous.looks[id]
			}
			if styles.palette == nil || styles.palette.Package != pkg {
				value := palette(pkg, dark)
				styles = look{value, build(value)}
				alternate := *styles.widget
				alternate.Accent, alternate.AccentHover, alternate.AccentPressed = value.Selected(), value.Selected(), value.Selected()
				alternate.InactiveAccent, alternate.AccentText = value.Selected(), value.Foreground
				neutral = &alternate
			} else {
				neutral = previous.neutral[styles.widget]
			}
			next.looks[id] = styles
			next.byWidget[styles.widget], next.byWidget[neutral] = styles.palette, styles.palette
			next.neutral[styles.widget] = neutral
		}
	}
	if previous != nil {
		for id, styles := range previous.looks {
			if next.looks[id].widget != styles.widget {
				neutral := previous.neutral[styles.widget]
				rememberRetired(styles.widget, styles.palette, neutral)
				rememberRetired(neutral, styles.palette, nil)
			}
		}
	}
	current.Store(next)
}

func rememberRetired(widget *ui.Theme, palette *Palette, neutral *ui.Theme) {
	// A window may still paint with the replaced widget. Retain its palette
	// only for the widget's lifetime, without rooting the widget itself.
	key := weak.Make(widget)
	retired.Store(key, retiredLook{palette, neutral})
	runtime.AddCleanup(widget, func(key weak.Pointer[ui.Theme]) { retired.Delete(key) }, key)
}

func retiredAppearance(widget *ui.Theme) retiredLook {
	value, ok := retired.Load(weak.Make(widget))
	runtime.KeepAlive(widget)
	if ok {
		return value.(retiredLook)
	}
	return retiredLook{}
}

// Of returns the palette of the frame's theme, or of the Themed subtree
// being built.
func Of(c *ui.Context) *Palette {
	themes, widget := current.Load(), c.Theme()
	if p := themes.byWidget[widget]; p != nil {
		return p
	}
	if old := retiredAppearance(widget); old.palette != nil {
		return old.palette
	}
	return themes.looks[key{BuiltinID, widget.Dark}].palette
}

// Neutral returns the frame's theme with MyGo's accent replaced by the
// neutral Selected fill under the normal foreground, for ui.Themed around a
// list, table or grid whose chosen rows are marked as the app's other
// neutral selections are, not in the emphasis color. Of gives the same
// palette inside it; the focus ring is unchanged.
func Neutral(c *ui.Context) *ui.Theme {
	widget := c.Theme()
	if neutral := current.Load().neutral[widget]; neutral != nil {
		return neutral
	}
	if old := retiredAppearance(widget); old.neutral != nil {
		return old.neutral
	}
	return widget
}

// Look returns the palette of a package in a mode, as previews show it.
func Look(id string, dark bool) *Palette {
	return current.Load().appearance(id, dark).palette
}

func Preview(pkg *Package, dark bool) *Palette {
	if cached, exists := current.Load().looks[key{pkg.ID, dark}]; exists && cached.palette.Package == pkg {
		return cached.palette
	}
	return palette(pkg, dark)
}

// Widget returns the MyGo theme of a package in a mode, for ui.Themed.
func Widget(id string, dark bool) *ui.Theme {
	return current.Load().appearance(id, dark).widget
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

// SidebarItem is the sidebar row fill.
func (p *Palette) SidebarItem(selected, hovered bool) ui.Color {
	if selected && p.SidebarAccent.A > 0 {
		return p.SidebarAccent
	}
	if !p.Dark {
		switch {
		case selected && hovered:
			return p.Selected()
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

// Selected is the neutral fill of the chosen row of a list that marks its
// choice by the fill alone, as the model picker's: Active laid over Hover.
// It is therefore stronger than Hover, the fill of the row under the
// pointer, in every theme, including a package whose Active alone is
// fainter than its Hover.
func (p *Palette) Selected() ui.Color {
	top, under := float32(p.Active.A)/255, float32(p.Hover.A)/255
	alpha := top + under*(1-top)
	if alpha <= 0 {
		return ui.Transparent
	}
	fill := p.Active.Mix(p.Hover, under*(1-top)/alpha)
	fill.A = uint8(alpha*255 + 0.5)
	return fill
}

// Floating is the opaque fill of popovers and dialogs, the package's
// popover.background at full opacity (DialogStyle::floating_surface).
func (p *Palette) Floating() ui.Color { return p.Popover.Alpha(1) }

// Backdrop dims the window under a dialog, as MyGo's Modal.
func (p *Palette) Backdrop() ui.Color { return ui.RGBA(0, 0, 0, 0.4) }

// Description is the receding secondary text of media rows and the sidebar.
func (p *Palette) Description() ui.Color { return p.MutedForeground.Alpha(0.7) }

// Separator is the hairline between a card's rows: fainter than the border
// around them in light mode, the border itself in dark mode, where the
// border is already so faint that half of it disappears.
func (p *Palette) Separator() ui.Color {
	if p.Dark {
		return p.Border
	}
	return p.Border.Alpha(0.5)
}

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

// ContentWidth is the transcript's and the composer's maximum width.
const ContentWidth = 800

// Welcome is the welcome guide's approved peach-to-coral brand banner and the
// white over it. Themes have no brand banner token; the reference paints
// these two stops for the design's four, and so does this.
var Welcome = struct {
	Banner ui.LinearGradient
	Text   ui.Color
}{
	Banner: ui.LinearGradient{From: ui.RGB(0xff, 0xd9, 0xcc), To: ui.RGB(0xe8, 0x68, 0x7f), Angle: 135},
	Text:   ui.RGB(0xff, 0xff, 0xff),
}
