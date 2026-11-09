package composer

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// track is model_controls/track.rs: the effort slider on MyGo's SliderBase
// (keys, dragging, the slider role), drawn as the reference draws it, a
// 26 DIP rounded base at 16% of the slider color, the fill to the thumb in
// the emphasis color, or a danger-to-warning gradient with a moving sheen
// at the highest level, a dot per level and a 32 DIP thumb. Custom
// drawing: MyGo's StepSlider has neither the gradient fill nor the dots.
func track(c *ui.Context, value *float64, steps int, highest, disabled bool) {
	p := theme.Of(c)
	hi := float64(max(steps-1, 1))
	s := ui.SliderBase(c.Key("effort"), value, 0, hi).Step(1).Label(i18n.T(c, "composer_effort")).
		FillWidth().Height(40).PaddingX(16 + 8).Disabled(disabled).Shrink(0)
	s.Draw(func(pt *ui.Painter, r ui.Rect) {
		a := float32(1)
		if disabled {
			a = 0.5
		}
		surface := p.Floating()
		frac := float32(*value / hi)
		base := ui.Rect{X: r.X + 8, Y: r.Y + 7, W: r.W - 16, H: 26}
		pt.Fill(base, surface.Mix(p.Slider, 0.16), 13)
		if frac > 0 {
			// right(1 − pct) and mr(2·pct − 1 rem): the fill reaches the
			// thumb's center in the middle, past it toward the end.
			w := base.W*frac + 16*(2*frac-1)
			fill := ui.Rect{X: base.X, Y: base.Y, W: min(max(w, 0), base.W), H: base.H}
			if highest {
				pt.FillGradient(fill, ui.LinearGradient{From: surface.Mix(p.Danger, a), To: surface.Mix(p.Warning, a), Angle: 90}, 13)
				if !disabled && !pt.Preferences().ReduceMotion {
					pt.AnimationFrameRate(kit.FPS)
					phase := float32(pt.Now().UnixMilli()%1600) / 1600
					x := fill.X - fill.W*0.4 + fill.W*1.4*phase
					pt.Clip(fill, 13, func() {
						pt.FillGradient(ui.Rect{X: x, Y: fill.Y, W: fill.W * 0.4, H: fill.H}, ui.LinearGradient{
							Angle: 90, Stops: []ui.GradientStop{{Color: ui.RGBA(1, 1, 1, 0)}, {Color: ui.RGBA(1, 1, 1, 0.28), At: 0.5}, {Color: ui.RGBA(1, 1, 1, 0), At: 1}},
						}, 0)
					})
				}
			} else {
				pt.Fill(fill, surface.Mix(p.Primary, a), 13)
			}
		}
		dot := p.SliderThumb
		if p.Dark {
			dot = p.MutedForeground
		}
		inner := ui.Rect{X: base.X + 16, W: base.W - 32}
		for i := range steps {
			x := inner.X + inner.W*float32(i)/float32(max(steps-1, 1))
			pt.Fill(ui.Rect{X: x - 2, Y: base.Y + 11, W: 4, H: 4}, dot, 2)
		}
		thumb := p.SliderThumb
		if p.Dark {
			thumb = p.Foreground
		}
		tx := inner.X + inner.W*frac
		t := ui.Rect{X: tx - 16, Y: base.Y + 13 - 16, W: 32, H: 32}
		pt.Shadow(t, 16, 0, 1, 2, 0, p.Backdrop().Alpha(0.12))
		pt.Fill(t, surface.Mix(thumb, a), 16)
		pt.Stroke(t, p.Border, 16, 1)
	})
}
