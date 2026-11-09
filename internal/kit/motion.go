// Package kit holds Sailry's shared components: the look of the reference
// app's Kit, built on MyGo's widgets, with every color and size from
// internal/theme. Pages compose them and add only their layout (docs/kit.md).
package kit

import (
	"hash/fnv"
	"math"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/theme"
)

// FPS caps drawings that animate. MyGo's only frame-rate parameter is
// Painter.AnimationFrameRate, so looping indicators are drawn with it rather
// than built from Element.Loop, which runs at the display's rate.
const FPS = theme.AnimationFPS

// Fast is Kit's motion duration for small transitions (the jump button's
// fade, popovers): 200 ms.
const Fast = 200 * time.Millisecond

// EaseOutCubic is Kit's ease_out_cubic: 1 - (1-t)^3.
func EaseOutCubic(t float32) float32 {
	u := 1 - t
	return 1 - u*u*u
}

// Shimmer is a label with a highlight sweeping across it every 2 s, as Kit's
// ShimmerText marks the running activity: 12 clipped layers of a highlight
// 0.3 of the width wide, peaking at 0.75 (light) or 0.6 (dark) opacity.
// Custom drawing: MyGo has no text highlight that moves; the layers are drawn
// over the label at 60 fps and stop with Reduce Motion.
func Shimmer(c *ui.Context, text string, size float32, color ui.Color) ui.Element {
	p := theme.Of(c)
	t := ui.Text(c, text).FontSize(size).TextColor(color).SingleLine()
	if c.Preferences().ReduceMotion {
		return t
	}
	peak, hi := float32(0.75), p.Background.MixOklab(color, 0.2)
	if p.Dark {
		peak, hi = 0.6, p.Foreground.MixOklab(color, 0.2)
	}
	const layers = 12
	return t.DrawOver(func(pt *ui.Painter, r ui.Rect) {
		pt.AnimationFrameRate(FPS)
		phase := float32(pt.Now().UnixMilli()%2000) / 2000
		spread := 0.3 * r.W
		center := r.X - spread + phase*(r.W+2*spread)
		span := ui.Span{Text: text, Size: size, Color: hi.Alpha(peak / layers)}
		pt.Clip(r, 0, func() {
			for i := range layers {
				half := spread / 2 * (1 - float32(i)/layers)
				pt.Clip(ui.Rect{X: center - half, Y: r.Y, W: 2 * half, H: r.H}, 0, func() {
					pt.RichText(r.X, r.Y, 0, span)
				})
			}
		})
	})
}

// Identicon is a subagent's 16 DIP mark (ui/identicon.rs): a ring of four
// segments in a color picked from its ID, turning while it runs (a 450 ms
// turn each second). Custom drawing: MyGo's Avatar draws initials, not
// procedural marks.
func Identicon(c *ui.Context, id string, running bool, size float32) ui.Element {
	p := theme.Of(c)
	h := fnv.New32a()
	h.Write([]byte(id))
	color := p.MutedForeground
	if running {
		color = identiconColors[h.Sum32()%uint32(len(identiconColors))].pick(p.Dark)
	}
	seed := h.Sum32()
	return ui.Box(c).Size(size, size).Shrink(0).Draw(func(pt *ui.Painter, r ui.Rect) {
		angle := float64(0)
		if running {
			pt.AnimationFrameRate(FPS)
			ms := float64(pt.Now().UnixMilli() % 1000)
			t := math.Min(ms/450, 1)
			angle = (t * t * (3 - 2*t)) * math.Pi / 2 // smoothstep, then rest
		}
		cx, cy, rad := r.X+r.W/2, r.Y+r.H/2, r.W*0.36
		for i := range 4 {
			a := angle + float64(i)*math.Pi/2
			x := cx + rad*float32(math.Cos(a))
			y := cy + rad*float32(math.Sin(a))
			d := r.W * (0.22 + 0.08*float32((seed>>(i*3))&3)/3)
			pt.Fill(ui.Rect{X: x - d/2, Y: y - d/2, W: d, H: d}, color.Alpha(0.55+0.15*float32(i)), d/2)
		}
	})
}

type pair [2]string

func (q pair) pick(dark bool) ui.Color {
	if dark {
		return ui.Hex(q[0])
	}
	return ui.Hex(q[1])
}

// identiconColors are Tailwind's 400 (dark) and 600 (light) shades.
var identiconColors = []pair{
	{"#60a5fa", "#2563eb"}, {"#a78bfa", "#7c3aed"}, {"#34d399", "#059669"},
	{"#fbbf24", "#d97706"}, {"#f472b6", "#db2777"}, {"#22d3ee", "#0891b2"},
}
