package kit

import (
	"math"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/theme"
)

// Thinking is the thinking indicator's state (ui/thinking types.rs
// OrbState): what the run does picks its figure.
type Thinking int

const (
	// Breathing is a still ring of dots: no known phase (Ring, static).
	Breathing Thinking = iota
	// Working: tools or subagents run (Orbits).
	Working
	// Searching: the model thinks or the request waits (Globe).
	Searching
	// Composing: the model writes the answer (Ribbon).
	Composing
)

// ThinkingOf maps a phase's label key to its state (ui/thinking state).
func ThinkingOf(phase string) Thinking {
	switch phase {
	case "turn_awaiting_response", "turn_thinking":
		return Searching
	case "turn_generating":
		return Composing
	case "turn_tools_running", "turn_waiting_children", "turn_waiting_child":
		return Working
	}
	return Breathing
}

// Indicator draws the thinking indicator (ui/thinking/mod.rs, paint.rs):
// dots in the foreground blended toward the background by depth, a
// figure per state, animated at 60 fps except Breathing, still under
// Reduce Motion. Custom drawing: Kit has no such figure.
func Indicator(c *ui.Context, st Thinking, size float32) ui.Element {
	p := theme.Of(c)
	still := st == Breathing || c.Preferences().ReduceMotion
	return ui.Box(c).Size(size, size).Shrink(0).Draw(func(pt *ui.Painter, r ui.Rect) {
		t := float64(0)
		if !still {
			pt.AnimationFrameRate(FPS)
			t = float64(pt.Now().UnixMilli()%600000) / 1000
		}
		dot := func(x, y, rad, depth float64) {
			// depth 0 (far) .. 1 (near): far dots fade toward the background.
			a := float32(0.25 + 0.75*depth)
			d := float32(rad * 2)
			pt.Fill(ui.Rect{X: r.X + float32(x) - d/2, Y: r.Y + float32(y) - d/2, W: d, H: d}, p.Foreground.Mix(p.Background, 1-a), d/2)
		}
		s := float64(r.W)
		cx, cy, R := s/2, s/2, s/2*0.82
		dr := math.Max(0.6, s/28)
		switch st {
		case Working: // Orbits: three tilted orbits, faint tracks, a particle each
			for o := range 3 {
				tilt := float64(o) * math.Pi / 3
				ro := R * (0.55 + 0.2*float64(o))
				sp := (0.9 + 0.35*float64(o)) * float64(1-2*(o%2))
				for k := range 14 {
					a := 2 * math.Pi * float64(k) / 14
					x, y := ro*math.Cos(a), ro*0.42*math.Sin(a)
					dot(cx+x*math.Cos(tilt)-y*math.Sin(tilt), cy+x*math.Sin(tilt)+y*math.Cos(tilt), dr*0.55, 0.15)
				}
				a := t*sp*2 + float64(o)
				x, y := ro*math.Cos(a), ro*0.42*math.Sin(a)
				dot(cx+x*math.Cos(tilt)-y*math.Sin(tilt), cy+x*math.Sin(tilt)+y*math.Cos(tilt), dr*1.3, 0.6+0.4*(0.5+0.5*math.Sin(a)))
			}
		case Searching: // Globe: latitude rings of dots turning about the axis
			for lat := -2; lat <= 2; lat++ {
				phi := float64(lat) * math.Pi / 6
				n := 10 - 2*abs(lat)
				for k := range n {
					lon := 2*math.Pi*float64(k)/float64(n) + t*0.9
					x := R * math.Cos(phi) * math.Sin(lon)
					z := math.Cos(phi) * math.Cos(lon)
					y := R * math.Sin(phi)
					dot(cx+x, cy-y*0.9, dr*(0.7+0.4*(z+1)/2), (z+1)/2)
				}
			}
		case Composing: // Ribbon: two strands weaving across
			for strand := range 2 {
				off := float64(strand) * math.Pi
				for k := range 13 {
					u := float64(k) / 12
					a := u*2*math.Pi + t*2.2 + off
					z := (math.Cos(a) + 1) / 2
					dot(cx-R+2*R*u, cy+R*0.45*math.Sin(a), dr*(0.6+0.6*z), z)
				}
			}
		default: // Ring: a still ring of dots
			for k := range 12 {
				a := 2*math.Pi*float64(k)/12 - math.Pi/2
				dot(cx+R*0.8*math.Cos(a), cy+R*0.8*math.Sin(a), dr, 0.35+0.65*float64(k)/12)
			}
		}
	})
}

func abs(n int) int {
	if n < 0 {
		return -n
	}
	return n
}
