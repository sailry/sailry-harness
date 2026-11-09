package conversation

import (
	_ "embed"
	"math"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// The new session's welcome page (conversation/welcome.rs): the theme's
// hero, the wordmark, the title, the composer, and the four mode cards that
// put their prompt in the composer.

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
// the composer under the title; prompt gets a card's prompt. Below
// pairWidth the page is compact: narrower padding, the cards stacked,
// and the wordmark shrinks with the width, leaving room for the theme's
// brand beside it (welcome.rs layout).
func Welcome(c *ui.Context, s *State, composer func(), prompt func(string)) {
	scroll := ui.Scroll(c.Key("welcome")).Grow(1)
	// The wordmark's light follows the pointer across the page and stays
	// where it left (welcome/wordmark.rs Light).
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
	logo = min(logo, float32(math.Floor(float64(available/(markW/markH+1)))))
	scroll.Children(func() {
		ui.Column(c).FillWidth().MinHeightPercent(100).MaxWidth(theme.ContentWidth).Margin(0, ui.Auto).
			Padding(32, 0).Gap(12).Justify(ui.Center).Children(func() {
			ui.Column(c.Key("hero")).FillWidth().PaddingX(padding).Gap(16).AlignItems(ui.Center).Children(func() {
				kit.Banner(c, "hero.new_session")
				ui.Row(c).Gap(12).Padding(4, 0).AlignItems(ui.Center).Children(func() {
					kit.Brand(c, logo)
					wordmark(c, s, logo)
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

// card is a mode card (theme/cards.rs): Kit's custom button in the mode's
// color at 20%, or the package's card colors, texture, border and radius.
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
	if texture := kit.Backdrop(c, "background.card."+m.key); texture != nil {
		b.Draw(func(pt *ui.Painter, r ui.Rect) { pt.Clip(r, radius, func() { texture(pt, r) }) })
	}
	b.Children(func() {
		ui.Column(c).FillWidth().MinWidth(0).Gap(16).Children(func() {
			ui.Row(c).FillWidth().Justify(ui.SpaceBetween).Children(func() {
				kit.ArtIcon(c, m.key, m.icon, 24, icon)
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
// "M{x} {y}h6v6h-6z" paths, as welcome/wordmark.rs reads them.
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

// wordmark draws the square-dot lettering: its dim base at 18% of the ink,
// and the particles under a beam that follows the pointer lit up to 86%
// (welcome/wordmark.rs), with the package's glow behind the beam.
// Custom drawing: MyGo's Icon tints a picture in one color; the light
// varies per particle.
func wordmark(c *ui.Context, s *State, height float32) {
	p := theme.Of(c)
	ink, glow := p.Foreground, p.Foreground
	if v := p.Variant; v != nil {
		ink = v.Wordmark.Foreground.Or(ink)
		glow = v.Wordmark.Glow.Or(ink)
	}
	light, lit := s.light, s.lit
	ui.Box(c.Key("wordmark")).Size(height*markW/markH, height).Shrink(0).Label(i18n.T(c, "welcome_name")).
		Role(ui.RoleImage).Draw(func(pt *ui.Painter, r ui.Rect) {
		sx, sy := r.W/markW, r.H/markH
		beam := float32(0.5)
		if lit && r.W > 0 {
			beam = min(max((light-r.X)/r.W, 0), 1)
		}
		center := beam * markW
		if glow != ink {
			gx := r.X + center*sx
			pt.FillRadialGradient(ui.Rect{X: gx - r.H, Y: r.Y - r.H/2, W: 2 * r.H, H: 2 * r.H}, ui.RadialGradient{Radius: r.H,
				Stops: []ui.GradientStop{{At: 0, Color: glow.Alpha(0.22)}, {At: 1, Color: glow.Alpha(0)}},
			}, 0)
		}
		for _, q := range particles {
			x, y := q[0], q[1]
			rect := ui.Rect{X: r.X + x*sx, Y: r.Y + y*sy, W: dot * sx, H: dot * sy}
			pt.Fill(rect, ink.Alpha(0.18*depth(x, y)), 0)
			d := (x + 3 - center) / (markW * 0.13)
			if strength := float32(math.Exp(float64(-0.5 * d * d))); strength > 0.01 {
				pt.Fill(rect, ink.Alpha(strength*0.86), 0)
			}
		}
	})
}
