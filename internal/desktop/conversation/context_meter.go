package conversation

import (
	"fmt"
	"math"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// ContextStats is what a session's context meter shows: the window's
// occupancy, the session's usage, its time and its connection. A nil amount
// is one not measured, which the meter leaves out rather than showing as 0.
// Durations are in microseconds.
type ContextStats struct {
	Model string
	// Tokens is the context used, estimated where no request reported it.
	Tokens    *uint64
	Estimated bool
	// Limit is the model's window, Threshold the size that starts automatic
	// compaction and Reserve the room kept for output; 0 where unknown.
	Limit, Threshold, Reserve uint64
	Compacting                bool
	Compactions               uint64
	// Steps are each turn's latest request, oldest first; Parts the context
	// categories summed over them.
	Steps    []ContextStep
	Parts    []ContextPart
	Complete bool
	// LastCompaction is the latest compaction, nil for none; Reveal scrolls
	// the transcript to it, nil where it cannot.
	LastCompaction *ContextCompaction

	Turns                            *uint64
	Cost                             string
	Input, Output, Cached, Reasoning *uint64
	Tools                            []ContextTool
	ToolCalls                        *uint64
	Children, Delegated              uint64

	Active *uint64
	Phases []ContextPhase
	Speed  *float64

	Response, FirstToken, Elapsed *uint64
}

// ContextStep is a turn's latest request as a bar of the history.
type ContextStep struct {
	ID        string
	Step      uint64
	Tokens    *uint64
	Estimated bool
	Model     string
	Limit     uint64
	Parts     []ContextPart
	Complete  bool
}

// ContextPart is a category of the context: its label key, tokens and chart
// color (theme Chart index). Identified estimates the source text it found.
type ContextPart struct {
	Key        string
	Tokens     *uint64
	Chart      int
	Identified bool
}

// ContextTool is a tool and how often the session called it.
type ContextTool struct {
	Name  string
	Count uint64
}

// ContextPhase is a part of the session's active time: its label key,
// microseconds and chart color, -1 for the muted one.
type ContextPhase struct {
	Key    string
	Micros uint64
	Chart  int
}

// ContextCompaction is the latest compaction's outcome.
type ContextCompaction struct {
	Succeeded bool
	Reveal    func()
}

// ContextMeterState is a meter's popover: whether it shows, whether it shows
// the details, the history step chosen and the folds open.
type ContextMeterState struct {
	Open, Expanded bool
	Selected       string
	folds          map[string]bool
}

func (s *ContextMeterState) fold(id string) bool { return s.folds[id] }

func (s *ContextMeterState) toggle(id string) {
	if s.folds == nil {
		s.folds = map[string]bool{}
	}
	s.folds[id] = !s.folds[id]
}

// percent is the part of the window used, -1 where either is unknown.
func (d *ContextStats) percent() float64 {
	if d.Tokens == nil || d.Limit == 0 {
		return -1
	}
	return float64(*d.Tokens) * 100 / float64(d.Limit)
}

// State is the occupancy in a word, as its i18n key, and its tone: measured
// against the compaction threshold where there is one, else against the
// window. The meter's name is the title and that word.
func (d *ContextStats) State() (string, kit.Tone) {
	switch {
	case d.Compacting:
		return "meter_compacting", kit.Info
	case d.Tokens == nil || d.Limit == 0:
		return "meter_state_unknown", kit.Neutral
	}
	whole := d.Limit
	if d.Threshold > 0 && d.Threshold < whole {
		whole = d.Threshold
	}
	switch f := float64(*d.Tokens) / float64(whole); {
	case f < 0.5:
		return "meter_state_plenty", kit.Info
	case f < 0.8:
		return "meter_state_filling", kit.Info
	case d.percent() >= 100:
		return "meter_state_near", kit.Negative
	}
	return "meter_state_near", kit.Caution
}

// remaining is the room before compaction, or before the window's end
// without a threshold; "" where either is unknown.
func (d *ContextStats) remaining(c *ui.Context) string {
	if d.Tokens == nil {
		return ""
	}
	room := func(whole uint64) string { return Count(int(whole - min(whole, *d.Tokens))) }
	switch {
	case d.Threshold > 0:
		return strings.ReplaceAll(i18n.T(c, "meter_remaining"), "%{count}", room(d.Threshold))
	case d.Limit > 0:
		return strings.ReplaceAll(i18n.T(c, "meter_left"), "%{count}", room(d.Limit))
	}
	return ""
}

// ContextMeter is the composer's context indicator left of the model chip:
// Kit's ring of the window used, which opens the session's context and usage
// in a popover. The popover shows a summary first: a gauge with the state in
// a word and the room left, the cost, tokens and speed; Show details expands
// it to every section. The toolbar ring is muted until compaction is near.
func ContextMeter(c *ui.Context, s *ContextMeterState, d *ContextStats) {
	if d == nil {
		return
	}
	key, tone := d.State()
	if tone == kit.Info {
		tone = kit.Neutral
	}
	hint := i18n.T(c, "meter_title") + " · " + i18n.T(c, key)
	percent := max(d.percent(), 0)
	open := s.Open
	b := kit.GhostLook(c, ui.ButtonBase(c.Key("context-meter")), open).Label(hint).Height(24).PaddingX(4).Radius(12).Shrink(0)
	if !open {
		kit.Tooltip(c, b, hint)
	}
	b.HasPopup(ui.PopupDialog).Expanded(open).
		Children(func() { kit.Ring(c, percent, d.Compacting, tone, "") }).
		OnClick(func() { s.Open = !s.Open })
	panel := kit.Popover(c, b, &s.Open, func() {
		if s.Expanded {
			contextFull(c, s, d)
			return
		}
		contextSummary(c, d)
		contextToggle(c, s)
	})
	if panel != (ui.Element{}) {
		width := float32(300)
		if s.Expanded {
			width = 360
		}
		panel.AttachTo(b, ui.AnchorTopEnd, ui.AnchorBottomEnd).Margin(0, 0, 8, 0).Padding(0).Width(width)
	}
}

// contextSummary is the popover's first view: the gauge with the percent
// in it beside the state and the room left, then the cost, the tokens and
// the speed as figures, those unknown left out.
func contextSummary(c *ui.Context, d *ContextStats) {
	p := theme.Of(c)
	key, tone := d.State()
	color := p.Foreground
	switch tone {
	case kit.Caution:
		color = p.Warning
	case kit.Negative:
		color = p.Danger
	}
	ui.Row(c.Key("summary")).FillWidth().Padding(16, 16, 14, 16).Gap(14).AlignItems(ui.Center).Children(func() {
		label := "—"
		if v := d.percent(); v >= 0 {
			label = percentLabel(v)
		}
		gaugeTone := tone
		if tone == kit.Neutral {
			gaugeTone = kit.Info
		}
		kit.RingSized(c, 56, 5, max(d.percent(), 0), d.Compacting, gaugeTone, i18n.T(c, key)).
			AlignItems(ui.Center).Justify(ui.Center).Children(func() {
			ui.Text(c, label).FontSize(13).FontWeight(500).FontFeatures("tnum").SingleLine()
		})
		ui.Column(c).Grow(1).MinWidth(0).Gap(3).Children(func() {
			ui.Text(c, i18n.T(c, key)).FontSize(15).FontWeight(500).TextColor(color).SingleLine()
			if room := d.remaining(c); room != "" {
				ui.Text(c, room).FontSize(13).TextColor(p.MutedForeground).FontFeatures("tnum").SingleLine()
			}
			if d.Model != "" {
				kit.Level(c, ui.Text(c, d.Model).FontSize(12).TextColor(p.MutedForeground).SingleLine().MinWidth(0), 12, d.Model)
			}
		})
	})
	type figure struct{ label, value string }
	var figures []figure
	if d.Cost != "" {
		figures = append(figures, figure{i18n.T(c, "meter_cost"), "$" + d.Cost})
	}
	if d.Input != nil {
		total := *d.Input
		if d.Output != nil {
			total += *d.Output
		}
		figures = append(figures, figure{i18n.T(c, "meter_tokens"), Count(int(total))})
	}
	if d.Speed != nil {
		figures = append(figures, figure{i18n.T(c, "meter_speed"), fmt.Sprintf("%.1f %s", *d.Speed, i18n.T(c, "meter_speed_unit"))})
	}
	if len(figures) == 0 {
		return
	}
	ui.Divider(c.Key("figures-divider")).Background(p.Border).Margin(0, 16, 0, 16)
	ui.Row(c.Key("figures")).FillWidth().Padding(12, 16, 12, 16).Gap(12).Children(func() {
		for i, f := range figures {
			ui.Column(c.Key(fmt.Sprint("figure/", i))).Grow(1).Basis(0).MinWidth(0).Gap(4).Children(func() {
				ui.Text(c, f.label).FontSize(12).TextColor(p.MutedForeground).SingleLine()
				ui.Text(c, f.value).FontSize(16).FontWeight(500).FontFeatures("tnum").SingleLine()
			})
		}
	})
}

// contextToggle is the popover's last row: Show details under the summary,
// Show less under the details.
func contextToggle(c *ui.Context, s *ContextMeterState) {
	p := theme.Of(c)
	label, icon := i18n.T(c, "meter_show_details"), icons.ChevronDown
	if s.Expanded {
		label, icon = i18n.T(c, "meter_hide_details"), icons.ChevronUp
	}
	ui.Divider(c.Key("toggle-divider")).Background(p.Border)
	b := ui.ButtonBase(c.Key("details")).Label(label).FillWidth().Height(36).PaddingX(16).Gap(6).AlignItems(ui.Center).
		Cursor(ui.CursorPointer).Expanded(s.Expanded).TextColor(p.MutedForeground)
	if b.Hovered() {
		b.TextColor(p.Foreground)
	}
	b.OnClick(func() { s.Expanded = !s.Expanded }).Children(func() {
		ui.Text(c, label).FontSize(13).Grow(1).SingleLine()
		ui.Icon(c, icon).FontSize(14)
	})
}

// percentLabel writes a share with one decimal under 10%, whole above.
func percentLabel(v float64) string {
	if v < 10 {
		return fmt.Sprintf("%.1f%%", v)
	}
	return fmt.Sprintf("%.0f%%", math.Min(v, 999))
}

// contextDuration writes microseconds as the statistics do: milliseconds
// under a second, seconds under a minute, then minutes and seconds.
func contextDuration(c *ui.Context, us uint64) string {
	switch {
	case us < 1_000_000:
		return strings.ReplaceAll(i18n.T(c, "meter_duration_ms"), "%{value}", fmt.Sprintf("%.1f", float64(us)/1000))
	case us < 60_000_000:
		return strings.ReplaceAll(i18n.T(c, "meter_duration_s"), "%{value}", fmt.Sprintf("%.2f", float64(us)/1_000_000))
	}
	return strings.NewReplacer("%{minutes}", fmt.Sprint(us/60_000_000), "%{seconds}", fmt.Sprint(us/1_000_000%60)).
		Replace(i18n.T(c, "meter_minutes"))
}

// tokenLabel is a token count, marked ≈ where estimated, — where unknown.
func tokenLabel(n *uint64, estimated bool) string {
	if n == nil {
		return "—"
	}
	if estimated {
		return "≈ " + Count(int(*n))
	}
	return Count(int(*n))
}

// chartColor is a ContextPart's or ContextPhase's color: a chart color by its
// index from 1, the muted text color for -1.
func chartColor(p *theme.Palette, i int) ui.Color {
	if i < 1 || i > theme.ChartColors {
		return p.MutedForeground
	}
	return p.Chart[i-1]
}
