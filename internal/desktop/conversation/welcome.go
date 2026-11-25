package conversation

import (
	_ "embed"
	"math"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The new session's welcome page: the theme's hero, the wordmark, the title,
// the composer, and the four mode cards that put their prompt in the
// composer.

// pairWidth is the basis of a row of two cards; narrower, they stack.
const pairWidth = 320

// mode is a welcome card: its theme key, copy keys and default icon and
// color (Kit's base colors).
type mode struct {
	key, title, detail, prompt string
	icon                       *ui.SVG
	color                      func(*theme.Palette) ui.Color
}

var modes = []mode{
	{"explore", "welcome_explore", "welcome_explore_detail", "welcome_explore_prompt", icons.Folder, func(p *theme.Palette) ui.Color { return p.Blue }},
	{"build", "welcome_build", "welcome_build_detail", "welcome_build_prompt", icons.Terminal, func(p *theme.Palette) ui.Color { return p.Green }},
	{"review", "welcome_review", "welcome_review_detail", "welcome_review_prompt", icons.Eye, func(p *theme.Palette) ui.Color { return p.Yellow }},
	{"plan", "welcome_plan", "welcome_plan_detail", "welcome_plan_prompt", icons.Dashboard, func(p *theme.Palette) ui.Color { return p.Magenta }},
}

// Welcome builds the page in place of an empty transcript. composer builds
// the composer under the title; prompt gets a card's prompt. Below pairWidth
// the page is compact: narrower padding, the cards stacked, and the wordmark
// shrinks with the width, leaving room for the theme's brand beside it.
func Welcome(c *ui.Context, s *State, composer func(), prompt func(string)) {
	scroll := ui.Scroll(c.Key("welcome")).Grow(1)
	// The wordmark's light follows the pointer across the page and stays where
	// it left.
	if x, _, over := scroll.PointerPosition(); over {
		s.light, s.lit = scroll.LayoutBounds().X+x, true
	}
	width := min(scroll.LayoutBounds().W, theme.ContentWidth)
	if width <= 0 {
		width = theme.ContentWidth
	}
	compact := width < pairWidth
	logo, padding := float32(72), float32(24)
	switch {
	case width < 180:
		logo = 20
	case width < 240:
		logo = 24
	case compact:
		logo = 32
	}
	if compact {
		padding = 8
	}
	available := max(width-2*padding-12, 0)
	mark := strings.TrimSpace(s.Mark)
	if mark == "" {
		// The row is the brand, a square, and the wordmark, markW/markH wide
		// for its height (as a fraction: the constants alone would divide
		// whole).
		logo = min(logo, float32(math.Floor(float64(available/(float32(markW)/markH+1)))))
	} else {
		// Custom lettering keeps at least twice the brand's width.
		logo = min(logo, float32(math.Floor(float64(available/3))))
	}
	scroll.Children(func() {
		// At least the scroll's height (last frame's), so the page centers
		// vertically; taller content grows from the top instead of being
		// cut at both ends.
		// On its first frame the scroll has no height yet: the page stays
		// hidden for that frame and draws centered on the next, rather than
		// showing at the top and jumping down.
		h := scroll.LayoutBounds().H
		page := ui.Column(c).FillWidth().MinHeight(h).MaxWidth(theme.ContentWidth).Margin(0, ui.Auto)
		if h <= 0 {
			page.Opacity(0)
			c.Invalidate()
		}
		page.Padding(32, 0).Gap(12).Justify(ui.Center).Children(func() {
			ui.Column(c.Key("hero")).FillWidth().PaddingX(padding).Gap(16).AlignItems(ui.Center).Children(func() {
				kit.Banner(c, "hero.new_session")
				ui.Row(c).Gap(12).Padding(4, 0).AlignItems(ui.Center).Children(func() {
					kit.Brand(c, logo)
					if mark == "" {
						wordmark(c, s, logo)
					} else {
						letters(c, s, mark, logo, available-logo)
					}
				})
				ui.Text(c, i18n.T(c, "welcome_title")).FontSize(18).TextAlign(ui.Center).FillWidth()
			})
			composer()
			ui.Row(c.Key("modes")).FillWidth().PaddingX(padding).Gap(12).Wrap().Children(func() {
				for i := 0; i < len(modes); i += 2 {
					pair := ui.Row(c).Grow(1).Basis(pairWidth).MinWidth(0).Gap(12).AlignItems(ui.Stretch)
					if compact {
						pair = ui.Column(c).Grow(1).Basis(pairWidth).MinWidth(0).Gap(12).AlignItems(ui.Stretch)
					}
					pair.Children(func() {
						card(c, modes[i], compact, prompt)
						card(c, modes[i+1], compact, prompt)
					})
				}
			})
		})
	})
}

// card is a mode card: Kit's custom button in the mode's color at 20%, or the
// package's card colors, texture, border and radius.
func card(c *ui.Context, m mode, stacked bool, prompt func(string)) {
	p := theme.Of(c)
	color := m.color(p)
	style := theme.Card{}
	if p.Variant != nil {
		style = p.Variant.Cards[m.key]
	}
	radius := p.Radius2xl()
	if style.Radius != nil {
		radius = *style.Radius
	}
	icon := style.Icon.Or(color)
	title := i18n.T(c, m.title)
	b := ui.ButtonBase(c.Key(m.key)).Label(title).MinWidth(0).Padding(16).Radius(radius).
		TextColor(style.Foreground.Or(p.Foreground)).Cursor(ui.CursorPointer)
	if !stacked {
		b.Grow(1).Basis(0) // a pair shares its row evenly
	}
	switch {
	case b.Pressed():
		b.Background(style.Active.Or(color.Alpha(0.26)))
	case b.Hovered():
		b.Background(style.Hover.Or(color.Alpha(0.2)))
	default:
		b.Background(style.Background.Or(color.Alpha(0.2)))
	}
	if style.BorderWidth > 0 {
		b.Border(style.BorderWidth, style.Border.Or(p.Border))
	}
	if style.Glow.OK {
		b.Shadow(0, 0, 20, 0, style.Glow.V)
	}
	if style.Gradient.OK && !b.Pressed() && !b.Hovered() {
		b.LinearGradient(ui.LinearGradient{From: style.Background.Or(color.Alpha(0.2)), To: style.Gradient.V, Angle: 180, Oklab: true})
	}
	if texture := kit.Backdrop(c, "background.card."+m.key); texture != nil {
		b.Draw(func(pt *ui.Painter, r ui.Rect) { pt.Clip(r, radius, func() { texture(pt, r) }) })
	}
	if glow, edge := style.InnerGlow, style.Border.Or(color); glow > 0 {
		b.DrawOver(func(pt *ui.Painter, r ui.Rect) { kit.InnerGlow(pt, r, radius, glow, edge, edge) })
	}
	b.Children(func() {
		ui.Column(c).FillWidth().MinWidth(0).Gap(16).Children(func() {
			ui.Row(c).FillWidth().Justify(ui.SpaceBetween).Children(func() {
				if tile := style.IconBackground; tile.OK {
					ui.Row(c).Size(36, 36).Shrink(0).Radius(10).Background(tile.V).AlignItems(ui.Center).Justify(ui.Center).
						Children(func() { kit.ArtIcon(c, m.key, m.icon, 22, icon) })
				} else {
					kit.ArtIcon(c, m.key, m.icon, 24, icon)
				}
				ui.Icon(c, icons.ArrowRight).FontSize(16).TextColor(icon.Alpha(0.5))
			})
			ui.Column(c).MinWidth(0).Gap(4).Children(func() {
				ui.Text(c, title).FontSize(14)
				ui.Text(c, i18n.T(c, m.detail)).FontSize(12).TextColor(style.Detail.Or(p.MutedForeground))
			})
		})
	}).OnClick(func() { prompt(i18n.T(c, m.prompt)) })
}

//go:embed wordmark.svg
var wordmarkSVG string

// The wordmark's artwork: 6-unit square particles on an 8-unit grid in a
// 328 x 112 box, its depth an alpha gradient along the diagonal.
const (
	markW, markH = 328, 112
	dot          = 6
)

// particles are the wordmark's squares, parsed from the embedded artwork's
// "M{x} {y}h6v6h-6z" paths.
var particles = func() [][2]float32 {
	var out [][2]float32
	for _, path := range strings.Split(wordmarkSVG, "<path ")[1:] {
		_, d, _ := strings.Cut(path, ` d="`)
		d, _, _ = strings.Cut(d, `"`)
		for _, m := range strings.Split(d, "M")[1:] {
			xy, ok := strings.CutSuffix(m, "h6v6h-6z")
			xs, ys, ok2 := strings.Cut(xy, " ")
			x, err1 := strconv.ParseFloat(xs, 32)
			y, err2 := strconv.ParseFloat(ys, 32)
			if !ok || !ok2 || err1 != nil || err2 != nil {
				panic("wordmark particles must be squares: " + m)
			}
			out = append(out, [2]float32{float32(x), float32(y)})
		}
	}
	return out
}()

// depth is the artwork's alpha gradient from its top left to its bottom
// right corner.
func depth(x, y float32) float32 {
	t := (x*markW + y*markH) / (markW*markW + markH*markH)
	stops := [][2]float32{{0, 0.96}, {0.38, 0.76}, {0.72, 0.46}, {1, 0.78}}
	for i := 1; i < len(stops); i++ {
		if t <= stops[i][0] {
			a, b := stops[i-1], stops[i]
			return a[1] + (b[1]-a[1])*(t-a[0])/(b[0]-a[0])
		}
	}
	return stops[len(stops)-1][1]
}

// wordmark draws the Sailry wordmark height tall.
func wordmark(c *ui.Context, s *State, height float32) {
	particleLight(c, s, i18n.T(c, "welcome_name"), particles, markW, markH, height*markW/markH, height, markW*0.13)
}

// Custom lettering is drawn line high for the brand's height, shrinking to
// minScale of that to stay on one line before it wraps; its particles are
// never closer than minPitch DIPs, so a compact page wraps it rather than
// shrinking it past reading.
const (
	minScale = 0.7
	minPitch = 2
)

// letters draws text in the lettering face beside a brand logo tall, in at
// most room DIPs across: a line as tall as the brand, narrower to fit, and
// wrapped below minScale. Its beam is as wide for its height as the
// wordmark's.
func letters(c *ui.Context, s *State, text string, logo, room float32) {
	unit := max(logo/letterLine, minPitch) // DIPs a particle at full size
	pitch := unit
	if full := float32(2 * cellsOf(text)); full*unit > room && full > 0 {
		pitch = max(room/full, unit*minScale, minPitch)
	}
	cols := 0
	if pitch > 0 && float32(2*cellsOf(text))*pitch > room {
		cols = max(int(room/(2*pitch)), 1)
	}
	if s.letters.text != text || s.letters.cols != cols {
		s.letters = layoutLettering(text, cols)
	}
	l := s.letters
	particleLight(c, s, text, l.particles, l.w, l.h, l.w*pitch/8, l.h*pitch/8, 8*letterLine*markW*0.13/markH)
}

// particleLight draws square-dot lettering of 6-unit particles in a w x h
// unit box at width x height DIPs: its base at the package's rest strength
// (18% of the ink by default), shaded across to the package's gradient
// color where it has one, and the particles under a beam sigma units wide
// that follows the pointer lit up to 86%, the lit particles in the
// package's glow color, the ink by default; nothing is drawn outside the
// lettering. Custom drawing: MyGo's Icon tints a picture in one color; the
// light varies per particle.
func particleLight(c *ui.Context, s *State, label string, parts [][2]float32, w, h, width, height, sigma float32) {
	p := theme.Of(c)
	ink, glow := p.Foreground, p.Foreground
	end, base := ui.Color{}, float32(theme.WordmarkBase)
	if v := p.Variant; v != nil {
		ink = v.Wordmark.Foreground.Or(ink)
		glow = v.Wordmark.Glow.Or(ink)
		end = v.Wordmark.Gradient.Or(ink)
		if v.Wordmark.Base > 0 {
			base = v.Wordmark.Base
		}
	} else {
		end = ink
	}
	light, lit := s.light, s.lit
	ui.Box(c.Key("wordmark")).Size(width, height).Shrink(0).Label(label).
		Role(ui.RoleImage).Draw(func(pt *ui.Painter, r ui.Rect) {
		sx, sy := r.W/w, r.H/h
		beam := float32(0.5)
		if lit && r.W > 0 {
			beam = min(max((light-r.X)/r.W, 0), 1)
		}
		center := beam * w
		for _, q := range parts {
			x, y := q[0], q[1]
			rect := ui.Rect{X: r.X + x*sx, Y: r.Y + y*sy, W: dot * sx, H: dot * sy}
			pt.Fill(rect, ink.Mix(end, x/w).Alpha(base*depth(x/w*markW, y/h*markH)), 0)
			d := (x + 3 - center) / sigma
			if strength := float32(math.Exp(float64(-0.5 * d * d))); strength > 0.01 {
				// The light shows only through the particles, in the
				// package's glow color: the lettering is its mask.
				pt.Fill(rect, glow.Alpha(strength*0.86), 0)
			}
		}
	})
}
