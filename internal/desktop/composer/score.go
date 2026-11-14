package composer

import (
	"fmt"
	"math"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/chart"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// A score's card is a gauge: the five axes as segments of an open ring, each
// filled to its axis, their names and values around it, and the overall in
// its middle. The controls show it from a help mark after the effort, the
// context bar from the model's figure.

// scoreAxes are the axes' names in Score.Axes's order, clockwise from the
// ring's start.
var scoreAxes = [5]string{"model_score_axis_backend", "model_score_axis_frontend", "model_score_axis_knowledge",
	"model_score_axis_aesthetic", "model_score_axis_maintainability"}

// The gauge's geometry: gaugeWidth by gaugeHeight, the ring's center
// gaugeY down, gaugeRadius out and gaugeThickness thick over gaugeSweep
// radians, its gap at the bottom; the labels gaugeLabel out from the
// center in boxes gaugeLabelWidth wide.
const (
	gaugeWidth      = 300
	gaugeHeight     = 210
	gaugeY          = 132
	gaugeRadius     = 72
	gaugeThickness  = 14
	gaugeSweep      = 1.5 * math.Pi
	gaugePad        = 0.035
	gaugeLabel      = 114
	gaugeLabelWidth = 76
)

// measuredAt is a model's score at an effort: the one measured at it, else
// the model's.
func measuredAt(m Model, effort string) *Score {
	if sc, ok := m.Scores[effort]; ok && sc != nil {
		return sc
	}
	return m.Score
}

// scoreGauge draws sc's gauge.
func scoreGauge(c *ui.Context, sc *Score) {
	p := theme.Of(c)
	start := -gaugeSweep / 2
	segment := gaugeSweep / float64(len(scoreAxes))
	at := func(angle float64, r float32) (float32, float32) {
		return gaugeWidth/2 + r*float32(math.Sin(angle)), gaugeY - r*float32(math.Cos(angle))
	}
	ui.Box(c.Key("gauge")).Size(gaugeWidth, gaugeHeight).Shrink(0).Draw(func(pt *ui.Painter, r ui.Rect) {
		cx, cy := r.X+gaugeWidth/2, r.Y+gaugeY
		for i, axis := range sc.Axes {
			a0 := start + float64(i)*segment
			a1 := a0 + segment
			pt.FillPath(chart.Sector(cx, cy, gaugeRadius-gaugeThickness, gaugeRadius, chart.Slice{Start: a0, End: a1, Pad: gaugePad}), p.Border)
			share := min(max(axis/10, 0), 1)
			if share > 0 && !math.IsNaN(share) {
				pt.FillPath(chart.Sector(cx, cy, gaugeRadius-gaugeThickness, gaugeRadius, chart.Slice{Start: a0, End: a0 + segment*share, Pad: gaugePad}), p.Primary)
			}
		}
	}).Children(func() {
		for i, axis := range scoreAxes {
			x, y := at(start+(float64(i)+0.5)*segment, gaugeLabel)
			ui.Column(c.Key(axis)).Absolute().Left(x - gaugeLabelWidth/2).Top(y - 17).Width(gaugeLabelWidth).AlignItems(ui.Center).Children(func() {
				ui.Text(c, i18n.T(c, axis)).FontSize(12).SingleLine().TextColor(p.MutedForeground)
				ui.Text(c, fmt.Sprintf("%.1f", sc.Axes[i])).FontSize(14).FontFeatures("tnum").TextColor(p.PopoverForeground)
			})
		}
		ui.Column(c.Key("overall")).Absolute().Left(0).Top(gaugeY - 30).Width(gaugeWidth).AlignItems(ui.Center).Children(func() {
			ui.Text(c, fmt.Sprintf("%.1f", sc.Overall)).FontSize(34).LineHeight(1.1).FontFeatures("tnum").TextColor(p.PopoverForeground)
			ui.Text(c, i18n.T(c, "model_score_overall")).FontSize(12).TextColor(p.MutedForeground)
		})
	})
}

// scoreCard is sc's hover card over anchor: the gauge, then the effort it
// was measured at and, outside the demo, that the scores are a preview.
func scoreCard(c *ui.Context, anchor ui.Element, sc *Score, attach func(card ui.Element)) {
	if sc == nil || kit.TipsHidden(c) {
		return
	}
	p := theme.Of(c)
	ui.TooltipBase(c, anchor, func(card ui.Element) {
		kit.Anchored(card, anchor, kit.CardLayer)
		attach(kit.Floating(c, card).Padding(12, 14).Width(gaugeWidth + 28).AlignItems(ui.Center))
		card.Children(func() {
			scoreGauge(c, sc)
			note := ""
			if sc.Tested != "" {
				note = i18n.T(c, "model_score_tested", "effort", sc.Tested)
			}
			if !kit.Demo(c) {
				if note != "" {
					note += " · "
				}
				note += i18n.T(c, "model_score_preview")
			}
			if note != "" {
				ui.Text(c, note).FontSize(12).SingleLine().TextColor(p.MutedForeground)
			}
		})
	})
}

// scoreHelp is the help mark after the controls' effort: a muted question
// mark whose hover shows the score's card above it. It sits 1 DIP under the
// row's middle, on the middle of the effort's lowercase letters rather than
// of their line.
func scoreHelp(c *ui.Context, sc *Score) {
	p := theme.Of(c)
	mark := ui.Box(c.Key("score-help")).Size(20, 20).Shrink(0).Center().Margin(2, 0, 0, 0).
		Label(i18n.T(c, "model_score_overall")).TextColor(p.MutedForeground)
	if mark.Hovered() {
		mark.TextColor(p.Foreground)
	}
	mark.Children(func() { ui.Icon(c, icons.CircleQuestion).FontSize(14) })
	scoreCard(c, mark, sc, func(card ui.Element) {
		kit.Attach(card, mark, kit.Top, 6).Transition(kit.TipMotion(kit.Top))
	})
}

// scoreFigure is the context bar's score: the chosen model's overall at its
// effort, as the usage figures look, with the gauge in its hover card.
func (s *State) scoreFigure(c *ui.Context, ctx *Context) {
	m := s.Chosen(ctx.Models)
	sc := measuredAt(m, s.EffortName(c, m))
	if sc == nil {
		return
	}
	item := ui.Row(c.Key("score")).Gap(4).AlignItems(ui.Center).Shrink(0).Label(i18n.T(c, "model_score_overall"))
	item.Children(func() {
		ui.Icon(c, icons.Target).FontSize(14)
		ui.Text(c, fmt.Sprintf("%.1f", sc.Overall)).FontSize(14).FontFeatures("tnum").SingleLine()
	})
	scoreCard(c, item, sc, func(card ui.Element) {
		card.AttachTo(item, ui.AnchorTopEnd, ui.AnchorBottomEnd).Margin(0, 0, 6, 0).AttachedOrigin().Transition(kit.TipMotion(kit.Top))
	})
}
