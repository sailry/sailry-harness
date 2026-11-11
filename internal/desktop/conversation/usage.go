package conversation

import (
	"fmt"
	"math"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Metric is a usage figure of the context bar: its label key, icon and
// value as shown.
type Metric struct {
	Label string
	Icon  *ui.SVG
	Value string
}

// Metrics are tokens, speed, cost, cache and turns.
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

// UsageBar is the context bar's metrics: each a 14 DIP icon and its value in
// muted 14, its label in a hover card. compact, below the composer's compact
// width, makes them one button whose popover is open while *open is true.
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
			if kit.TipsHidden(c) {
				continue
			}
			ui.TooltipBase(c, item, func(card ui.Element) {
				kit.Anchored(card, item, kit.CardLayer)
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

// usageWidth is the figures panel's width.
const usageWidth = 300

// usageButton is a small round button in muted text with the chart icon and
// the first figure, the tokens. A hover shows every figure in a card; a click
// holds them in a popover.
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
	if !*open && !kit.TipsHidden(c) {
		ui.TooltipBase(c, b, func(card ui.Element) {
			kit.Anchored(card, b, kit.CardLayer)
			kit.Floating(c, card).AttachTo(b, ui.AnchorTopEnd, ui.AnchorBottomEnd).Margin(0, 0, 6, 0).Padding(12, 14).Gap(6).
				Width(usageWidth).AttachedOrigin().Transition(kit.TipMotion(kit.Top))
			card.Children(figures)
		})
	}
	kit.Popover(c, b, open, figures).AttachTo(b, ui.AnchorTopEnd, ui.AnchorBottomEnd).Margin(0, 0, 6, 0).
		Padding(12, 14).Gap(6).Width(usageWidth)
}

// ContextRing is the composer's context indicator, left of the model chip:
// Kit's ring with 4 on each side, the track and the part used, muted, with
// its tooltip the tokens used of the model's window where the usage knows the
// window, as a measured indicator's reads, and the preview's compaction hint
// where it does not. Sample data has no indicator panel to open, so it takes
// no clicks.
func ContextRing(c *ui.Context, u *Usage) {
	if u == nil {
		return
	}
	percent := math.Max(0, math.Min(1, u.Context)) * 100
	hint := i18n.T(c, "composer_compression_hint")
	if u.Window > 0 {
		hint = Count(int(percent/100*float64(u.Window)+0.5)) + " / " + Count(u.Window)
	}
	box := ui.Row(c.Key("context")).Shrink(0).PaddingX(4).AlignItems(ui.Center)
	kit.Tooltip(c, box, hint)
	box.Children(func() {
		kit.Ring(c, percent, false, kit.Neutral, fmt.Sprintf("%s %d%%", hint, int(percent+0.5)))
	})
}
