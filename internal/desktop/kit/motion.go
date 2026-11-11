// Package kit holds Sailry's shared components: the look of the reference
// app's Kit, built on MyGo's widgets, with every color and size from
// internal/desktop/theme. Pages compose them and add only their layout
// (docs/kit.md).
package kit

import (
	"fmt"
	"hash/fnv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// FPS caps drawings that animate. MyGo's only frame-rate parameter is
// Painter.AnimationFrameRate, so looping indicators are drawn with it rather
// than built from Element.Loop, which runs at the display's rate.
const FPS = 60

// Fast is Kit's motion duration for small transitions (the jump button's
// fade, popovers): 200 ms.
const Fast = 200 * time.Millisecond

// EaseOutCubic is Kit's ease_out_cubic: 1 - (1-t)^3.
func EaseOutCubic(t float32) float32 {
	u := 1 - t
	return 1 - u*u*u
}

// Shimmer is a label with a highlight sweeping across it every 2 s, as Kit's
// ShimmerText marks the running activity. It is a Sweep of its own; the
// texts of one row share one with Sweep.Text.
func Shimmer(c *ui.Context, text string, size float32, color ui.Color) ui.Element {
	var sw Sweep
	span := ui.Span{Text: text, Size: size, Color: color}
	return sw.Text(c, ui.RichText(c, span).SingleLine(), span)
}

// Sweep is one shimmer band across a row (Kit's ShimmerText): 12 clipped
// layers of a highlight 0.3 of the row wide, peaking at 0.75 (light) or 0.6
// (dark) opacity, crossing the row every 2 s. Every text given to Text
// shows the part of the band over it, each in its own font and color, so
// the band moves across them as one; what is not given (an icon) has
// none. The band is placed in the row's box, which Frame records as the
// row paints: paint only, nothing is rebuilt. Custom drawing: MyGo has no
// text highlight that moves. It draws at FPS and stays still with Reduce
// Motion.
type Sweep struct {
	row  ui.Rect
	used bool
}

// Frame makes row the box the band crosses. Call it on the row's element
// after its children are built; it does nothing when no text took the
// band.
func (s *Sweep) Frame(row ui.Element) {
	if s.used {
		row.Draw(func(_ *ui.Painter, r ui.Rect) { s.row = r })
	}
}

// Text draws the band over e, a single-line text of span's text, font,
// size and color. Without a Frame, the band crosses e alone. The band is
// the span drawn again by the painter, as a RichText: its line starts from
// the theme's font size, the strut, whatever the span's. So e must be laid
// out the same way, as ui.RichText(c, span) with no font size of its own,
// which Shimmer builds and LevelSpan levels. A plain ui.Text agrees with
// it only at the theme's own size and font. Smaller, its baseline is its
// own size's and the band's the strut's, about 2 DIPs lower at 12 under
// 14; larger, it has half its font's line gap above it, where the font has
// one, and the band's line, grown past the strut, has none; in another
// font the ascents differ. The glyphs then sit off the band's and the
// text looks doubled or heavier as the band passes.
func (s *Sweep) Text(c *ui.Context, e ui.Element, span ui.Span) ui.Element {
	if c.Preferences().ReduceMotion {
		return e
	}
	s.used = true
	p := theme.Of(c)
	peak, hi := float32(0.75), p.Background.MixOklab(span.Color, 0.2)
	if p.Dark {
		peak, hi = 0.6, p.Foreground.MixOklab(span.Color, 0.2)
	}
	const layers = 12
	span.Color = hi.Alpha(peak / layers)
	return e.DrawOver(func(pt *ui.Painter, r ui.Rect) {
		pt.AnimationFrameRate(FPS)
		row := s.row
		if row.W <= 0 {
			row = r
		}
		phase := float32(pt.Now().UnixMilli()%2000) / 2000
		spread := 0.3 * row.W
		center := row.X - spread + phase*(row.W+2*spread)
		if center+spread/2 < r.X || center-spread/2 > r.X+r.W {
			return // the band is elsewhere in the row
		}
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

// Identicon is a subagent's mark: its agent mark in its color while it runs,
// turning, and muted once it settles.
func Identicon(c *ui.Context, id string, running bool, size float32) ui.Element {
	color := theme.Of(c).MutedForeground
	if running {
		color = AgentColor(c, id)
	}
	return Agent(c, id, color, running, size)
}

// AgentColor is the color an agent's ID picks.
func AgentColor(c *ui.Context, id string) ui.Color {
	_, tint := identity(id)
	pair := theme.Identicon[tint]
	if theme.Of(c).Dark {
		return pair[0]
	}
	return pair[1]
}

// Agent is an agent's procedural mark: one of eight SVG designs its ID picks,
// drawn as an icon in color. Turning, it makes a 450 ms smoothstep revolution
// each second (identicon::rotation), at kit.FPS, and rests under Reduce
// Motion. Custom drawing: MyGo's Avatar draws initials, not procedural marks,
// and Element.Loop runs at the display's rate.
func Agent(c *ui.Context, id string, color ui.Color, turning bool, size float32) ui.Element {
	shape, _ := identity(id)
	svg := agentDesigns[shape]
	if !turning || c.Preferences().ReduceMotion {
		return ui.Icon(c, svg).FontSize(size).TextColor(color).Shrink(0)
	}
	return ui.Box(c).Size(size, size).Shrink(0).Draw(func(pt *ui.Painter, r ui.Rect) {
		pt.AnimationFrameRate(FPS)
		t := min(float32(pt.Now().UnixMilli()%1000)/450, 1)
		turn := t * t * (3 - 2*t) * 360
		pt.Transform(ui.Rotation(turn), r.X+r.W/2, r.Y+r.H/2, func() { pt.Icon(svg, r, color) })
	})
}

// identity is the design and color an ID picks: FNV-1a 64 of its bytes,
// modulo 8 and its bits from 16 modulo 8, the same on every platform.
func identity(id string) (shape, tint int) {
	h := fnv.New64a()
	h.Write([]byte(id))
	sum := h.Sum64()
	return int(sum % 8), int((sum >> 16) % 8)
}

// agentDesigns are a petal repeated around the center, then a center dot.
var agentDesigns = func() (out [8]*ui.SVG) {
	counts := [8]int{6, 4, 8, 8, 5, 6, 4, 10}
	petals := [8]string{
		"<ellipse cx='12' cy='6.8' rx='3.1' ry='4.5' opacity='.45'/><ellipse cx='12' cy='6.3' rx='1.5' ry='2.3' opacity='.5'/>",
		"<circle cx='12' cy='6.5' r='4.5' opacity='.45'/><circle cx='12' cy='5.6' r='2' opacity='.42'/>",
		"<rect x='10.6' y='1.5' width='2.8' height='7.2' rx='1.4' opacity='.7'/>",
		"<path d='M12 2 Q16 9 12 12 Q8 9 12 2' opacity='.42'/>",
		"<ellipse cx='12' cy='6.5' rx='3.7' ry='4.8' opacity='.5'/>",
		"<circle cx='12' cy='5.5' r='2.6' opacity='.6'/><circle cx='12' cy='9' r='1.4' opacity='.42'/>",
		"<path d='M12 2 L15 8 L12 12 L9 8 Z' opacity='.6'/><circle cx='12' cy='5' r='1.3' opacity='.45'/>",
		"<ellipse cx='12' cy='6.5' rx='1.4' ry='4.8' opacity='.5'/>",
	}
	for shape := range out {
		var b strings.Builder
		b.WriteString("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='black'>")
		for i := range counts[shape] {
			fmt.Fprintf(&b, "<g transform='rotate(%g 12 12)'>%s</g>", float32(i)*360/float32(counts[shape]), petals[shape])
		}
		b.WriteString("<circle cx='12' cy='12' r='2.3' opacity='.85'/></svg>")
		out[shape] = ui.MustParseSVG([]byte(b.String()))
	}
	return out
}()
