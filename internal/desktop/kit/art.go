package kit

import (
	"math"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Theme artwork. Custom drawing: MyGo's Image fits a picture but neither
// repeats it as tiles nor fades it without a layer (Element.Opacity renders
// one), so pictures are painted with Painter.ImageRegion: cropped for cover,
// placed by the alignment, repeated for tile, and faded by a white tint at
// the decoration's opacity. Every picture is painted from an element's Draw,
// after its background and under its children, and takes no input.

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

// Corners paints the slot's top_left and bottom_right corner art: squares of
// the decoration's height in r's corners, moved out past them by its outset.
func Corners(pt *ui.Painter, r ui.Rect, v *theme.Variant, slot string) {
	if d := v.Asset(slot + ".top_left"); d != nil {
		Picture(pt, d, ui.Rect{X: r.X - d.Outset, Y: r.Y - d.Outset, W: d.Height, H: d.Height})
	}
	if d := v.Asset(slot + ".bottom_right"); d != nil {
		Picture(pt, d, ui.Rect{X: r.X + r.W - d.Height + d.Outset, Y: r.Y + r.H - d.Height + d.Outset, W: d.Height, H: d.Height})
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

// Banner is a slot's full-width strip, as the new session's hero: the
// decoration's height, its effect layers under its picture. It builds nothing
// when the theme has no picture there.
//
// A cover or fill picture is a card: clipped to the theme's large radius and
// edged like the composer. A contain picture is a cutout drawn unclipped over
// its effects, so a halo fades out past the strip. The slot's optional .figure
// stands on the card's bottom edge, placed across it by its horizontal
// alignment, rises above the card where it is taller and drifts by its float;
// the strip reserves that room so it covers nothing above. Over a card the
// effects drift over the whole strip, figure and frame included.
func Banner(c *ui.Context, slot string) {
	p := theme.Of(c)
	v := p.Variant
	d := v.Asset(slot)
	if d == nil {
		return
	}
	layers := effects(c, v, slot)
	figure := v.Asset(slot + ".figure")
	var rise, drift float32
	if figure != nil {
		rise = max(0, figure.Height-d.Height)
		if moving(c) {
			drift = figure.Float
		}
	}
	card := d.Fit == theme.Cover || d.Fit == theme.Fill
	radius, frame := p.Radius2xl(), v.Hero
	strip := ui.Box(c.Key(slot)).FillWidth().Height(d.Height + rise).Shrink(0).PassThrough()
	if card {
		strip.Margin(0, 0, 12, 0) // a card keeps clear of the wordmark under it
	}
	strip.Draw(func(pt *ui.Painter, r ui.Rect) {
		band := ui.Rect{X: r.X, Y: r.Y + rise, W: r.W, H: d.Height}
		if card {
			if frame.Glow.OK {
				pt.Shadow(band, radius, 0, 0, 28, 0, frame.Glow.V)
			}
			pt.Clip(band, radius, func() { Picture(pt, d, band) })
			edge := frame.Border.Or(p.Input)
			if end := frame.BorderGradient; end.OK {
				GradientEdge(pt, band, radius, 1.5, edge, end.V)
			} else {
				pt.Stroke(band, edge, radius, 1)
			}
		} else {
			paintEffects(pt, band, layers)
			Picture(pt, d, band)
		}
		if figure != nil {
			y := band.Y + band.H - figure.Height
			if drift > 0 {
				// One slow breath every six seconds; the bottom edge rises
				// with it, so the waist never shows a gap below the card.
				pt.AnimationFrameRate(FPS)
				phase := float64(pt.Now().UnixMilli()%6000) / 6000
				y -= drift * float32(0.5-0.5*math.Cos(2*math.Pi*phase))
			}
			fr := ui.Rect{X: band.X, Y: y, W: band.W, H: figure.Height}
			// The figure is clipped at the card's bottom, so a rising waist
			// ends at the card's edge.
			pt.Clip(ui.Rect{X: r.X, Y: r.Y - figure.Height, W: r.W, H: band.Y + band.H - r.Y + figure.Height}, 0, func() {
				Picture(pt, figure, fr)
			})
		}
		if card {
			paintEffects(pt, r, layers)
		}
	})
}

// Fill colors a filled button b with a package's action colors in b's
// state, as a left-to-right gradient where they have one, with its glow
// and edge; disabled, it keeps them slightly faded. nil leaves b.
func Fill(b ui.Element, a *theme.Action, disabled bool) {
	if a == nil {
		return
	}
	if disabled {
		b.Opacity(0.85)
	}
	if a.Glow.OK {
		b.Shadow(0, 0, 18, 0, a.Glow.V)
	}
	if a.Border.OK {
		b.Border(1, a.Border.V)
	}
	bg := a.Background.Or(ui.Transparent)
	switch {
	case disabled:
	case b.Pressed():
		bg = a.Active.Or(bg)
	case b.Hovered():
		bg = a.Hover.Or(bg)
	}
	b.Background(bg)
	if a.Gradient.OK {
		b.LinearGradient(ui.LinearGradient{From: bg, To: a.Gradient.V, Angle: 90, Oklab: true})
	}
	if a.Foreground.OK {
		b.TextColor(a.Foreground.V)
	}
}

// Brand is the theme's brand mark, a size square. It builds nothing when the
// theme has none.
func Brand(c *ui.Context, size float32) {
	d := theme.Of(c).Variant.Asset("brand")
	if d == nil || d.Bitmap() == nil {
		return
	}
	ui.Box(c.Key("brand")).Size(size, size).Shrink(0).PassThrough().Draw(func(pt *ui.Painter, r ui.Rect) {
		Picture(pt, d, r)
	})
}

// ArtIcon is the theme's raster icon.{key} at size, or the vector fallback.
// color tints only the fallback.
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
// its top, taking no input. Without an outset it is clipped to the button's
// rounded box; with one it reaches that far past the button's edges.
func NewSessionArt(c *ui.Context, b ui.Element) ui.Element {
	p := theme.Of(c)
	d := p.Variant.Asset("button.new_session")
	if d == nil {
		return b
	}
	radius := p.Radius
	return b.DrawOver(func(pt *ui.Painter, r ui.Rect) {
		strip := ui.Rect{X: r.X, Y: r.Y, W: r.W, H: min(r.H, d.Height)}
		if o := d.Outset; o > 0 {
			Picture(pt, d, ui.Rect{X: strip.X - o, Y: strip.Y - o, W: strip.W + 2*o, H: strip.H + 2*o})
			return
		}
		pt.Clip(r, radius, func() { Picture(pt, d, strip) })
	})
}

// GradientEdge strokes r's rounded edge, width DIPs inside it, from from on
// the left to to on the right. Custom drawing: an element's Border takes one
// color.
func GradientEdge(pt *ui.Painter, r ui.Rect, radius, width float32, from, to ui.Color) {
	h := width / 2
	x, y, w, ht := r.X+h, r.Y+h, r.W-width, r.H-width
	rad := max(0, min(radius-h, w/2, ht/2))
	k := rad * 0.4477 // 1 - the cubic quarter circle's 0.5523
	path := new(ui.Path).MoveTo(x+rad, y).LineTo(x+w-rad, y).CubeTo(x+w-k, y, x+w, y+k, x+w, y+rad).
		LineTo(x+w, y+ht-rad).CubeTo(x+w, y+ht-k, x+w-k, y+ht, x+w-rad, y+ht).
		LineTo(x+rad, y+ht).CubeTo(x+k, y+ht, x, y+ht-k, x, y+ht-rad).
		LineTo(x, y+rad).CubeTo(x, y+k, x+k, y, x+rad, y).Close()
	pt.StrokePathGradient(path, width, ui.LinearGradient{From: from, To: to, Angle: 90, Oklab: true})
}

// InnerGlow is a light inside r's rounded edge, fading inward from it in
// the edge's colors at strength: strokes centered on the edge, each
// narrower and stronger, so the light deepens toward the rim,
// clipped to the box so only their inner halves show. Custom drawing:
// MyGo's shadows fall outside a box only.
func InnerGlow(pt *ui.Painter, r ui.Rect, radius, strength float32, from, to ui.Color) {
	if strength <= 0 {
		return
	}
	pt.Clip(r, radius, func() {
		for _, ring := range []struct{ width, alpha float32 }{{36, 0.05}, {28, 0.06}, {20, 0.08}, {13, 0.11}, {7, 0.16}, {3, 0.3}} {
			h := ring.width / 2
			grown := ui.Rect{X: r.X - h, Y: r.Y - h, W: r.W + ring.width, H: r.H + ring.width}
			a := strength * ring.alpha
			GradientEdge(pt, grown, radius+h, ring.width, from.Alpha(a), to.Alpha(a))
		}
	})
}
