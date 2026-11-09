// Alert is adapted from Lobe MyGo's feedback.Alert
// (github.com/lobehub/lobe-mygo pkg/feedback/alert.go at 5b3ecde):
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
// Changes: the soft variant only, Sailry's palette and Tone, no close
// button, and an action slot after the text.

package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Alert is an inline message in a tone (MyGo has no alert): the tone's
// icon and the text on the page's background tinted 5% with the tone's
// color inside a 1 DIP ring of it at 14%, and action, if any, after the
// text.
func Alert(c *ui.Context, key any, tone Tone, text string, action func()) ui.Element {
	p := theme.Of(c)
	accent := tone.color(p)
	icon := icons.Info
	switch tone {
	case Positive:
		icon = icons.CircleCheck
	case Caution:
		icon = icons.TriangleAlert
	case Negative:
		icon = icons.CircleX
	}
	return ui.Row(c.Key(key)).FillWidth().Gap(10).AlignItems(ui.Center).Padding(10, 12).Radius(p.Radius).FontSize(14).
		TextColor(p.Foreground).Background(p.Background.Mix(accent, 0.05)).InsetShadow(0, 0, 0, 1, accent.Alpha(0.14)).
		Role(ui.RoleStatus).Children(func() {
		ui.Icon(c, icon).FontSize(16).TextColor(accent).Shrink(0)
		ui.Text(c, text).LineHeight(20.0 / 14).Grow(1).Shrink(1).MinWidth(0)
		if action != nil {
			action()
		}
	})
}
