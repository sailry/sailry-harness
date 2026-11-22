// Package kit holds Sailry's shared components: the look of the reference
// app's Kit, built on MyGo's widgets, with every color and size from
// internal/desktop/theme. Pages compose them and add only their layout.
package kit

import (
	"fmt"
	"hash/fnv"
	"strings"
	"time"
	"unicode"
	"unicode/utf16"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// FPS caps drawings that animate. MyGo's only frame-rate parameter is
// Painter.AnimationFrameRate, so looping indicators are drawn with it rather
// than built from Element.Loop, which runs at the display's rate.
const FPS = 60

const sweepPeriod = 2 * time.Second

func visibleMotionRect(p *ui.Painter, r ui.Rect) ui.Rect {
	v := p.Visible()
	x, y := max(r.X, v.X), max(r.Y, v.Y)
	return ui.Rect{X: x, Y: y, W: min(r.X+r.W, v.X+v.W) - x, H: min(r.Y+r.H, v.Y+v.H) - y}
}

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
	row    ui.Rect
	labels int
}

type sweepLabelKey struct{}

type sweepLabel struct {
	span      ui.Span
	font      ui.Font
	direction ui.Direction
	width, x  float32
	layout    *ui.TextLayout
}

// MyGo does not expose the layout of a SingleLine element to DrawOver.
// Shape its displayed line on width or style changes, then reuse it on every
// pass. The wrap check uses the same font fallback and line breaking as RichText.
func (l *sweepLabel) fit(span ui.Span, font ui.Font, direction ui.Direction, width float32) {
	width = max(width, 1)
	if l.layout != nil && l.span == span && l.font == font && l.direction == direction && l.width == width {
		return
	}
	*l = sweepLabel{span: span, font: font, direction: direction, width: width}
	shape := ui.TextShape{Direction: direction, Width: width}
	layout := ui.ShapeRichTextWith([]ui.Span{span}, font, shape)
	lines := layout.Lines()
	if len(lines) == 1 {
		// Keep the plain-label fast path when its geometry matches RichText.
		if (span.Size == 0 || span.Size == font.Size) && span == (ui.Span{Text: span.Text, Size: span.Size}) &&
			strings.TrimSpace(span.Text) == span.Text && !lines[0].RTL && direction != ui.RightToLeft {
			layout = ui.ShapeText(span.Text, font, 0)
		}
		l.layout = layout
		return
	}

	// SingleLine reserves the theme-sized ellipsis and retains whole graphemes
	// from the first paragraph. The visible ellipsis takes the last span's font.
	paragraph, _, _ := strings.Cut(span.Text, "\n")
	paragraph = strings.TrimSuffix(paragraph, "\r")
	span.Text = sweepPrefix(paragraph, font, span, layout, width)
	if span.Text == "" {
		span = ui.Span{Text: "…"}
	} else {
		span.Text += "…"
	}
	l.layout = ui.ShapeRichText([]ui.Span{span}, font, 0)
	if direction == ui.RightToLeft || lines[0].RTL {
		w, _ := l.layout.Size()
		l.x = width - w
	}
}

func sweepPrefix(text string, font ui.Font, span ui.Span, layout *ui.TextLayout, width float32) string {
	markWidth, _ := ui.ShapeText("…", font, 0).Size()
	if span.Font != "" {
		font.Family = span.Font
	}
	if span.Size != 0 {
		font.Size = span.Size
	}
	font.Weight, font.Italic, font.Features = span.Weight, span.Italic, span.Features
	if span.Lang != "" {
		font.Lang = span.Lang
	}
	advances := make([]float32, utf8.RuneCountInString(text))
	for _, glyph := range ui.Shape(text, font) {
		advances[glyph.Cluster] += glyph.Advance
	}
	var used float32
	units, index, cut := 0, 0, 0
	next := layout.NextBoundary(0)
	for i, r := range text {
		if units == next {
			if used+markWidth > width {
				break
			}
			cut = i
			next = layout.NextBoundary(units)
		}
		used += advances[index]
		units += utf16.RuneLen(r)
		index++
	}
	if used+markWidth <= width {
		cut = len(text)
	}
	return strings.TrimRightFunc(text[:cut], unicode.IsSpace)
}

// Frame makes row the box the band crosses. Call it on the row's element
// after its children are built; it does nothing when no text took the
// band. A single label uses its own width so the band stays visible even
// when the containing row spans a wide transcript.
func (s *Sweep) Frame(row ui.Element) {
	if s.labels > 1 {
		row.Draw(func(_ *ui.Painter, r ui.Rect) { s.row = r })
	}
}

// Text draws the band over e, a ui.RichText(c, span).SingleLine() with
// the theme's font and size as its base style, so the text and band use
// the same baseline. Without a Frame, the band crosses e alone.
func (s *Sweep) Text(c *ui.Context, e ui.Element, span ui.Span) ui.Element {
	if c.Preferences().ReduceMotion || span.Text == "" {
		return e
	}
	s.labels++
	p := theme.Of(c)
	peak, hi := float32(0.75), p.Background.MixOklab(span.Color, 0.2)
	if p.Dark {
		peak, hi = 0.6, p.Foreground.MixOklab(span.Color, 0.2)
		if span.Color == p.Foreground {
			hi = p.MutedForeground
		}
	}
	const layers = 12
	color := hi.Alpha(peak / layers)
	span.Color = ui.Transparent
	font := ui.Font{Family: c.Theme().Font, Size: c.Theme().FontSize, Lang: c.Locale()}
	direction := c.Direction()
	label := ui.Local(e, sweepLabelKey{}, func() sweepLabel { return sweepLabel{} })
	return e.DrawOver(func(pt *ui.Painter, r ui.Rect) {
		visible := visibleMotionRect(pt, r)
		if visible.W <= 0 || visible.H <= 0 {
			return
		}
		row := s.row
		if row.W <= 0 {
			row = r
		}
		spread := 0.3 * row.W
		origin, travel := row.X-spread, row.W+2*spread
		// The band moves linearly. Wake when its leading edge next enters
		// this visible text box, not at every refresh across the row's gaps.
		crossing := func(x float32) time.Duration {
			return time.Duration(float64((x-origin)/travel) * float64(sweepPeriod))
		}
		enter := max(0, crossing(visible.X-spread/2))
		leave := min(sweepPeriod, crossing(visible.X+visible.W+spread/2))
		if enter >= leave {
			return
		}
		now := pt.Now()
		elapsed := now.Sub(now.Truncate(sweepPeriod))
		if elapsed < enter {
			pt.After(enter - elapsed)
			return
		}
		if elapsed >= leave {
			pt.After(sweepPeriod - elapsed + enter)
			return
		}
		pt.AnimationFrameRate(FPS)
		center := origin + float32(elapsed)/float32(sweepPeriod)*travel
		label.fit(span, font, direction, r.W)
		pt.Clip(r, 0, func() {
			for i := range layers {
				half := spread / 2 * (1 - float32(i)/layers)
				if center+half <= visible.X || center-half >= visible.X+visible.W {
					break // all remaining clips are narrower
				}
				pt.Clip(ui.Rect{X: center - half, Y: r.Y, W: 2 * half, H: r.H}, 0, func() {
					pt.TextLayout(label.layout, r.X+label.x, r.Y, color)
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
		if visible := visibleMotionRect(pt, r); visible.W <= 0 || visible.H <= 0 {
			return
		}
		const spin = 450 * time.Millisecond
		now := pt.Now()
		elapsed := now.Sub(now.Truncate(time.Second))
		if elapsed >= spin {
			pt.Icon(svg, r, color)
			pt.After(time.Second - elapsed)
			return
		}
		pt.AnimationFrameRate(FPS)
		t := float32(elapsed) / float32(spin)
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
