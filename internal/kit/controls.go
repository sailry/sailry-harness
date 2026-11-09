package kit

import (
	"math"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/theme"
)

// Ghost is Kit's ghost button with Sailry's subtle_button states (theme.rs):
// transparent, accent on hover, secondary_active when pressed or selected.
// It is a MyGo ButtonBase, which keeps focus, keys and accessibility.
func Ghost(c *ui.Context, label string, selected bool) ui.Element {
	return GhostLook(c, ui.ButtonBase(c).Label(label), selected)
}

// GhostLook gives a button without a look Kit's ghost states, as Ghost.
func GhostLook(c *ui.Context, b ui.Element, selected bool) ui.Element {
	p := theme.Of(c)
	b.Radius(p.Radius).Center().TextColor(p.Foreground).Cursor(ui.CursorPointer)
	switch {
	case b.Pressed() || selected:
		b.Background(p.Active)
	case b.Hovered():
		b.Background(p.Hover)
	}
	return b
}

// IconButton is a square ghost button holding one icon, named by label for
// its tooltip and assistive technology. Kit sizes: xsmall 20, small 28,
// default 32, rail 36; icons are 16 (14 below 24).
func IconButton(c *ui.Context, icon *ui.SVG, label string, size float32, selected bool) ui.Element {
	return IconButtonHint(c, icon, label, "", size, selected)
}

// IconButtonHint is an IconButton whose tooltip also shows the shortcut
// that does the same, hint.
func IconButtonHint(c *ui.Context, icon *ui.SVG, label, hint string, size float32, selected bool) ui.Element {
	return IconLook(c, ui.ButtonBase(c), icon, label, hint, size, selected)
}

// IconLook gives a button without a look, as a MenuButton's trigger, the
// look of an IconButtonHint.
func IconLook(c *ui.Context, b ui.Element, icon *ui.SVG, label, hint string, size float32, selected bool) ui.Element {
	tip := label
	if hint != "" {
		tip += " " + hint
	}
	Tooltip(c, GhostLook(c, b.Label(label), selected).Size(size, size), tip)
	glyph := float32(15)
	if size < 24 {
		glyph = 14
	}
	return b.Children(func() { ui.Icon(c, icon).FontSize(glyph) })
}

// Primary is Kit's primary button in the button.primary colors: the
// neutral foreground fill of the built-in theme.
func Primary(c *ui.Context, label string, disabled bool) ui.Element {
	p := theme.Of(c)
	b := ui.ButtonBase(c).Label(label).Disabled(disabled).DimDisabled(false).Radius(p.Radius).Center().Gap(6).
		PaddingX(12).Height(32).TextColor(p.OnPrimary).Background(p.ButtonPrimary)
	switch {
	case disabled && !p.Dark:
		b.Background(p.Active).TextColor(p.MutedForeground.Alpha(0.5))
	case disabled:
		b.Background(p.ButtonPrimary.Alpha(0.15)).TextColor(p.MutedForeground.Alpha(0.5))
	case b.Pressed():
		b.Background(p.ButtonPrimaryActive)
	case b.Hovered():
		b.Background(p.ButtonPrimaryHi)
	}
	return b
}

// Tag is Kit's small Tag: a rounded label in the emphasis color at 12%.
func Tag(c *ui.Context, text string) ui.Element {
	p := theme.Of(c)
	return ui.Text(c, text).FontSize(12).SingleLine().Padding(1, 6).Radius(999).
		Background(p.Primary.Alpha(0.12)).TextColor(p.Primary)
}

// Loader draws the sidebar's running indicator (ui/loading.rs in the
// reference): a 2x3 dot matrix whose dots light in turn. Custom drawing: MyGo
// has a Spinner of spokes but no dot matrix, and a Spinner cannot be capped
// at 60fps.
func Loader(c *ui.Context, size float32) ui.Element {
	p := theme.Of(c)
	colors := [3]ui.Color{p.Info, p.Warning, p.Danger}
	return ui.Box(c).Size(size, size).Label("running").Draw(func(pt *ui.Painter, r ui.Rect) {
		pt.AnimationFrameRate(FPS)
		t := float64(pt.Now().UnixMilli()%1200) / 1200
		d := r.W / 4
		for i := range 6 {
			col, row := i%2, i/2
			phase := math.Mod(t-float64(i)/6+1, 1)
			a := float32(0.25 + 0.75*math.Max(0, 1-phase*3))
			x := r.X + d*0.5 + float32(col)*d*2
			y := r.Y + float32(row)*(r.H/3) + (r.H/3-d)/2
			pt.Fill(ui.Rect{X: x, Y: y, W: d, H: d}, colors[row].Alpha(a), d/2)
		}
	})
}

// Orb draws the thinking indicator (ui/thinking in the reference): a dot that
// breathes. Custom drawing for the same reason as Loader.
func Orb(c *ui.Context, size float32) ui.Element {
	p := theme.Of(c)
	return ui.Box(c).Size(size, size).Draw(func(pt *ui.Painter, r ui.Rect) {
		pt.AnimationFrameRate(FPS)
		t := float64(pt.Now().UnixMilli()%1600) / 1600
		s := float32(0.55 + 0.25*math.Sin(t*2*math.Pi))
		w := r.W * s
		pt.Fill(ui.Rect{X: r.X + (r.W-w)/2, Y: r.Y + (r.H-w)/2, W: w, H: w}, p.Primary.Alpha(0.85), w/2)
	})
}

// Secondary is Kit's outline button: a 1 DIP border over the
// button.secondary fill, 32 high, its label 14.
func Secondary(c *ui.Context, label string) ui.Element {
	p := theme.Of(c)
	b := ui.ButtonBase(c).Label(label).Radius(p.Radius).Center().Gap(6).PaddingX(12).Height(32).
		Border(1, p.Border).Background(p.ButtonSecondary).TextColor(p.ButtonSecondaryForeground).Cursor(ui.CursorPointer)
	switch {
	case b.Pressed():
		b.Background(p.ButtonSecondaryActive)
	case b.Hovered():
		b.Background(p.ButtonSecondaryHover)
	}
	return b.Children(func() { ui.Text(c, label).FontSize(14).SingleLine() })
}

// Danger is Kit's danger button, for what removes something.
func Danger(c *ui.Context, label string) ui.Element {
	p := theme.Of(c)
	b := ui.ButtonBase(c).Label(label).Radius(p.Radius).Center().Gap(6).PaddingX(12).Height(32).
		Background(p.Danger).TextColor(p.DangerForeground).Cursor(ui.CursorPointer)
	if b.Hovered() || b.Pressed() {
		b.Background(p.Danger.Mix(p.Foreground, 0.12))
	}
	return b.Children(func() { ui.Text(c, label).FontSize(14).SingleLine() })
}

// Tone colors a badge or a status dot.
type Tone int

const (
	Neutral Tone = iota
	Info
	Positive
	Caution
	Negative
)

func (t Tone) color(p *theme.Palette) ui.Color {
	switch t {
	case Info:
		return p.Primary
	case Positive:
		return p.Success
	case Caution:
		return p.Warning
	case Negative:
		return p.Danger
	}
	return p.MutedForeground
}

// Badge is a Tag in a tone: the tone's color on 12% of it.
func Badge(c *ui.Context, text string, tone Tone) ui.Element {
	col := tone.color(theme.Of(c))
	return ui.Text(c, text).FontSize(12).SingleLine().Padding(1, 6).Radius(999).
		Background(col.Alpha(0.12)).TextColor(col)
}

// StatusDot is an 8 DIP dot in a tone, named by label for assistive
// technology.
func StatusDot(c *ui.Context, tone Tone, label string) ui.Element {
	return ui.Box(c).Size(8, 8).Shrink(0).Radius(4).Background(tone.color(theme.Of(c))).Label(label)
}

// StatusRing is the status dot on a mark: the same 8 DIP dot in a 1 DIP
// ring of the color under it, so it stands apart from the mark.
func StatusRing(c *ui.Context, tone Tone, under ui.Color) ui.Element {
	return ui.Box(c).Size(10, 10).Shrink(0).Radius(5).Background(tone.color(theme.Of(c))).Border(1, under)
}
