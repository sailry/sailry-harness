// The image viewer is adapted from Lobe MyGo's display image viewer
// (github.com/lobehub/lobe-mygo pkg/display/imageviewer.go at 5b3ecde),
// with the ease it uses (pkg/motion/easing.go EaseOut):
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
// Changes: the state is the app's Viewer, which the caller keeps, with the
// reference app's zoom limits and step, wheel rate, fit and pan bounds, its
// keys and its flips; the viewer is MyGo's DialogBase, which closes on
// Escape; the open flight from the thumbnail and the double click zoom left
// out; the backdrop the palette's; the ease inlined.

package kit

import (
	"math"
	"strconv"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Zoom limits and steps of the image viewer.
const (
	MinZoom   = 0.25
	MaxZoom   = 8
	ZoomIn    = 1.25
	ZoomOut   = 0.8
	wheelRate = 0.008
)

// Viewer is the image viewer's state: the gallery it opened on, the picture
// shown, and how it is zoomed, panned, turned and flipped. Its methods are
// the viewer's pure logic; the caller keeps it and ImageViewer draws it.
type Viewer[T any] struct {
	Open   bool
	Images []T
	Index  int
	Zoom   float32
	Turns  int // quarter turns clockwise
	FlipH  bool
	FlipV  bool
	X, Y   float32 // pan in DIPs
	arrive bool    // a picture was just shown: it eases in
}

// Show opens the viewer on list at i, fitted.
func (v *Viewer[T]) Show(list []T, i int) {
	*v = Viewer[T]{Open: true, Images: list, Index: min(max(i, 0), max(len(list)-1, 0)), Zoom: 1, arrive: true}
}

// ZoomBy zooms by factor within MinZoom and MaxZoom; back at the fitted size
// or under, the picture is centered again.
func (v *Viewer[T]) ZoomBy(factor float32) {
	if factor <= 0 || math.IsInf(float64(factor), 0) || math.IsNaN(float64(factor)) {
		return
	}
	v.Zoom = min(MaxZoom, max(MinZoom, v.Zoom*factor))
	if v.Zoom <= 1 {
		v.X, v.Y = 0, 0
	}
}

// Fit shows the picture whole again, unturned, unpanned.
func (v *Viewer[T]) Fit() { v.Zoom, v.Turns, v.FlipH, v.FlipV, v.X, v.Y = 1, 0, false, false, 0, 0 }

// Rotate turns the picture a quarter clockwise.
func (v *Viewer[T]) Rotate() { v.Turns = (v.Turns + 1) % 4 }

// Step shows the next or previous picture, staying at the ends, fitted.
func (v *Viewer[T]) Step(next bool) {
	i := v.Index - 1
	if next {
		i = v.Index + 1
	}
	if i < 0 || i >= len(v.Images) {
		return
	}
	v.Index = i
	v.Fit()
	v.arrive = true
}

// Counter is where the gallery is, "2 / 5"; "" for one picture.
func (v *Viewer[T]) Counter() string {
	if len(v.Images) < 2 {
		return ""
	}
	return strconv.Itoa(v.Index+1) + " / " + strconv.Itoa(len(v.Images))
}

// Pan moves the picture by dx, dy, kept so that a picture rendered at w, h
// larger than the canvas cw, ch never leaves an edge inside it.
func (v *Viewer[T]) Pan(dx, dy, w, h, cw, ch float32) {
	hx, hy := max((w-cw)/2, 0), max((h-ch)/2, 0)
	v.X = min(hx, max(-hx, v.X+dx))
	v.Y = min(hy, max(-hy, v.Y+dy))
}

// Key applies the viewer's key, without modifiers but Shift for +: ← → step,
// + = − zoom, 0 fits. It reports whether the key was one.
func (v *Viewer[T]) Key(mods ui.Modifiers, key ui.Key) bool {
	switch {
	case mods == 0 && key == ui.KeyLeft:
		v.Step(false)
	case mods == 0 && key == ui.KeyRight:
		v.Step(true)
	case (mods == 0 || mods == ui.Shift) && key == ui.KeyEqual:
		v.ZoomBy(ZoomIn)
	case mods == 0 && key == ui.KeyMinus:
		v.ZoomBy(ZoomOut)
	case mods == 0 && key == ui.Key0:
		v.Fit()
	default:
		return false
	}
	return true
}

// FitImage is the size a w by h picture shows at, zoom 1, on a canvas cw by
// ch: at most its own size, 960 by 640 and 82% of the canvas.
func FitImage(w, h, cw, ch float32) (float32, float32) {
	if w <= 0 || h <= 0 {
		return 0, 0
	}
	k := min(1, 960/w, 640/h, cw*0.82/w, ch*0.82/h)
	return w * k, h * k
}

// ImageAction is what the viewer's toolbar asks of the picture shown.
type ImageAction int

const (
	ImageCopy ImageAction = iota
	ImageSave
	ImageOpen
)

// ViewerItem is a picture as the viewer shows it.
type ViewerItem struct {
	Source ui.ImageSource // nil while it loads
	Name   string
}

var (
	viewerSwitch = ui.Animation{Spring: ui.SpringOptions{Stiffness: 304.6, Damping: 34.9, Mass: 1}}
	viewerFade   = ui.Animation{Duration: 150 * time.Millisecond, Ease: ui.CubicBezier(0.215, 0.61, 0.355, 1)}
)

const switchFromScale = 0.96

// ImageViewer draws v over the window while it is open: the picture item
// gives fitted to the window, zoomed by the wheel, a pinch, the keys and
// the toolbar, panned by dragging, turned and flipped; the counter and
// the previous and next arrows for a gallery; Close; and the toolbar
// (zoom out, the zoom, zoom in, fit, rotate, flip, copy, save, open),
// whose Copy, Save and Open call act with the picture. Escape closes it.
func ImageViewer[T any](c *ui.Context, v *Viewer[T], item func(T) ViewerItem, act func(ImageAction, T)) {
	if !v.Open || len(v.Images) == 0 {
		v.Open = false
		return
	}
	p := theme.Of(c)
	v.Index = min(max(v.Index, 0), len(v.Images)-1)
	it := item(v.Images[v.Index])
	w, h := c.Size()
	ui.DialogBase(c, &v.Open, func(back, panel ui.Element) {
		fade := panel.AnimateTo("viewer-fade", 1, viewerFade)
		back.Background(p.Backdrop().Alpha(0.85 * fade))
		panel.Fill().Label(i18n.T(c, "chat_image_preview"))
		for _, k := range []struct {
			mods ui.Modifiers
			key  ui.Key
		}{{0, ui.KeyLeft}, {0, ui.KeyRight}, {0, ui.KeyEqual}, {ui.Shift, ui.KeyEqual}, {0, ui.KeyMinus}, {0, ui.Key0}} {
			if panel.Shortcut(k.mods, k.key) {
				v.Key(k.mods, k.key)
			}
		}
		if v.arrive {
			panel.Jump("viewer-switch", 0)
			v.arrive = false
		}
		arrival := panel.AnimateTo("viewer-switch", 1, viewerSwitch)

		var fw, fh float32 // the fitted, unturned size
		if it.Source != nil {
			iw, ih := ui.ImageSize(it.Source)
			if v.Turns%2 == 1 {
				iw, ih = ih, iw
			}
			fw, fh = FitImage(iw, ih, w, h)
		}
		rw, rh := fw*v.Zoom, fh*v.Zoom // rendered, turned
		stage := ui.Box(c.Key("stage")).Fill().Center().Clip().Cursor(ui.CursorGrab)
		stage.HandleInput(func(ev ui.InputEvent) bool {
			switch ev.Kind {
			case ui.InputScroll:
				v.ZoomBy(float32(math.Exp(float64(-ev.DY * wheelRate))))
			case ui.InputZoom:
				v.ZoomBy(ev.Scale)
			default:
				return false
			}
			return true
		})
		if dx, dy, ok := stage.Dragged(); ok && (dx != 0 || dy != 0) {
			v.Pan(dx, dy, rw, rh, w, h)
		}
		stage.Children(func() {
			if it.Source == nil {
				Skeleton(c, SkeletonProps{Width: 320, Height: 220}).Label(i18n.T(c, "chat_image_loading"))
				return
			}
			bw, bh := fw, fh
			if v.Turns%2 == 1 {
				bw, bh = bh, bw
			}
			k := switchFromScale + (1-switchFromScale)*arrival
			sx, sy := v.Zoom*k, v.Zoom*k
			if v.FlipH {
				sx = -sx
			}
			if v.FlipV {
				sy = -sy
			}
			ui.Image(c.Key(v.Index), it.Source).Size(bw, bh).Fit(ui.Contain).Translate(v.X, v.Y).Scale(sx, sy).
				Rotate(float32(90 * (v.Turns % 4))).Opacity(min(1, arrival*2)).Label(it.Name)
		})
		v.chrome(c, func(a ImageAction) { act(a, v.Images[v.Index]) })
	})
}
