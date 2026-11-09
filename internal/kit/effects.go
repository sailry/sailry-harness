package kit

import (
	"math"
	"sync/atomic"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/theme"
)

// Theme effects (theme/effects.rs, effects/motion.rs): layers of
// procedural particles a package puts on a surface (background.window,
// background.content, background.sidebar, hero.new_session, conversation).
//
// Custom drawing: MyGo has no particle layer, so the particles are painted
// from the surface's Draw, under its children and taking no input. Their
// motion is the reference's: each particle's place, size and phase come
// from a hash of its index and the layer's seed, so a layer looks the same
// on every run, and a cycle of the layer's duration loops without a jump.
// They move at the FPS cap through Painter.AnimationFrameRate, the only
// frame-rate parameter MyGo has, which repaints without building the view.
// They are off with Reduce Motion or the Appearance switch, and a hidden
// or occluded window paints no frames, so they stop with it.

var effectsOff atomic.Bool

// SetEffects turns the theme's effects on or off (Settings › Appearance,
// the reference's theme_effects preference).
func SetEffects(on bool) { effectsOff.Store(!on) }

// effects returns the theme's visible effect layers of a surface, or nil
// when effects are off.
func effects(c *ui.Context, v *theme.Variant, surface string) []theme.Effect {
	if v == nil || effectsOff.Load() || c.Preferences().ReduceMotion || c.WindowHidden() {
		return nil
	}
	var out []theme.Effect
	for _, e := range v.Layers(surface) {
		if e.Opacity > 0 && e.Count > 0 && e.Duration > 0 && e.Color.OK {
			out = append(out, e)
		}
	}
	return out
}

// paintEffects paints effect layers into r at the current frame's time,
// and asks for the next frame.
func paintEffects(pt *ui.Painter, r ui.Rect, layers []theme.Effect) {
	if len(layers) == 0 || r.W <= 0 || r.H <= 0 {
		return
	}
	pt.AnimationFrameRate(FPS)
	ms := pt.Now().UnixMilli()
	for _, e := range layers {
		period := int64(e.Duration * 1000)
		phase := float32(ms%period) / float32(period)
		particles(pt, r, e, phase)
	}
}

// particles paints one layer at phase, from 0 to 1 over its duration.
func particles(pt *ui.Painter, r ui.Rect, e theme.Effect, phase float32) {
	for i := range uint32(e.Count) {
		s := sample(i, e.Seed, phase)
		extent := e.Size * s.scale
		var x, y, alpha float32
		switch e.Kind {
		case theme.Twinkle:
			x, y, alpha = s.x, s.y, e.Opacity*s.pulse
		case theme.Embers:
			x, y, alpha = s.x+s.sway, 1-s.time, e.Opacity*s.fade
		case theme.Dust:
			x, y, alpha = s.x+s.sway, s.time, e.Opacity*s.fade*0.65
		case theme.Petals:
			x, y, alpha = s.x+s.sway*3, s.time, e.Opacity*s.fade
		case theme.Pulse:
			x, y, alpha = 0.5, 0.48, e.Opacity*(0.5+0.5*sin(phase*2*math.Pi))
		case theme.Scan:
			x, y, alpha = 0.5, s.time, e.Opacity*s.fade
		default:
			return
		}
		cx, cy := r.X+r.W*x, r.Y+r.H*y
		ink := e.Color.V.Alpha(alpha)
		switch e.Kind {
		case theme.Pulse:
			// A soft halo 24% wide and 20% high, blurred by a quarter of
			// the height, as a radial fade.
			rad := max(r.W*0.12, r.H*0.10) + r.H*0.25
			pt.FillRadialGradient(ui.Rect{X: cx - rad, Y: cy - rad, W: 2 * rad, H: 2 * rad}, ui.RadialGradient{
				Radius: rad,
				Stops: []ui.GradientStop{
					{At: 0, Color: ink}, {At: 0.4, Color: ink.Alpha(0.75)}, {At: 0.7, Color: ink.Alpha(0.3)}, {At: 1, Color: ink.Alpha(0)},
				},
			}, 0)
		case theme.Scan:
			pt.Fill(ui.Rect{X: r.X, Y: cy, W: r.W, H: extent}, ink, 0)
		case theme.Petals:
			path := new(ui.Path).MoveTo(cx-extent, cy-extent*0.6).
				QuadTo(cx+extent, cy-extent, cx+extent, cy+extent*0.6).
				QuadTo(cx-extent, cy+extent, cx-extent, cy-extent*0.6).Close()
			pt.FillPath(path, ink)
		case theme.Twinkle:
			pt.Fill(ui.Rect{X: cx - extent/2, Y: cy - extent/2, W: extent, H: extent}, ink, extent/2)
			arm := ink.Alpha(0.6)
			pt.Fill(ui.Rect{X: cx - extent, Y: cy - 0.35, W: extent * 2, H: 0.7}, arm, 0)
			pt.Fill(ui.Rect{X: cx - 0.35, Y: cy - extent, W: 0.7, H: extent * 2}, arm, 0)
		default:
			pt.Fill(ui.Rect{X: cx - extent/2, Y: cy - extent/2, W: extent, H: extent}, ink, extent/2)
		}
	}
}

// motion is a particle's normalized state (effects/motion.rs Sample).
type motion struct {
	x, y, time, fade, pulse, scale, sway float32
}

// noise hashes a particle's index, the layer's seed and a channel to
// [0, 1), as motion.rs does with wrapping u32 arithmetic.
func noise(index, seed, channel uint32) float32 {
	v := index*0x9e3779b9 ^ seed ^ channel*0x85ebca6b
	v ^= v >> 16
	v *= 0x7feb352d
	v ^= v >> 15
	v *= 0x846ca68b
	v ^= v >> 16
	return float32(v&0xffffff) / 0x1000000
}

// sample is particle index's state at phase: where it is, how far through
// its own cycle, how faded, pulsing, large and swaying.
func sample(index, seed uint32, phase float32) motion {
	t := phase + noise(index, seed, 1)
	t -= float32(math.Floor(float64(t))) // rem_euclid(1)
	f := sin(t * math.Pi)
	p := sin(t * 2 * math.Pi)
	return motion{
		x:     0.025 + noise(index, seed, 2)*0.95,
		y:     0.025 + noise(index, seed, 3)*0.95,
		time:  t,
		fade:  f * f,
		pulse: 0.12 + 0.88*p*p,
		scale: 0.65 + noise(index, seed, 4)*0.7,
		sway:  p * 0.025,
	}
}

func sin(x float32) float32 { return float32(math.Sin(float64(x))) }
