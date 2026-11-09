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
	p := theme.Of(c)
	b := ui.ButtonBase(c).Label(label).Radius(6).Center().TextColor(p.Foreground).Cursor(ui.CursorPointer)
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
	b := Ghost(c, label, selected).Size(size, size).Tooltip(label)
	glyph := float32(16)
	if size < 24 {
		glyph = 14
	}
	return b.Children(func() { ui.Icon(c, icon).FontSize(glyph) })
}

// Primary is Kit's primary button: the neutral foreground fill.
func Primary(c *ui.Context, label string, disabled bool) ui.Element {
	p := theme.Of(c)
	b := ui.ButtonBase(c).Label(label).Disabled(disabled).DimDisabled(false).Radius(6).Center().Gap(6).
		PaddingX(12).Height(32).TextColor(p.OnPrimary).Background(p.ButtonPrimary)
	switch {
	case disabled && !p.Dark:
		b.Background(p.Active).TextColor(p.MutedForeground.Alpha(0.5))
	case disabled:
		b.Background(p.ButtonPrimary.Alpha(0.15)).TextColor(p.MutedForeground.Alpha(0.5))
	case b.Hovered() || b.Pressed():
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
	colors := [3]ui.Color{ui.Hex("#3b82f6"), p.Warning, p.Danger}
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
