// Image and ImageGroup are adapted from Lobe MyGo's display.Image and
// display.ImageGroup (github.com/lobehub/lobe-mygo pkg/display/image.go
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
// Changes: a click calls OnOpen, and ImageGroup opens the caller's
// Viewer, which the caller draws with ImageViewer once; Fill for a box
// the caller sizes; Radius; the frame and the failed placeholder in the
// palette's muted and border colors; a nil source loads as Kit's
// Skeleton; internal/icons; Lobe's download hook is the viewer's Save.

package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/theme"
)

// ImageVariant is an Image's frame.
type ImageVariant int

const (
	ImageFilled     ImageVariant = iota // the muted fill behind the picture, bordered
	ImageOutlined                       // bordered on the background
	ImageBorderless                     // neither
)

// ImageProps are an Image's.
type ImageProps struct {
	// Width and Height size the box; Size sets both. With none set the
	// picture shows at its own size, at most its parent's width, unless
	// Fill.
	Width, Height, Size float32
	// Fill has the picture fill a box the caller sizes on the element
	// returned.
	Fill bool
	// Contain fits the whole picture inside the box instead of covering it.
	Contain bool
	// Variant picks the frame; Radius rounds it, the palette's large
	// radius when zero.
	Variant ImageVariant
	Radius  float32
	// Loading shows a Skeleton in the box, as a nil source does.
	Loading bool
	// Failed shows the placeholder picture, as when it could not load.
	Failed bool
	// Alt names the picture for assistive technology.
	Alt string
	// OnOpen, set, makes the picture a button that calls it, as to open
	// the viewer.
	OnOpen func()
	// Actions builds buttons at the top right, shown while the pointer is
	// on the picture unless AlwaysShowActions.
	Actions           func()
	AlwaysShowActions bool
}

// Image shows src in a rounded frame: a Skeleton while src is nil or
// Loading, the placeholder once Failed. With OnOpen, a click calls it.
func Image(c *ui.Context, src ui.ImageSource, p ImageProps) ui.Element {
	pal := theme.Of(c)
	w, h := p.Width, p.Height
	if w == 0 {
		w = p.Size
	}
	if h == 0 {
		h = p.Size
	}
	radius := p.Radius
	if radius == 0 {
		radius = pal.RadiusLg
	}
	if (p.Loading || src == nil) && !p.Failed {
		s := Skeleton(c, SkeletonProps{Width: w, Height: h, Radius: radius})
		if p.Fill {
			s.Fill()
		}
		if p.Alt != "" {
			s.Label(p.Alt)
		}
		return s
	}
	open := p.OnOpen != nil && !p.Failed
	var root ui.Element
	if open {
		root = ui.ButtonBase(c).Column().Cursor(ui.CursorPointer).OnClick(p.OnOpen)
	} else {
		root = ui.Box(c)
	}
	root.Radius(radius).Clip().Shrink(0).Unselectable()
	if p.Alt != "" {
		root.Label(p.Alt)
	}
	sized := w > 0 || h > 0 || p.Fill
	if w > 0 {
		root.Width(w)
	}
	if h > 0 {
		root.Height(h)
	}
	if !sized {
		root.MaxWidthPercent(100)
	}
	switch p.Variant {
	case ImageOutlined:
		root.Border(1, pal.Border).Background(pal.Background)
	case ImageBorderless:
	default:
		root.Border(1, pal.Border).Background(pal.Muted)
	}
	root.Children(func() {
		if p.Failed {
			imageFallback(c, pal, w, h, sized)
		} else {
			img := ui.Image(c, src).Fit(ui.Cover)
			if p.Contain {
				img.Fit(ui.Contain)
			}
			if sized {
				img.Fill()
			} else {
				img.MaxWidthPercent(100)
			}
		}
		if p.Actions != nil && (p.AlwaysShowActions || root.Hovered() || root.FocusWithin()) {
			ui.Row(c).Absolute().Top(0).Right(0).ZIndex(1).Children(p.Actions)
		}
	})
	return root
}

// imageFallback is the placeholder of a picture that could not load.
func imageFallback(c *ui.Context, p *theme.Palette, w, h float32, sized bool) {
	box := ui.Row(c).Background(p.Muted).TextColor(p.MutedForeground).Justify(ui.Center).AlignItems(ui.Center)
	if sized {
		box.Fill()
	} else {
		box.Size(128, 128)
	}
	side := min(w, h)
	if side <= 0 {
		side = max(w, h, 128)
	}
	box.Children(func() { ui.Icon(c, icons.Image).FontSize(max(16, side*0.28)) })
}

// ImageGroup builds list's pictures in root, each the Image image gives
// that, clicked, opens v on the whole list at its own index, so the
// viewer steps through them with its counter. The caller draws v with
// ImageViewer once. It returns root.
func ImageGroup[T any](c *ui.Context, root ui.Element, v *Viewer[T], list []T, image func(i int, it T) (ui.ImageSource, ImageProps)) ui.Element {
	return root.Children(func() {
		for i, it := range list {
			src, p := image(i, it)
			p.OnOpen = func() { v.Show(list, i) }
			Image(c.Key(i), src, p)
		}
	})
}
