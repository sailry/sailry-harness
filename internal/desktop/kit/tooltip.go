// Tooltip and Placement are adapted from Lobe MyGo's overlay.Tooltip and
// overlay placement (github.com/lobehub/lobe-mygo pkg/overlay/tooltip.go
// and pkg/overlay/placement.go at 5b3ecde), with the motion presets they
// use (pkg/motion/transition.go Tooltip and Toward, pkg/motion/spring.go):
//
// MIT License
//
// Copyright (c) 2026 LobeHub
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
// TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
// SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
//
// Changes: the tip through Floating at the palette's radius, MyGo's
// tooltip padding, width and font size, a muted shortcut text in place of
// display.Hotkey's key cap, and the motion inlined.

package kit

import (
	"math"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Placement is the side of its anchor a tip or hover card shows on. Where
// there is no room there, it shows on the opposite side, its motion
// mirrored.
type Placement int

const (
	Top Placement = iota
	Bottom
	Left
	Right
)

// TooltipProps set a Tooltip's side and an optional shortcut.
type TooltipProps struct {
	// Placement is the side of the anchor the tip shows on.
	Placement Placement
	// Hotkey shows the keys that do the same after the text, muted, as
	// KeyText writes them.
	Hotkey string
}

// tipContent fades a tip's content in as it shows or its text changes.
var tipContent = ui.ElementTransition{Duration: 120 * time.Millisecond, Ease: EaseOut, Enter: &ui.Motion{}}

// tipSpring is Lobe MyGo's motion.Bouncy(280 ms): damping ratio 0.7,
// settled within 0.003 at 280 ms.
var tipSpring = spring(0.7, 0.003, 280*time.Millisecond)

// TipMotion is Lobe MyGo's motion.Tooltip, turned so the tip or card
// starts at its anchor from side p: a spring scale and a 3 DIP slide in,
// a 100 ms fade out. One tip shows at a time, so moving to another anchor
// rebuilds it there rather than animating its box.
func TipMotion(p Placement) ui.ElementTransition {
	return toward(ui.ElementTransition{
		Animation:    &tipSpring,
		ExitDuration: 100 * time.Millisecond,
		ExitEase:     ui.CubicBezier(0.4, 0, 1, 1),
		Enter:        &ui.Motion{Scale: 0.97, Y: -3, Flip: true},
		Exit:         &ui.Motion{Scale: 0.97, Y: -3, Flip: true},
		Colors:       true,
	}, p)
}

// Attach places tip on side p of anchor, gap apart, scaling from the
// anchor, with TipMotion.
func Attach(tip, anchor ui.Element, p Placement, gap float32) ui.Element {
	switch p {
	case Bottom:
		tip.AttachTo(anchor, ui.AnchorBottom, ui.AnchorTop).Margin(gap, 0, 0, 0)
	case Left:
		tip.AttachTo(anchor, ui.AnchorLeft, ui.AnchorRight).Margin(0, gap, 0, 0)
	case Right:
		tip.AttachTo(anchor, ui.AnchorRight, ui.AnchorLeft).Margin(0, 0, 0, gap)
	default:
		tip.AttachTo(anchor, ui.AnchorTop, ui.AnchorBottom).Margin(0, 0, gap, 0)
	}
	return tip.AttachedOrigin().Transition(TipMotion(p))
}

// Tooltip shows text by anchor once the pointer rests on it, or the
// keyboard focus comes to it (MyGo's TooltipBase and its delay), on a
// Floating panel, and describes anchor to assistive technology. It
// returns the tip, empty while it does not show.
func Tooltip(c *ui.Context, anchor ui.Element, text string, props ...TooltipProps) ui.Element {
	if text == "" {
		return ui.Element{}
	}
	var o TooltipProps
	if len(props) > 0 {
		o = props[0]
	}
	anchor.Description(text)
	if TipsHidden(c) {
		return ui.Element{}
	}
	p, t := theme.Of(c), c.Theme()
	return ui.TooltipBase(c, anchor, func(tip ui.Element) {
		Anchored(tip, anchor, TipLayer)
		Attach(Floating(c, tip), anchor, o.Placement, t.Space(1.5))
		tip.Radius(p.Radius).MaxWidth(t.Space(80)).Padding(t.Space(1.25), t.Space(2)).FontSize(t.FontSize - 1)
		tip.Children(func() {
			ui.Row(c.Key(text)).Gap(6).Transition(tipContent).Children(func() {
				ui.Text(c, text)
				if o.Hotkey != "" {
					ui.Text(c, o.Hotkey).SingleLine().TextColor(p.MutedForeground)
				}
			})
		})
	})
}

// toward turns the offsets of a motion made for an element below its
// anchor so it starts at the anchor from side p (Lobe MyGo's
// motion.Toward).
func toward(tr ui.ElementTransition, p Placement) ui.ElementTransition {
	turn := func(m *ui.Motion) *ui.Motion {
		if m == nil {
			return nil
		}
		t := *m
		switch p {
		case Top:
			t.Y = -m.Y
		case Right:
			t.X, t.Y = m.Y, 0
		case Left:
			t.X, t.Y = -m.Y, 0
		}
		return &t
	}
	tr.Enter, tr.Exit = turn(tr.Enter), turn(tr.Exit)
	return tr
}

// spring is Lobe MyGo's motion.Spring: a spring of damping ratio zeta
// that settles within threshold of its target at settle.
func spring(zeta, threshold float64, settle time.Duration) ui.Animation {
	const step, horizon = 0.0005, 60.0
	last := 0.0
	for t := 0.0; t < horizon; t += step {
		if math.Abs(stepResponse(zeta, t)-1) > threshold {
			last = t
		}
	}
	omega := last / settle.Seconds()
	return ui.Animation{Spring: ui.SpringOptions{Stiffness: omega * omega, Damping: 2 * zeta * omega, Mass: 1}}
}

func stepResponse(zeta, t float64) float64 {
	if zeta >= 1 {
		return 1 - math.Exp(-t)*(1+t)
	}
	wd := math.Sqrt(1 - zeta*zeta)
	return 1 - math.Exp(-zeta*t)*(math.Cos(wd*t)+zeta/wd*math.Sin(wd*t))
}
