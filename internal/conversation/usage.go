package conversation

import (
	"fmt"
	"math"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// Metric is a usage figure of the context bar: its label key, icon and
// value as shown.
type Metric struct {
	Label string
	Icon  *ui.SVG
	Value string
}

// Metrics are a session's figures in conversation/usage/preview.rs's
// order: tokens, speed, cost, cache and turns.
func Metrics(u *Usage) []Metric {
	return []Metric{
		{"composer_tokens", icons.ChartPie, Count(u.Tokens)},
		{"composer_speed", icons.Gauge, fmt.Sprintf("%.0f/s", u.Speed)},
		{"composer_cost", icons.DollarSign, fmt.Sprintf("%.2f", u.Cost)},
		{"composer_cache", icons.MemoryStick, Count(u.Cached)},
		{"composer_turns", icons.MessagesSquare, fmt.Sprint(u.Turns)},
	}
}

// Count writes a token count short: 950, 1.2k, 3.4M.
func Count(n int) string {
	switch {
	case n < 1000:
		return fmt.Sprint(n)
	case n < 1_000_000:
		return trim(fmt.Sprintf("%.1f", float64(n)/1000)) + "k"
	}
	return trim(fmt.Sprintf("%.1f", float64(n)/1_000_000)) + "M"
}

func trim(s string) string {
	if len(s) > 2 && s[len(s)-2:] == ".0" {
		return s[:len(s)-2]
	}
	return s
}

// UsageBar is the context bar's metrics (usage.rs render_groups): each a
// 14 DIP icon and its value in muted 14, its label in a hover card.
// compact, below the composer's compact width, makes them one button
// (usage/summary.rs) whose popover is open while *open is true.
func UsageBar(c *ui.Context, u *Usage, compact bool, open *bool) {
	if u == nil {
		return
	}
	if compact {
		usageButton(c, u, open)
		return
	}
	p := theme.Of(c)
	ui.Row(c.Key("usage")).Gap(12).AlignItems(ui.Center).TextColor(p.MutedForeground).Children(func() {
		for _, m := range Metrics(u) {
			item := ui.Row(c.Key(m.Label)).Gap(4).AlignItems(ui.Center)
			item.Children(func() {
				ui.Icon(c, m.Icon).FontSize(14)
				ui.Text(c, m.Value).FontSize(14).SingleLine()
			})
			ui.TooltipBase(c, item, func(card ui.Element) {
				kit.Floating(c, card).AttachTo(item, ui.AnchorTopEnd, ui.AnchorBottomEnd).Margin(0, 0, 6, 0).Padding(12, 14).Gap(4).
					AttachedOrigin().Transition(kit.TipMotion(kit.Top))
				card.Children(func() {
					ui.Text(c, i18n.T(c, m.Label)).FontSize(12).TextColor(p.MutedForeground)
					ui.Text(c, m.Value).FontSize(14).FontWeight(500)
				})
			})
		}
	})
}

// usageWidth is the figures panel's width, usage.rs POPOVER_WIDTH.
const usageWidth = 300

// usageButton is usage/summary.rs's compact figures: a small round button
// in muted text with the chart icon and the first figure, the tokens. A
// hover shows every figure in a card; a click holds them in a popover.
func usageButton(c *ui.Context, u *Usage, open *bool) {
	p := theme.Of(c)
	ms := Metrics(u)
	b := kit.GhostLook(c, ui.ButtonBase(c.Key("usage")).Label(i18n.T(c, "composer_statistics")), *open).
		Height(28).PaddingX(8).Gap(4).Radius(14).Shrink(0).TextColor(p.MutedForeground)
	b.HasPopup(ui.PopupDialog).Expanded(*open).OnClick(func() { *open = !*open })
	b.Children(func() {
		ui.Icon(c, icons.ChartPie).FontSize(14)
		ui.Text(c, ms[0].Value).FontSize(14).SingleLine()
	})
	figures := func() {
		for _, m := range ms {
			ui.Row(c.Key(m.Label)).Gap(8).AlignItems(ui.Center).Children(func() {
				ui.Icon(c, m.Icon).FontSize(14).TextColor(p.MutedForeground)
				ui.Text(c, i18n.T(c, m.Label)).FontSize(14).TextColor(p.MutedForeground)
				ui.Spacer(c)
				ui.Text(c, m.Value).FontSize(14).FontWeight(500).SingleLine()
			})
		}
	}
	if !*open {
		ui.TooltipBase(c, b, func(card ui.Element) {
			kit.Floating(c, card).AttachTo(b, ui.AnchorTopEnd, ui.AnchorBottomEnd).Margin(0, 0, 6, 0).Padding(12, 14).Gap(6).
				Width(usageWidth).AttachedOrigin().Transition(kit.TipMotion(kit.Top))
			card.Children(figures)
		})
	}
	kit.Popover(c, b, open, figures).AttachTo(b, ui.AnchorTopEnd, ui.AnchorBottomEnd).Margin(0, 0, 6, 0).
		Padding(12, 14).Gap(6).Width(usageWidth)
}

// ContextRing is the composer's context indicator, left of the model chip
// (usage/preview.rs context_progress, placed as live/composer.rs's
// Composer-end controls): Kit's ProgressCircle at 20 DIPs with 4 on each
// side, the track and the part used, muted, with the compaction hint as
// its tooltip. Preview data has no indicator panel to open, so it takes
// no clicks.
func ContextRing(c *ui.Context, u *Usage) {
	if u == nil {
		return
	}
	p := theme.Of(c)
	v := math.Max(0, math.Min(1, u.Context))
	box := ui.Box(c.Key("context")).Size(20, 20).Margin(0, 4).Shrink(0)
	kit.Tooltip(c, box, i18n.T(c, "composer_compression_hint"))
	box.Label(fmt.Sprintf("%s %d%%", i18n.T(c, "composer_compression_hint"), int(v*100+0.5))).
		Draw(func(pt *ui.Painter, r ui.Rect) {
			cx, cy, rad := r.X+r.W/2, r.Y+r.H/2, r.W/2-2
			var track ui.Path
			track.Circle(cx, cy, rad)
			pt.StrokePath(&track, 2, p.MutedForeground.Alpha(0.25))
			if v <= 0 {
				return
			}
			var arc ui.Path
			start := -math.Pi / 2
			steps := max(2, int(48*v))
			for i := 0; i <= steps; i++ {
				a := start + 2*math.Pi*v*float64(i)/float64(steps)
				x, y := cx+rad*float32(math.Cos(a)), cy+rad*float32(math.Sin(a))
				if i == 0 {
					arc.MoveTo(x, y)
				} else {
					arc.LineTo(x, y)
				}
			}
			pt.StrokePath(&arc, 2, p.MutedForeground)
		})
}
