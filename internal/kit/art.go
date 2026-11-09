package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/theme"
)

// Theme artwork (theme/decoration.rs). Custom drawing: MyGo's Image fits a
// picture but neither repeats it as tiles nor fades it without a layer
// (Element.Opacity renders one), so pictures are painted with
// Painter.ImageRegion: cropped for cover, placed by the alignment, repeated
// for tile, and faded by a white tint at the decoration's opacity. Every
// picture is painted from an element's Draw, after its background and
// under its children, and takes no input.

// Picture paints a decoration into r.
func Picture(pt *ui.Painter, d *theme.Decoration, r ui.Rect) {
	b := d.Bitmap()
	if b == nil || r.W <= 0 || r.H <= 0 {
		return
	}
	iw, ih := b.Size()
	w, h := float32(iw), float32(ih)
	if w == 0 || h == 0 {
		return
	}
	o := ui.ImageOptions{}
	if d.Opacity < 1 {
		o.Tint = theme.Opacity(d.Opacity)
	}
	ax, ay := d.Alignment[0], d.Alignment[1]
	switch d.Fit {
	case theme.Fill:
		pt.ImageRegion(b, ui.Rect{}, r, o)
	case theme.Tile:
		tw := d.TileSize
		th := tw * h / w
		if tw <= 0 || th <= 0 {
			return
		}
		pt.Clip(r, 0, func() {
			for y := r.Y; y < r.Y+r.H; y += th {
				for x := r.X; x < r.X+r.W; x += tw {
					pt.ImageRegion(b, ui.Rect{}, ui.Rect{X: x, Y: y, W: tw, H: th}, o)
				}
			}
		})
	case theme.Contain:
		s := min(r.W/w, r.H/h)
		dw, dh := w*s, h*s
		pt.ImageRegion(b, ui.Rect{}, ui.Rect{X: r.X + (r.W-dw)*ax, Y: r.Y + (r.H-dh)*ay, W: dw, H: dh}, o)
	default: // cover: the part of the picture r shows, placed by the alignment
		s := max(r.W/w, r.H/h)
		sw, sh := r.W/s, r.H/s
		pt.ImageRegion(b, ui.Rect{X: (w - sw) * ax, Y: (h - sh) * ay, W: sw, H: sh}, r, o)
	}
}

// Corners paints the slot's top_left and bottom_right corner art: squares
// of the decoration's height in r's corners (decoration.rs corner).
func Corners(pt *ui.Painter, r ui.Rect, v *theme.Variant, slot string) {
	if d := v.Asset(slot + ".top_left"); d != nil {
		Picture(pt, d, ui.Rect{X: r.X, Y: r.Y, W: d.Height, H: d.Height})
	}
	if d := v.Asset(slot + ".bottom_right"); d != nil {
		Picture(pt, d, ui.Rect{X: r.X + r.W - d.Height, Y: r.Y + r.H - d.Height, W: d.Height, H: d.Height})
	}
}

// Backdrop returns what paints a background slot (background.window,
// background.content, background.sidebar, background.card.*): its picture
// over the box, then its effect layers. It is nil when the theme has
// neither, so the element keeps no Draw.
func Backdrop(c *ui.Context, slot string) func(*ui.Painter, ui.Rect) {
	v := theme.Of(c).Variant
	d, layers := v.Asset(slot), effects(c, v, slot)
	if d == nil && layers == nil {
		return nil
	}
	return func(pt *ui.Painter, r ui.Rect) {
		pt.Clip(r, 0, func() {
			if d != nil {
				Picture(pt, d, r)
			}
			paintEffects(pt, r, layers)
		})
	}
}

// Draws adds a painter to e when there is one.
func Draws(e ui.Element, fn func(*ui.Painter, ui.Rect)) ui.Element {
	if fn != nil {
		e.Draw(fn)
	}
	return e
}

// Banner is a slot's full-width strip (decoration.rs banner), as the new
// session's hero: the decoration's height, its effect layers under its
// picture. It builds nothing when the theme has no picture there.
func Banner(c *ui.Context, slot string) {
	v := theme.Of(c).Variant
	d := v.Asset(slot)
	if d == nil {
		return
	}
	layers := effects(c, v, slot)
	ui.Box(c.Key(slot)).FillWidth().Height(d.Height).Shrink(0).PassThrough().Draw(func(pt *ui.Painter, r ui.Rect) {
		pt.Clip(r, 0, func() {
			paintEffects(pt, r, layers)
			Picture(pt, d, r)
		})
	})
}

// ArtIcon is the theme's raster icon.{key} at size, or the vector fallback
// (decoration.rs sized_icon). color tints only the fallback.
func ArtIcon(c *ui.Context, key string, fallback *ui.SVG, size float32, color ui.Color) ui.Element {
	if d := theme.Of(c).Variant.Asset("icon." + key); d != nil && d.Bitmap() != nil {
		return ui.Box(c).Size(size, size).Shrink(0).PassThrough().Draw(func(pt *ui.Painter, r ui.Rect) {
			Picture(pt, d, r)
		})
	}
	if fallback == nil {
		return ui.Box(c).Size(size, size).Shrink(0)
	}
	return ui.Icon(c, fallback).FontSize(size).TextColor(color)
}

// HasArtIcon reports whether the theme draws icon.{key}.
func HasArtIcon(c *ui.Context, key string) bool {
	return theme.Of(c).Variant.Asset("icon."+key) != nil
}

// NewSessionArt lays the theme's button.new_session ornament over the
// primary button it is called on: a strip of the decoration's height from
// its top, rounded to the theme radius, taking no input.
func NewSessionArt(c *ui.Context, b ui.Element) ui.Element {
	p := theme.Of(c)
	d := p.Variant.Asset("button.new_session")
	if d == nil {
		return b
	}
	radius := p.Radius
	return b.DrawOver(func(pt *ui.Painter, r ui.Rect) {
		pt.Clip(r, radius, func() {
			Picture(pt, d, ui.Rect{X: r.X, Y: r.Y, W: r.W, H: min(r.H, d.Height)})
		})
	})
}
