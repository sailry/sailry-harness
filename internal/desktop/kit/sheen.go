package kit

import (
	"math"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/shader"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Custom drawing: a sheen lights only parts of the hero (its staff, the
// gold of a garment), which no Painter fill can follow, so it is a shader
// reading the hero.new_session.sheen mask as image0. Src is the part of the
// mask shown, in fractions of its size, as the hero's own picture is placed.
var sheenShader = shader.MustCompile(`package main

var Color vec4
var Src vec4
var Center float
var Width float
var Glow float

func Fragment(dst vec4, pos vec2) vec4 {
	uv := pos / rectSize()
	m := image0Sample((Src.xy + uv*Src.zw) * image0Size()).a
	t := uv.x*0.75 + uv.y*0.25
	band := 1 - smoothstep(0, Width, abs(t-Center))
	return Color * (m * min(band+Glow, 1))
}
`)

// sheenSweep is the part of a sheen's cycle its band takes to cross the
// hero; the rest it rests, only the glow breathing.
const sheenSweep = 0.6

// paintSheen paints the sheen layers among layers over the hero placed in
// r, through mask. Each sweeps a diagonal band size × 2% of the hero wide
// once a cycle over a glow breathing with it.
func paintSheen(pt *ui.Painter, mask *theme.Decoration, r ui.Rect, layers []theme.Effect) {
	b := mask.Bitmap()
	if b == nil || r.W <= 0 || r.H <= 0 {
		return
	}
	iw, ih := b.Size()
	w, h := float32(iw), float32(ih)
	if w == 0 || h == 0 {
		return
	}
	src, dst := placed(mask, w, h, r)
	if src.W == 0 {
		src = ui.Rect{W: w, H: h}
	}
	ms := pt.Now().UnixMilli()
	for _, e := range layers {
		if e.Kind != theme.Sheen {
			continue
		}
		pt.AnimationFrameRate(FPS)
		period := int64(e.Duration * 1000)
		phase := float32(ms%period) / float32(period)
		width := e.Size * 0.02
		sweep := min(phase/sheenSweep, 1)
		breath := 0.5 - 0.5*float32(math.Cos(float64(phase)*2*math.Pi))
		sheenShader.Draw(pt, dst, shader.Options{
			Images: [4]*ui.Bitmap{b},
			Uniforms: map[string]any{
				"Color":  premul(e.Color.V.Alpha(e.Opacity * mask.Opacity)),
				"Src":    [4]float32{src.X / w, src.Y / h, src.W / w, src.H / h},
				"Center": -width + sweep*(1+2*width),
				"Width":  width,
				"Glow":   0.12 + 0.18*breath,
			},
		})
	}
}
