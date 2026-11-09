// Segmented is adapted from Lobe MyGo's action.Segmented
// (github.com/lobehub/lobe-mygo pkg/action/segmented.go at 5b3ecde):
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
// Changes: Sailry's palette (the track on the muted surface, without the
// source's faint border, which has no slot here; the active segment
// inverted, the foreground under the background's color), the filled
// variant only, sizes whose default matches a text input's height, and
// the ease inlined.

package kit

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Size is a control's size.
type Size int

const (
	SizeDefault Size = iota
	SizeSmall
	SizeLarge
)

// SegmentedProps are a Segmented's options.
type SegmentedProps struct {
	// Size sets the segments' padding and font: the default is as high as
	// a text input or a Select.
	Size Size
	// Block stretches the track across its parent and shares the width
	// between the segments.
	Block bool
}

// EaseOut is Lobe MyGo's motion.EaseOut.
var EaseOut = ui.CubicBezier(0.215, 0.61, 0.355, 1)

type segmentedMatch struct{ selected *int }

var segmentPress = ui.Animation{Duration: 120 * time.Millisecond, Ease: EaseOut}

// Segmented is a choice among a few labels on MyGo's SegmentedBase, which
// keeps the tab semantics and the keyboard: a track on the muted surface
// with the chosen segment's indicator sliding to it, inverted (the
// foreground behind the background's color), the others' labels muted
// until hovered, a press scaling a segment down. Its Changed reports a
// new choice; Label names it.
func Segmented(c *ui.Context, key any, selected *int, options []string, o SegmentedProps) ui.Element {
	p := theme.Of(c)
	// The track pads 3 around segments padded py: with the 14 DIP font,
	// the default is a text input's height (MyGo's: 6 DIP padding and a
	// 1 DIP border around the line).
	py, px, font := float32(4), float32(12), float32(14)
	switch o.Size {
	case SizeSmall:
		py, px, font = 2, 10, 12
	case SizeLarge:
		py, px = 6, 16
	}
	parts := ui.SegmentedBase(c.Key(key), selected, len(options))
	track := parts.Track.Padding(3).Gap(4).Radius(p.RadiusLg).Background(p.Muted)
	if o.Block {
		track.AlignSelf(ui.Stretch)
	}
	slide := ui.ElementTransition{Duration: 240 * time.Millisecond, Ease: EaseOut, Match: segmentedMatch{selected}}
	track.Children(func() {
		for i, option := range options {
			seg := parts.Segment(i).PaddingY(py).PaddingX(px).Radius(p.Radius).FontSize(font).FontWeight(500).
				Cursor(ui.CursorPointer).Center()
			if o.Block {
				seg.Grow(1).Basis(0)
			}
			text := p.MutedForeground
			switch {
			case i == *selected:
				text = p.Background
			case seg.Hovered():
				text = p.Foreground
			}
			pose := ui.Pose{}
			if seg.Pressed() {
				pose.Scale = 0.98
			}
			seg.TextColor(text).AnimatePose(pose, segmentPress)
			seg.Children(func() {
				if i == *selected {
					ui.Box(c).Absolute().Top(0).Left(0).Right(0).Bottom(0).
						Radius(p.Radius).Background(p.Foreground).ZIndex(-1).Transition(slide)
				}
				ui.Text(c, option).SingleLine()
			})
		}
	})
	return track
}
