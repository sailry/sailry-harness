package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/shader"
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
	frostShader.Draw(p, box, shader.Options{Radius: radii[0], BackdropBlur: f.blur,
		Uniforms: map[string]any{"Base": premul(f.base), "Tint": premul(f.tint)}})
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
