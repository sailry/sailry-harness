package composer

import (
	"math"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Indicator is a progress ring of the toolbar, as a package declares one
// (plugin ui Indicator; plugins/contributions/controls/indicator.rs): the
// statistics package's context usage, directly left of the model chip.
type Indicator struct {
	ID string
	// Percent is the part of the ring filled, 0 to 100. An unmeasured value
	// is 0: the ring shows its track alone.
	Percent float64
	// Loading sweeps the ring instead of filling it: a compaction runs.
	Loading bool
	// Tone colors the ring: Caution and Negative are the warning and danger
	// colors, any other tone the muted text color.
	Tone kit.Tone
	// Hint is the tooltip and the accessible name, in the package's words.
	Hint string
	// Start puts it among the toolbar's leading controls (align start), not
	// before the model chip.
	Start bool
	// Panel builds the popover of an indicator with a handler, which makes
	// the ring a button: Open says whether it shows, Toggle asks to show or
	// hide it, Disabled keeps it from being asked. Without a Panel the ring
	// takes no clicks.
	Panel    func(c *ui.Context)
	Open     bool
	Toggle   func(open bool)
	Disabled bool
}

// Ring geometry: Kit's ProgressCircle at 20 draws three quarters of it, 15
// across, with a stroke of 15% of that; its wrapper adds 4 on each side.
const (
	ringSize   = 15
	ringStroke = ringSize * 0.15
	ringInset  = 4
)

// indicatorColor is the ring's color for a tone.
func indicatorColor(p *theme.Palette, t kit.Tone) ui.Color {
	switch t {
	case kit.Caution:
		return p.Warning
	case kit.Negative:
		return p.Danger
	}
	return p.MutedForeground
}

// Ring draws a progress ring 15 across: a track at 20% of the color and,
// over it, the arc of percent (0 to 100) clockwise from the top, or while
// loading an arc that sweeps round once a second. It is a picture, not a
// control, named label. Custom drawing: pinned MyGo has a progress bar and a
// spinner of spokes, no progress circle; Kit takes it when it has an owner
// for one (docs/composer.md).
func Ring(c *ui.Context, percent float64, loading bool, color ui.Color, label string) ui.Element {
	still := c.Preferences().ReduceMotion
	return ui.Box(c).Size(ringSize, ringSize).Shrink(0).Label(label).Draw(func(pt *ui.Painter, r ui.Rect) {
		cx, cy, rad := r.X+r.W/2, r.Y+r.H/2, (min(r.W, r.H)-ringStroke)/2
		var track ui.Path
		track.Circle(cx, cy, rad)
		pt.StrokePath(&track, ringStroke, color.Alpha(0.2))
		from, to := 0.0, min(max(percent, 0), 100)/100
		if loading {
			// ProgressCircle's loading: the end eases round in a second, the
			// start follows through its second half.
			from, to = 0, 0.25
			if !still {
				t := float64(pt.Now().UnixMilli()%1000) / 1000
				from, to = ringEase(min(max((t-0.5)/0.5, 0), 1)), ringEase(t)
				pt.AnimationFrameRate(kit.FPS)
			}
		}
		if to <= from {
			return
		}
		var arc ui.Path
		steps := max(2, int(math.Ceil(64*(to-from))))
		for i := 0; i <= steps; i++ {
			a := 2*math.Pi*(from+(to-from)*float64(i)/float64(steps)) - math.Pi/2
			x, y := cx+rad*float32(math.Cos(a)), cy+rad*float32(math.Sin(a))
			if i == 0 {
				arc.MoveTo(x, y)
			} else {
				arc.LineTo(x, y)
			}
		}
		pt.StrokePath(&arc, ringStroke, color)
	})
}

// ringEase is the quadratic ease in and out of the loading sweep.
func ringEase(t float64) float64 {
	if t < 0.5 {
		return 2 * t * t
	}
	return 1 - 2*(1-t)*(1-t)
}

// indicators builds the toolbar's rings on one side, in the order given.
func (s *State) indicators(c *ui.Context, ctx *Context, start bool) {
	for i := range ctx.Indicators {
		if in := &ctx.Indicators[i]; in.Start == start {
			s.indicator(c.Key("indicator/"+in.ID), in)
		}
	}
}

// indicator is one ring: 4 on each side of it and its hint as the tooltip.
// One with a panel is a small round ghost button whose popover opens over
// its end; the others are not pressable.
func (s *State) indicator(c *ui.Context, in *Indicator) {
	color := indicatorColor(theme.Of(c), in.Tone)
	if in.Panel == nil {
		box := ui.Row(c.Key("ring")).Shrink(0).PaddingX(ringInset).AlignItems(ui.Center)
		kit.Tooltip(c, box, in.Hint)
		box.Children(func() { Ring(c, in.Percent, in.Loading, color, in.Hint) })
		return
	}
	open := in.Open
	b := kit.GhostLook(c, ui.ButtonBase(c.Key("ring")), open).Label(in.Hint).Height(24).PaddingX(ringInset).Radius(12).
		Shrink(0).Disabled(in.Disabled)
	if !open {
		kit.Tooltip(c, b, in.Hint)
	}
	b.HasPopup(ui.PopupDialog).Expanded(open).Children(func() { Ring(c, in.Percent, in.Loading, color, "") }).
		OnClick(func() {
			if in.Toggle != nil {
				in.Toggle(!open)
			}
		})
	panel := kit.Popover(c, b, &open, func() { in.Panel(c) })
	if panel != (ui.Element{}) {
		panel.AttachTo(b, ui.AnchorTopEnd, ui.AnchorBottomEnd).Margin(0, 0, 8, 0)
	}
	// Kit closes the popover on an outside click or Escape: the owner hears it.
	if in.Open && !open && in.Toggle != nil {
		in.Toggle(false)
	}
}
