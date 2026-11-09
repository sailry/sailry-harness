// The image viewer's chrome is adapted from Lobe MyGo's display image
// viewer chrome (github.com/lobehub/lobe-mygo pkg/display/imagechrome.go
// at 5b3ecde):
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
// Changes: the pills are Floating panels and the buttons Kit's IconButton
// with its tooltip; the reference app's toolbar with the zoom between, Copy
// and Open added; the arrows disabled at the ends as the reference app's;
// internal/desktop/icons.

package kit

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// viewerPill is a round Floating panel of the viewer's chrome.
func viewerPill(c *ui.Context, key string) ui.Element {
	return Floating(c, ui.Row(c.Key(key))).Padding(4).Gap(2).Radius(999).AlignItems(ui.Center)
}

// chrome is the viewer's Close at the top right, the gallery's counter
// at the top and arrows at the sides, and the toolbar at the bottom.
func (v *Viewer[T]) chrome(c *ui.Context, act func(ImageAction)) {
	p := theme.Of(c)
	btn := func(key string, icon *ui.SVG, label string, off bool, do func()) {
		IconButton(c.Key(key), icon, i18n.T(c, label), 32, false).Disabled(off).OnClick(do)
	}
	viewerPill(c, "close").Absolute().Top(16).Right(16).Children(func() {
		btn("close", icons.Close, "close", false, func() { v.Open = false })
	})
	if n := v.Counter(); n != "" {
		ui.Row(c.Key("counter")).Absolute().Top(16).Left(0).Right(0).Justify(ui.Center).IgnoreInput(true).Children(func() {
			viewerPill(c, "count").Padding(4, 12).Children(func() {
				ui.Text(c, n).FontSize(12).FontFeatures("tnum").TextColor(p.MutedForeground)
			})
		})
		ui.Column(c.Key("prev")).Absolute().Left(16).Top(0).Bottom(0).Justify(ui.Center).Children(func() {
			viewerPill(c, "pill").Children(func() {
				btn("prev", icons.ChevronLeft, "chat_image_previous", v.Index == 0, func() { v.Step(false) })
			})
		})
		ui.Column(c.Key("next")).Absolute().Right(16).Top(0).Bottom(0).Justify(ui.Center).Children(func() {
			viewerPill(c, "pill").Children(func() {
				btn("next", icons.ChevronRight, "chat_image_next", v.Index == len(v.Images)-1, func() { v.Step(true) })
			})
		})
	}
	ui.Row(c.Key("toolbar")).Absolute().Bottom(24).Left(0).Right(0).Justify(ui.Center).Children(func() {
		viewerPill(c, "bar").Padding(4, 6).Children(func() {
			btn("out", icons.Minus, "chat_image_zoom_out", v.Zoom <= MinZoom, func() { v.ZoomBy(ZoomOut) })
			ui.Text(c, fmt.Sprintf("%d%%", int(v.Zoom*100+0.5))).FontSize(12).FontFeatures("tnum").Width(44).
				TextAlign(ui.Center).TextColor(p.MutedForeground)
			btn("in", icons.Plus, "chat_image_zoom_in", v.Zoom >= MaxZoom, func() { v.ZoomBy(ZoomIn) })
			btn("fit", icons.Minimize, "chat_image_fit", false, v.Fit)
			btn("rotate", icons.RotateCw, "chat_image_rotate", false, v.Rotate)
			btn("flip-h", icons.FlipHorizontal, "chat_image_flip_horizontal", false, func() { v.FlipH = !v.FlipH })
			btn("flip-v", icons.FlipVertical, "chat_image_flip_vertical", false, func() { v.FlipV = !v.FlipV })
			btn("copy", icons.Copy, "content_copy", false, func() { act(ImageCopy) })
			btn("save", icons.Save, "files_download", false, func() { act(ImageSave) })
			btn("open", icons.ExternalLink, "files_open_system", false, func() { act(ImageOpen) })
		})
	})
}
