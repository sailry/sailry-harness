package conversation

import (
	"cmp"
	"fmt"
	"math"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Metric is a figure of the context bar: its label, icon and value as shown,
// and what its hover card lists under them (conversation/usage.rs Metric,
// usage/external.rs). Rows without a meter come first, the meters after a
// divider. Panel, a package's own details, takes the rows' place; Width is
// the card's, 0 for usageWidth. Order places the figure among those drawn
// side by side, whatever its group.
type Metric struct {
	// ID keys the figure among those of the bar.
	ID    string
	Order int
	Label string
	Icon  *ui.SVG
	Value string
	Rows  []Row
	Width float32
	Panel func(c *ui.Context)
}

// Row is a line of a metric's card: a label and its value, and with Meter a
// bar filled to Progress, 0 to 100.
type Row struct {
	Label, Value string
	Meter        bool
	Progress     float64
}

// Metrics are a sample conversation's tokens, speed, cost, cache and turns.
// A live session's are the packages' (the statistics and footer slots).
func Metrics(c *ui.Context, u *Usage) []Metric {
	if u == nil {
		return nil
	}
	return []Metric{
		{ID: "composer_tokens", Order: 0, Label: i18n.T(c, "composer_tokens"), Icon: icons.ChartPie, Value: Count(u.Tokens)},
		{ID: "composer_speed", Order: 10, Label: i18n.T(c, "composer_speed"), Icon: icons.Gauge, Value: fmt.Sprintf("%.0f/s", u.Speed)},
		{ID: "composer_cost", Order: 20, Label: i18n.T(c, "composer_cost"), Icon: icons.DollarSign, Value: fmt.Sprintf("%.2f", u.Cost)},
		{ID: "composer_cache", Order: 30, Label: i18n.T(c, "composer_cache"), Icon: icons.MemoryStick, Value: Count(u.Cached)},
		{ID: "composer_turns", Order: 40, Label: i18n.T(c, "composer_turns"), Icon: icons.MessagesSquare, Value: fmt.Sprint(u.Turns)},
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

// UsageBar is the context bar's metrics (conversation/live/context.rs
// context_bar, usage.rs render_groups): each a 14 DIP icon and its value in
// muted 14, with a hover card of its label, value and rows. lead, the
// providers' figures, stay side by side at any width; groups, one owner's
// figures each, are one button below the composer's compact width, whose
// popover is open while *open is true and lists the groups divided. Figures
// side by side follow their Order, the groups' mixed as the reference's.
func UsageBar(c *ui.Context, lead []Metric, groups [][]Metric, compact bool, open *bool) {
	total := len(lead)
	for _, group := range groups {
		total += len(group)
	}
	if total == 0 {
		return
	}
	p := theme.Of(c)
	ordered := func(figures []Metric) []Metric {
		slices.SortStableFunc(figures, func(a, b Metric) int { return cmp.Compare(a.Order, b.Order) })
		return figures
	}
	ui.Row(c.Key("usage")).Gap(12).AlignItems(ui.Center).TextColor(p.MutedForeground).Children(func() {
		for _, m := range ordered(slices.Clone(lead)) {
			usageFigure(c.Key("lead/"+m.ID), m)
		}
		if compact {
			usageButton(c, groups, open)
			return
		}
		for _, m := range ordered(slices.Concat(groups...)) {
			usageFigure(c.Key(m.ID), m)
		}
	})
}

// usageFigure is one figure and its hover card over its end.
func usageFigure(c *ui.Context, m Metric) {
	item := ui.Row(c.Key("figure")).Gap(4).AlignItems(ui.Center).Shrink(0)
	item.Children(func() {
		if m.Icon != nil {
			ui.Icon(c, m.Icon).FontSize(14)
		}
		ui.Text(c, m.Value).FontSize(14).SingleLine()
	})
	if kit.TipsHidden(c) {
		return
	}
	ui.TooltipBase(c, item, func(card ui.Element) {
		kit.Anchored(card, item, kit.CardLayer)
		kit.Floating(c, card).AttachTo(item, ui.AnchorTopEnd, ui.AnchorBottomEnd).Margin(0, 0, 6, 0).Padding(12, 14).
			Width(usageCard(m)).AttachedOrigin().Transition(kit.TipMotion(kit.Top))
		card.Children(func() { usageDetails(c, m) })
	})
}

// usageWidth is the figures' card and panel width; usageInset the room at
// their sides.
const (
	usageWidth = 300
	usageInset = 14
)

// usageCard is a figure's card width: the one its package declares, or
// usageWidth.
func usageCard(m Metric) float32 {
	if m.Width > 0 {
		return m.Width
	}
	return usageWidth
}

// usageDetails is a figure's card: its package's own panel, or its label and
// value, the rows under them and the meters after a divider.
func usageDetails(c *ui.Context, m Metric) {
	if m.Panel != nil {
		m.Panel(c)
		return
	}
	p := theme.Of(c)
	line := func(c *ui.Context, label, value string) {
		ui.Row(c).FillWidth().MinWidth(0).Height(26).Gap(16).AlignItems(ui.Center).Children(func() {
			ui.Text(c, label).FontSize(14).SingleLine().Grow(1).MinWidth(0).TextColor(p.MutedForeground)
			ui.Text(c, value).FontSize(14).SingleLine().Shrink(0).FontFeatures("tnum").TextColor(p.PopoverForeground)
		})
	}
	ui.Column(c.Key(m.ID)).FillWidth().MinWidth(0).Children(func() {
		line(c.Key("summary"), m.Label, m.Value)
		meters := 0
		for i, row := range m.Rows {
			if row.Meter {
				meters++
				continue
			}
			line(c.Key(fmt.Sprint("row/", i)), row.Label, row.Value)
		}
		if meters == 0 {
			return
		}
		ui.Divider(c.Key("divider")).Background(p.Border).Margin(10, 0, 12, 0)
		ui.Column(c.Key("meters")).FillWidth().MinWidth(0).Gap(12).Children(func() {
			for i, row := range m.Rows {
				if !row.Meter {
					continue
				}
				ui.Column(c.Key(fmt.Sprint("meter/", i))).FillWidth().MinWidth(0).Gap(6).Children(func() {
					ui.Row(c).FillWidth().MinWidth(0).Gap(12).AlignItems(ui.Center).Children(func() {
						ui.Text(c, row.Label).FontSize(14).SingleLine().Grow(1).MinWidth(0).TextColor(p.PopoverForeground)
						ui.Text(c, row.Value).FontSize(12).SingleLine().Shrink(0).FontFeatures("tnum").TextColor(p.MutedForeground)
					})
					ui.Progress(c, math.Max(0, math.Min(100, row.Progress))/100).FillWidth().Height(4).Label(row.Label)
				})
			}
		})
	})
}

// usageButton is a small round button in muted text with the chart icon and
// the first figure, the tokens. A hover shows every figure and its rows in a
// card, group after group with a divider between; a click holds them in a
// popover (conversation/usage/summary.rs).
func usageButton(c *ui.Context, groups [][]Metric, open *bool) {
	var first *Metric
	for _, group := range groups {
		if len(group) > 0 {
			first = &group[0]
			break
		}
	}
	if first == nil {
		return
	}
	p := theme.Of(c)
	b := kit.GhostLook(c, ui.ButtonBase(c.Key("usage")).Label(i18n.T(c, "composer_statistics")), *open).
		Height(28).PaddingX(8).Gap(4).Radius(14).Shrink(0).TextColor(p.MutedForeground)
	b.HasPopup(ui.PopupDialog).Expanded(*open).OnClick(func() { *open = !*open })
	b.Children(func() {
		ui.Icon(c, icons.ChartPie).FontSize(14)
		ui.Text(c, first.Value).FontSize(14).SingleLine()
	})
	_, window := c.Size()
	figures := func() {
		ui.Scroll(c.Key("figures")).FillWidth().MaxHeight(min(usagePanelHeight, max(window-usagePanelRoom, usagePanelLeast))).Gap(12).Children(func() {
			shown := 0
			for _, group := range groups {
				if len(group) == 0 {
					continue
				}
				if shown > 0 {
					ui.Divider(c.Key(fmt.Sprint("divider/", shown))).Background(p.Border)
				}
				shown++
				for _, m := range group {
					usageDetails(c, m)
				}
			}
		})
	}
	width := usageCard(*first)
	if !*open && !kit.TipsHidden(c) {
		ui.TooltipBase(c, b, func(card ui.Element) {
			kit.Anchored(card, b, kit.CardLayer)
			kit.Floating(c, card).AttachTo(b, ui.AnchorTopEnd, ui.AnchorBottomEnd).Margin(0, 0, 6, 0).Padding(12, usageInset).
				Width(width).AttachedOrigin().Transition(kit.TipMotion(kit.Top))
			card.Children(figures)
		})
	}
	kit.Popover(c, b, open, figures).AttachTo(b, ui.AnchorTopEnd, ui.AnchorBottomEnd).Margin(0, 0, 6, 0).
		Padding(12, usageInset).Width(width)
}

// usagePanelHeight bounds the compact button's panel, which scrolls past it:
// the window's height less usagePanelRoom where that is less, and never
// under usagePanelLeast (summary.rs: the viewport less 80, at most 560).
const (
	usagePanelHeight = 560
	usagePanelRoom   = 80
	usagePanelLeast  = 100
)

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
