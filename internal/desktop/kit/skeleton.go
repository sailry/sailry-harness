// Skeleton, SkeletonText and SkeletonAvatar are adapted from Lobe MyGo's
// display.Skeleton, display.SkeletonText and display.SkeletonAvatar
// (github.com/lobehub/lobe-mygo pkg/display/skeleton.go at 5b3ecde), with
// the ease they use (pkg/motion/easing.go CSSEase):
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
// Changes: the block in Kit's skeleton color (secondary.active) at the
// palette's radius, with Kit's Secondary at half of it and a Radius of
// its own; the pulse and the sweep painted at FPS through
// Painter.AnimationFrameRate, as Element.Loop runs at the display's rate,
// and still with Reduce Motion; the sweep's band in the hover color; the
// avatar's shape a Circle flag; the ease inlined.

package kit

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// SkeletonAnimation is how a Skeleton moves.
type SkeletonAnimation int

const (
	SkeletonFade SkeletonAnimation = iota
	SkeletonSweep
	SkeletonStatic
)

// SkeletonProps are a Skeleton's.
type SkeletonProps struct {
	// Width is the width, the full width when zero; Height the height, the font size (14) when zero.
	Width, Height float32
	// Radius rounds the block: the palette's radius when zero.
	Radius float32
	// Circle makes the block round.
	Circle bool
	// Secondary draws the block at half the color, as Kit's
	// Skeleton::secondary does for the lesser lines.
	Secondary bool
	// Animation pulses the block's opacity, sweeps a highlight across it, or keeps it still.
	Animation SkeletonAnimation
}

const (
	skeletonFade  = 1600 * time.Millisecond
	skeletonSweep = 1400 * time.Millisecond
)

// Skeleton holds the place of content that is loading. Callers may size
// it further (WidthPercent, Grow); the block fills the box it gets.
func Skeleton(c *ui.Context, p SkeletonProps) ui.Element {
	pal := theme.Of(c)
	color := pal.Active
	if p.Secondary {
		color = color.Alpha(0.5)
	}
	e := ui.Box(c).Shrink(0).Unselectable()
	if p.Width > 0 {
		e.Width(p.Width)
	} else {
		e.FillWidth()
	}
	h := p.Height
	if h == 0 {
		h = 14
	}
	e.Height(h)
	radius := p.Radius
	if radius == 0 {
		radius = pal.Radius
	}
	if p.Circle {
		radius = h / 2
	}
	a := p.Animation
	if c.Preferences().ReduceMotion {
		a = SkeletonStatic
	}
	band := pal.Hover
	return e.Draw(func(pt *ui.Painter, r ui.Rect) {
		switch a {
		case SkeletonFade:
			pt.AnimationFrameRate(FPS)
			t := float32(pt.Now().UnixMilli()%skeletonFade.Milliseconds()) / float32(skeletonFade.Milliseconds())
			pt.Fill(r, color.Alpha(1-0.5*ui.Bounce(ui.EaseInOut)(t)), radius)
		case SkeletonSweep:
			pt.AnimationFrameRate(FPS)
			pt.Fill(r, color, radius)
			t := cssEase(float32(pt.Now().UnixMilli()%skeletonSweep.Milliseconds()) / float32(skeletonSweep.Milliseconds()))
			at := ui.Rect{X: r.X + (2*t-1)*r.W, Y: r.Y, W: r.W, H: r.H}
			pt.Clip(r, radius, func() {
				pt.FillGradient(at, ui.LinearGradient{Angle: 90, Stops: []ui.GradientStop{
					{Color: band.Alpha(0), At: 0}, {Color: band, At: 0.5}, {Color: band.Alpha(0), At: 1},
				}}, 0)
			})
		default:
			pt.Fill(r, color, radius)
		}
	})
}

// SkeletonTextProps are a SkeletonText's.
type SkeletonTextProps struct {
	// Rows is how many lines show: one when zero. The last of several is 66% wide.
	Rows int
	// FontSize sizes the lines as text of that size: 14 when zero.
	FontSize float32
	// Animation pulses, sweeps or keeps the lines still, as Skeleton's does.
	Animation SkeletonAnimation
}

// SkeletonText holds the place of a paragraph that is loading: lines as tall as its text.
func SkeletonText(c *ui.Context, p SkeletonTextProps) ui.Element {
	font := p.FontSize
	if font == 0 {
		font = 14
	}
	rows := max(p.Rows, 1)
	margin := float32(int(font*0.15 + 0.5))
	col := ui.Column(c).FillWidth()
	col.Children(func() {
		for i := range rows {
			line := ui.Column(c.Key(i)).PaddingY(margin)
			if rows > 1 && i == rows-1 {
				line.WidthPercent(66)
			} else {
				line.FillWidth()
			}
			line.Children(func() {
				Skeleton(c, SkeletonProps{Height: float32(int(font*1.3 + 0.5)), Animation: p.Animation})
			})
		}
	})
	return col
}

// SkeletonAvatar holds the place of an avatar: 40 DIPs when size is zero,
// round when circle, else a square of radius size/6.
func SkeletonAvatar(c *ui.Context, size float32, circle bool, a SkeletonAnimation) ui.Element {
	if size == 0 {
		size = 40
	}
	return Skeleton(c, SkeletonProps{Width: size, Height: size, Circle: circle, Radius: max(size/6, 2), Animation: a})
}
