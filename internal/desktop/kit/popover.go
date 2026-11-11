// Popover is adapted from Lobe MyGo's overlay.Popover
// (github.com/lobehub/lobe-mygo pkg/overlay/popover.go at 5b3ecde):
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
// Changes: the panel through Floating, MyGo Popover's gap, padding and
// minimum width (the anchor's), no placement or toggle of its own: the
// callers attach it and open it as before.

package kit

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// popup is Lobe MyGo's motion.Popup.
var popup = ui.ElementTransition{Duration: 150 * time.Millisecond, Ease: EaseOut, ExitDuration: 75 * time.Millisecond,
	ExitEase: ui.CubicBezier(0.42, 0, 1, 1), Enter: &ui.Motion{Scale: 0.96, Y: -6, Flip: true},
	Exit: &ui.Motion{Scale: 0.96, Y: -6, Flip: true}}

// Popover shows content in a floating panel below anchor while *open is
// true, scaling in from it; pressing outside it or Escape closes it. It
// returns the panel, empty while closed, for the caller to place and
// size.
func Popover(c *ui.Context, anchor ui.Element, open *bool, content func()) ui.Element {
	return ui.PopoverBase(c, anchor, open, func(panel ui.Element) {
		Anchored(panel, anchor, PopoverLayer)
		Floating(c, panel.Margin(4, 0, 0, 0).Padding(4).MinWidth(anchor.Bounds().W)).Transition(popup)
		layer(c, open, content)
	})
}

// Snapped makes a Popover's panel take the place and the size the layout
// gives it at once, a change on its way too, and leave as it does: for a
// page of it that needs its room in the frame it shows, where a panel
// still easing to the page's size would have part of the page past its
// edge, and a press there closes it.
func Snapped(panel ui.Element) ui.Element {
	at := popup
	at.Duration = -1
	return panel.Transition(at)
}
