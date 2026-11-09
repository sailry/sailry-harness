package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/shader"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// frostShader is a backdrop blur that ends opaque: the blurred backdrop
// over Base, then Tint over that, then a dither of half an 8-bit step.
// glass.Blur returns the backdrop's own alpha, so over our translucent
// (vibrant) window the sharp pixels under the panel show through what it
// draws; compositing over an opaque Base covers them. The dither breaks
// the bands a wide blur of a dark backdrop leaves in 8 bits.
var frostShader = shader.MustCompile(`package main

var Base vec4
var Tint vec4

func Fragment(dst vec4, pos vec2) vec4 {
	c := backdropSample(pos)
	c = c + Base*(1-c.a)
	c = Tint + c*(1-Tint.a)
	n := fract(sin(dot(pos, vec2(12.9898, 78.233))) * 43758.5453)
	c.rgb = c.rgb + (n-0.5)/255
	return c
}
`)

// frost is the floating panels' material: a backdrop blur of blur DIP
// over the opaque base, toned by tint.
type frost struct {
	blur       float32
	base, tint ui.Color
}

// PaintMaterial paints the frost over box, rounded by radii.
func (f frost) PaintMaterial(p *ui.Painter, box ui.Rect, radii [4]float32) {
	draw := func(r ui.Rect) {
		frostShader.Draw(p, r, shader.Options{Radius: radii[0], BackdropBlur: f.blur,
			Uniforms: map[string]any{"Base": premul(f.base), "Tint": premul(f.tint)}})
	}
	// The shader rounds every corner alike: square bottom corners (a tray
	// sitting on the composer) draw it taller and clip it to the box.
	if radii[0] > 0 && radii[2] == 0 && radii[3] == 0 {
		p.Clip(box, 0, func() { draw(ui.Rect{X: box.X, Y: box.Y, W: box.W, H: box.H + radii[0]}) })
		return
	}
	draw(box)
}

// premul is c as premultiplied RGBA in 0..1, as the shader takes colors.
func premul(c ui.Color) [4]float32 {
	a := float32(c.A) / 255
	return [4]float32{float32(c.R) / 255 * a, float32(c.G) / 255 * a, float32(c.B) / 255 * a, a}
}

// Opaque is c at full alpha. ui.Color.Alpha multiplies the alpha, so
// Alpha(1) keeps a translucent color translucent.
func Opaque(c ui.Color) ui.Color {
	c.A = 255
	return c
}

// Frost gives el the floating panels' backdrop blur toned by base instead
// of a flat background: the composer's surface and its queue tray and
// the surface cards (settings cards, resource and market cards, lists),
// so what is under them, and a theme's backdrop, shows through blurred.
// A translucent base (a theme's muted fill) is taken as it shows over the
// window background, so the theme sets only the color.
// tint is how strongly the color covers the blurred backdrop (SurfaceTint,
// CardTint): the app sets it, never a theme.
func Frost(c *ui.Context, el ui.Element, base ui.Color, tint float32) ui.Element {
	solid := base.Over(Opaque(theme.Of(c).Background))
	return el.Material(frost{blur: FloatingBlur, base: solid, tint: solid.Alpha(tint)})
}

const (
	// SurfaceTint is the composer's and its queue tray's: they keep their
	// fill's tone and the page under them only shows faintly.
	SurfaceTint = 0.6
	// CardTint is the surface cards' (settings, resource, market and panel
	// cards, lists): the theme's backdrop shows through them clearly.
	CardTint = 0.45
)

// Veil is a translucent fill of base at SurfaceTint, without the frost's
// blur: for the transcript's bubbles, many and scrolling, where a backdrop
// blur each would cost a frame. base is taken as it shows over the window
// background, so a theme sets only the color.
func Veil(c *ui.Context, base ui.Color) ui.Color {
	return base.Over(Opaque(theme.Of(c).Background)).Alpha(SurfaceTint)
}

// Pane fills a window column (the sidebar, the main page, the right
// panel) with fill. Over a theme's window art (frosted) the art shows
// through blurred, at fill's alpha (the Appearance page's container
// opacity); otherwise fill is a plain background, so the window material
// of a translucent window still shows through.
// blur is the radius in DIP (Appearance's blur slider); 0 leaves the art
// sharp under the translucent fill.
func Pane(c *ui.Context, el ui.Element, fill ui.Color, frosted bool, blur float32) ui.Element {
	if !frosted || blur <= 0 {
		return el.Background(fill)
	}
	solid := Opaque(fill).Over(Opaque(theme.Of(c).Background))
	return el.Material(frost{blur: blur, base: solid, tint: solid.Alpha(float32(fill.A) / 255)})
}
