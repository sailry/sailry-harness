package conversation

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The context meter's details: the occupancy with its history of steps, the
// session's usage, its active time and its connection.

// historySlots is how many steps the history draws: fixed slots keep the
// bars' width, empty ones a short stub.
const (
	historySlots  = 16
	historyHeight = 34
	historyStub   = 3
)

const contextTPSLabel = "TPS"

// contextFull is every section in a scroll, then Show less.
func contextFull(c *ui.Context, s *ContextMeterState, d *ContextStats) {
	_, window := c.Size()
	ui.Scroll(c.Key("sections")).FillWidth().MaxHeight(min(usagePanelHeight, max(window-usagePanelRoom, usagePanelLeast))).
		PaddingX(18).Children(func() {
		contextOccupancy(c, s, d)
		contextUsage(c, s, d)
		contextTiming(c, d)
		contextConnection(c, d)
	})
	contextToggle(c, s)
}

// contextSection is a section: its title and aside over its rows, a border
// above all but the first.
func contextSection(c *ui.Context, id, title, aside string, first bool, body func()) {
	p := theme.Of(c)
	if !first {
		ui.Divider(c.Key("divider/" + id)).Background(p.Border)
	}
	ui.Column(c.Key("section/"+id)).FillWidth().MinWidth(0).Padding(16, 0, 16, 0).Gap(12).Children(func() {
		ui.Row(c).FillWidth().Gap(12).AlignItems(ui.Center).Children(func() {
			ui.Text(c, title).FontSize(14).FontWeight(500).Grow(1).SingleLine()
			if aside != "" {
				kit.Level(c, ui.Text(c, aside).FontSize(12).TextColor(p.MutedForeground).SingleLine().MinWidth(0).Shrink(1), 12, aside)
			}
		})
		body()
	})
}

// contextRow is a label and its value, with a color dot before it where
// chart is a color, and its note as the tooltip.
func contextRow(c *ui.Context, label, value string, chart int, note string) {
	p := theme.Of(c)
	row := ui.Row(c).FillWidth().MinWidth(0).Height(26).Gap(8).AlignItems(ui.Center)
	if note != "" {
		kit.Tooltip(c, row, note)
	}
	row.Children(func() {
		if chart != 0 {
			ui.Box(c).Size(7, 7).Shrink(0).Radius(2).Background(chartColor(p, chart))
		}
		ui.Text(c, label).FontSize(13).Grow(1).MinWidth(0).SingleLine().TextColor(p.MutedForeground)
		ui.Text(c, value).FontSize(13).MinWidth(0).MaxWidthPercent(65).Shrink(1).FontFeatures("tnum").SingleLine().TextAlign(ui.End)
	})
}

// contextFold is a row that opens rows under it: its label, a value and a
// chevron.
func contextFold(c *ui.Context, s *ContextMeterState, id, label, value string, body func()) {
	p := theme.Of(c)
	open := s.fold(id)
	icon := icons.ChevronRight
	if open {
		icon = icons.ChevronDown
	}
	ui.Column(c.Key("fold/" + id)).FillWidth().MinWidth(0).Children(func() {
		b := ui.ButtonBase(c.Key("toggle")).Label(label).FillWidth().Height(26).Gap(8).AlignItems(ui.Center).
			Cursor(ui.CursorPointer).Expanded(open).TextColor(p.MutedForeground)
		if b.Hovered() {
			b.TextColor(p.Foreground)
		}
		b.OnClick(func() { s.toggle(id) }).Children(func() {
			ui.Text(c, label).FontSize(13).Grow(1).MinWidth(0).SingleLine()
			if value != "" {
				ui.Text(c, value).FontSize(13).FontFeatures("tnum").TextColor(p.Foreground).SingleLine()
			}
			ui.Icon(c, icon).FontSize(14)
		})
		if open {
			ui.Column(c.Key("body")).FillWidth().MinWidth(0).Padding(4, 0, 0, 0).Children(body)
		}
	})
}

// contextFigures are equal columns of a label over a value, those without a
// value left out; each figure's note is its tooltip.
type contextFigure struct {
	label, value, note string
	color              ui.Color
}

func contextFigures(c *ui.Context, id string, size float32, figures []contextFigure) {
	p := theme.Of(c)
	var shown []contextFigure
	for _, f := range figures {
		if f.value != "" {
			shown = append(shown, f)
		}
	}
	if len(shown) == 0 {
		return
	}
	ui.Row(c.Key("figures/" + id)).FillWidth().Gap(12).Children(func() {
		for i, f := range shown {
			cell := ui.Column(c.Key(fmt.Sprint(i))).Grow(1).Basis(0).MinWidth(0).Gap(6)
			if f.note != "" {
				kit.Tooltip(c, cell, f.note)
			}
			color := f.color
			if color == (ui.Color{}) {
				color = p.Foreground
			}
			cell.Children(func() {
				ui.Text(c, f.label).FontSize(12).TextColor(p.MutedForeground).SingleLine()
				ui.Text(c, f.value).FontSize(size).FontWeight(500).FontFeatures("tnum").TextColor(color).SingleLine()
			})
		}
	})
}

// contextBar is a 6 high bar of parts' shares of whole, side by side.
type barPart struct {
	value float64
	color ui.Color
}

func contextBar(c *ui.Context, label string, whole float64, parts []barPart) ui.Element {
	p := theme.Of(c)
	return ui.Box(c).FillWidth().Height(6).Label(label).Draw(func(pt *ui.Painter, r ui.Rect) {
		pt.Fill(r, p.Muted, 3)
		if whole <= 0 {
			return
		}
		pt.Clip(r, 3, func() {
			x := r.X
			for _, part := range parts {
				w := min(r.W*float32(part.value/whole), r.X+r.W-x)
				if w <= 0 {
					continue
				}
				pt.Fill(ui.Rect{X: x, Y: r.Y, W: w, H: r.H}, part.color, 0)
				x += w + 1
			}
		})
	})
}

func (d *ContextStats) occupancyAside(c *ui.Context) string {
	var parts []string
	if d.Model != "" {
		parts = append(parts, d.Model)
	}
	if d.Compactions > 0 {
		parts = append(parts, i18n.T(c, "meter_compactions", "count", Count(int(d.Compactions))))
	}
	return strings.Join(parts, " · ")
}

// contextOccupancy is the window: the tokens used of it and the percent, a
// bar with the compaction line and the output reserve, the room left, the
// history of steps and the categories of the steps, or of the one chosen.
func contextOccupancy(c *ui.Context, s *ContextMeterState, d *ContextStats) {
	p := theme.Of(c)
	roomLimit, roomKey := d.Limit, "meter_left"
	if d.Threshold > 0 {
		roomLimit, roomKey = d.Threshold, "meter_remaining"
	}
	room, reserve := "", ""
	if remaining := d.remaining(roomLimit); remaining != nil {
		room = i18n.T(c, roomKey, "count", Count(int(*remaining)))
	}
	if d.Reserve > 0 {
		reserve = i18n.T(c, "meter_reserve") + " " + Count(int(d.Reserve))
	}
	contextSection(c, "occupancy", i18n.T(c, "meter_title"), d.occupancyAside(c), true, func() {
		head := ui.Row(c.Key("figure")).FillWidth().Gap(5).AlignItems(ui.End)
		basis := "meter_basis_observed"
		if d.Estimated {
			basis = "meter_basis_estimate"
		}
		if d.Tokens != nil {
			kit.Tooltip(c, head, i18n.T(c, basis))
		}
		head.Children(func() {
			ui.Text(c, tokenLabel(d.Tokens, d.Estimated)).FontSize(26).FontWeight(500).FontFeatures("tnum").SingleLine()
			if d.Limit > 0 {
				ui.Text(c, "/ "+Count(int(d.Limit))).FontSize(13).TextColor(p.MutedForeground).Margin(0, 0, 4, 0).SingleLine()
			}
			ui.Box(c).Grow(1)
			if v := d.percent(); v >= 0 {
				ui.Text(c, percentLabel(v)).FontSize(15).FontWeight(500).TextColor(p.MutedForeground).Margin(0, 0, 3, 0).SingleLine()
			}
		})
		ui.Box(c.Key("meter")).FillWidth().Height(12).Label(i18n.T(c, "meter_title")).Draw(func(pt *ui.Painter, r ui.Rect) {
			bar := ui.Rect{X: r.X, Y: r.Y + 3, W: r.W, H: 6}
			pt.Fill(bar, p.Muted, 3)
			if d.Limit == 0 {
				return
			}
			at := func(v uint64) float32 { return r.X + r.W*min(float32(float64(v)/float64(d.Limit)), 1) }
			if d.Tokens != nil && *d.Tokens > 0 {
				pt.Clip(bar, 3, func() { pt.Fill(ui.Rect{X: bar.X, Y: bar.Y, W: max(at(*d.Tokens)-r.X, 3), H: bar.H}, p.Chart[1], 0) })
			}
			if d.Reserve > 0 {
				x := at(d.Limit - min(d.Reserve, d.Limit))
				pt.Line(x, bar.Y, x, bar.Y+bar.H, 1, p.MutedForeground)
			}
			if d.Threshold > 0 {
				x := at(d.Threshold)
				pt.Line(x, r.Y, x, r.Y+r.H, 1, p.MutedForeground)
			}
		})
		if room != "" || reserve != "" {
			ui.Row(c.Key("notes")).FillWidth().Gap(12).Children(func() {
				ui.Text(c, room).FontSize(12).TextColor(p.MutedForeground).Grow(1).SingleLine()
				ui.Text(c, reserve).FontSize(12).TextColor(p.MutedForeground).SingleLine()
			})
		}
		chosen := contextHistory(c, s, d)
		parts, complete := d.Parts, d.Complete
		if chosen != nil {
			parts, complete = chosen.Parts, chosen.Complete
		}
		var shown []ContextPart
		for _, part := range parts {
			if part.Tokens != nil && *part.Tokens > 0 {
				shown = append(shown, part)
			}
		}
		if len(shown) == 0 && d.LastCompaction == nil {
			return
		}
		contextFold(c, s, "occupancy", i18n.T(c, "meter_details"), "", func() {
			for _, part := range shown {
				note := ""
				if part.Identified {
					note = i18n.T(c, "meter_identified_note")
				}
				contextRow(c.Key("part/"+part.Key), i18n.T(c, part.Key), tokenLabel(part.Tokens, true), part.Chart, note)
			}
			if !complete && len(shown) > 0 {
				ui.Text(c.Key("partial"), i18n.T(c, "meter_payload_partial")).FontSize(12).TextColor(p.MutedForeground).Padding(4, 0, 0, 0)
			}
			if record := d.LastCompaction; record != nil {
				outcome := i18n.T(c, "meter_failed")
				if record.Succeeded {
					outcome = i18n.T(c, "meter_succeeded")
				}
				label := i18n.T(c, "meter_last_compaction")
				if record.Reveal == nil {
					contextRow(c.Key("compaction"), label, outcome, 0, "")
					return
				}
				b := ui.ButtonBase(c.Key("compaction")).Label(label).FillWidth().Cursor(ui.CursorPointer)
				b.OnClick(record.Reveal).Children(func() { contextRow(c, label, outcome, 0, "") })
			}
		})
	})
}

// contextHistory draws the latest steps as bars of their tokens against the
// tallest; a click chooses a step for the details, again the whole. It
// returns the step chosen, nil for none.
func contextHistory(c *ui.Context, s *ContextMeterState, d *ContextStats) *ContextStep {
	if len(d.Steps) == 0 {
		return nil
	}
	p := theme.Of(c)
	recent := d.Steps[max(0, len(d.Steps)-historySlots):]
	var chosen *ContextStep
	peak := uint64(1)
	for i := range recent {
		if recent[i].ID == s.Selected {
			chosen = &recent[i]
		}
		if recent[i].Tokens != nil {
			peak = max(peak, *recent[i].Tokens)
		}
	}
	if chosen == nil {
		s.Selected = ""
	}
	fields := func(step *ContextStep) (title, tokens, tps, ttft string) {
		speed, first := contextUnknown, contextUnknown
		if step.Speed != nil {
			speed = fmt.Sprintf("%.1f", *step.Speed)
		}
		if step.FirstTokenUs != nil {
			first = contextDuration(c, *step.FirstTokenUs)
		}
		title = strings.ReplaceAll(i18n.T(c, "meter_step"), "%{count}", fmt.Sprint(step.Step))
		tokens = tokenLabel(step.Tokens, step.Estimated)
		return title, tokens, contextTPSLabel + " " + speed, i18n.T(c, "meter_first_response") + " " + first
	}
	ui.Row(c.Key("history")).FillWidth().Height(historyHeight).Gap(4).AlignItems(ui.End).Children(func() {
		for i := range historySlots {
			if i >= len(recent) {
				ui.Box(c.Key(fmt.Sprint("stub/", i))).Grow(1).Basis(0).Height(historyStub).Radius(1).Background(p.Muted)
				continue
			}
			step := &recent[i]
			color := p.Chart[1]
			if chosen != nil && chosen != step {
				color = p.MutedForeground.Alpha(0.5)
			}
			height := float32(historyStub)
			if step.Tokens != nil && *step.Tokens > 0 {
				height = max(historyStub, float32(*step.Tokens)/float32(peak)*historyHeight)
			}
			title, tokens, tps, ttft := fields(step)
			labels := []string{title, tokens}
			if step.Model != "" {
				labels = append(labels, step.Model)
			}
			label := strings.Join(append(labels, tps, ttft), "  ")
			b := ui.ButtonBase(c.Key("step/" + step.ID)).Label(label).Grow(1).Basis(0).Height(historyHeight).
				Cursor(ui.CursorPointer).AccessSelected(chosen == step)
			kit.ChartTooltip(c, b, stepTip(c, step))
			b.OnClick(func() {
				if s.Selected == step.ID {
					s.Selected = ""
				} else {
					s.Selected = step.ID
				}
			}).Draw(func(pt *ui.Painter, r ui.Rect) {
				pt.Fill(ui.Rect{X: r.X, Y: r.Y + r.H - height, W: r.W, H: height}, color, 2)
			})
		}
	})
	if chosen != nil {
		title, tokens, tps, ttft := fields(chosen)
		field := func(text string) ui.Element {
			return ui.Text(c, text).FontSize(12).TextColor(p.MutedForeground).FontFeatures("tnum").SingleLine()
		}
		ui.Column(c.Key("chosen")).FillWidth().MinWidth(0).Gap(3).Children(func() {
			ui.Row(c.Key("identity")).FillWidth().MinWidth(0).Gap(10).Children(func() {
				field(title).Shrink(0)
				field(tokens).Shrink(0)
				if chosen.Model != "" {
					field(chosen.Model).Shrink(1).MinWidth(0)
				}
			})
			ui.Row(c.Key("timing")).FillWidth().MinWidth(0).Gap(10).Children(func() {
				field(tps)
				field(ttft)
			})
		})
	}
	return chosen
}

// stepTip is a step's hover card: the step over a row for each measured
// field, those not measured left out.
func stepTip(c *ui.Context, step *ContextStep) kit.ChartTip {
	tip := kit.ChartTip{Heading: strings.ReplaceAll(i18n.T(c, "meter_step"), "%{count}", fmt.Sprint(step.Step))}
	if step.Tokens != nil {
		tip.Rows = append(tip.Rows, kit.ChartTipRow{Label: i18n.T(c, "meter_tokens"), Value: tokenLabel(step.Tokens, step.Estimated)})
	}
	if step.Model != "" {
		tip.Rows = append(tip.Rows, kit.ChartTipRow{Label: i18n.T(c, "meter_model"), Value: step.Model})
	}
	if step.Speed != nil {
		tip.Rows = append(tip.Rows, kit.ChartTipRow{Label: contextTPSLabel, Value: fmt.Sprintf("%.1f", *step.Speed)})
	}
	if step.FirstTokenUs != nil {
		tip.Rows = append(tip.Rows, kit.ChartTipRow{Label: i18n.T(c, "meter_first_response"), Value: contextDuration(c, *step.FirstTokenUs)})
	}
	return tip
}

// contextUsage is the session's tokens: input, output and the cache hit
// with its bar, then the tool calls and the subagents, each opening its
// list.
func contextUsage(c *ui.Context, s *ContextMeterState, d *ContextStats) {
	if d.Input == nil && d.Output == nil && d.ToolCalls == nil && d.Children == 0 {
		return
	}
	p := theme.Of(c)
	var aside []string
	if d.Turns != nil {
		aside = append(aside, strings.ReplaceAll(i18n.T(c, "meter_turns"), "%{count}", Count(int(*d.Turns))))
	}
	if d.Cost != "" {
		aside = append(aside, "≈ $"+d.Cost)
	}
	count := func(n *uint64) string {
		if n == nil {
			return ""
		}
		return Count(int(*n))
	}
	hit := -1.0
	if d.Input != nil && *d.Input > 0 && d.Cached != nil {
		hit = min(100, float64(*d.Cached)*100/float64(*d.Input))
	}
	outputNote := i18n.T(c, "meter_output_note")
	if d.Reasoning != nil {
		outputNote = i18n.T(c, "meter_reasoning") + " " + Count(int(*d.Reasoning))
	}
	figures := []contextFigure{
		{label: i18n.T(c, "meter_input"), value: count(d.Input), note: i18n.T(c, "meter_input_note")},
		{label: i18n.T(c, "meter_output"), value: count(d.Output), note: outputNote},
	}
	if hit >= 0 {
		figures = append(figures, contextFigure{label: i18n.T(c, "meter_cache_hit"), value: fmt.Sprintf("%.0f%%", hit),
			note: i18n.T(c, "meter_cache_read") + " " + Count(int(*d.Cached))})
	}
	contextSection(c, "usage", i18n.T(c, "meter_session_usage"), strings.Join(aside, " · "), false, func() {
		contextFigures(c, "usage", 20, figures)
		if hit >= 0 {
			contextBar(c.Key("cache"), i18n.T(c, "meter_cache_hit"), 100, []barPart{{hit, p.Success}})
		}
		if d.ToolCalls == nil && d.Children == 0 {
			return
		}
		ui.Column(c.Key("entries")).FillWidth().Gap(4).Children(func() {
			if d.ToolCalls != nil {
				label, value := i18n.T(c, "meter_tool_calls"), Count(int(*d.ToolCalls))
				if len(d.Tools) == 0 {
					contextRow(c.Key("tools"), label, value, 0, "")
				} else {
					contextFold(c, s, "tools", label, value, func() {
						for _, tool := range d.Tools {
							contextRow(c.Key("tool/"+tool.Name), tool.Name, Count(int(tool.Count)), 0, "")
						}
					})
				}
			}
			if d.Children > 0 {
				contextFold(c, s, "children", i18n.T(c, "meter_subagents"), Count(int(d.Children)), func() {
					contextRow(c.Key("delegated"), i18n.T(c, "meter_child_usage"), Count(int(d.Delegated)), 0, "")
				})
			}
		})
	})
}

// contextTiming is the session's active time: a bar of its phases, their
// legend in pairs and the average speed where the connection shows an API
// response instead of it.
func contextTiming(c *ui.Context, d *ContextStats) {
	var phases []ContextPhase
	for _, phase := range d.Phases {
		if phase.Micros > 0 {
			phases = append(phases, phase)
		}
	}
	if len(phases) == 0 {
		return
	}
	p := theme.Of(c)
	aside, whole := "", 0.0
	if d.Active != nil {
		aside, whole = contextDuration(c, *d.Active), float64(*d.Active)
	} else {
		for _, phase := range phases {
			whole += float64(phase.Micros)
		}
	}
	contextSection(c, "timing", i18n.T(c, "meter_session_time"), aside, false, func() {
		parts := make([]barPart, len(phases))
		for i, phase := range phases {
			parts[i] = barPart{float64(phase.Micros), chartColor(p, phase.Chart)}
		}
		contextBar(c.Key("bar"), i18n.T(c, "meter_session_time"), whole, parts)
		ui.Column(c.Key("legend")).FillWidth().Gap(6).Children(func() {
			item := func(c *ui.Context, phase ContextPhase) {
				ui.Row(c).Grow(1).Basis(0).MinWidth(0).Gap(6).AlignItems(ui.Center).Children(func() {
					ui.Box(c).Size(7, 7).Shrink(0).Radius(2).Background(chartColor(p, phase.Chart))
					ui.Text(c, i18n.T(c, phase.Key)).FontSize(12).TextColor(p.MutedForeground).Grow(1).MinWidth(0).SingleLine()
					ui.Text(c, contextDuration(c, phase.Micros)).FontSize(12).FontFeatures("tnum").SingleLine()
				})
			}
			for i := 0; i < len(phases); i += 2 {
				ui.Row(c.Key(fmt.Sprint("pair/", i))).FillWidth().Gap(16).Children(func() {
					item(c.Key("a"), phases[i])
					if i+1 < len(phases) {
						item(c.Key("b"), phases[i+1])
					} else {
						ui.Box(c.Key("b")).Grow(1).Basis(0)
					}
				})
			}
		})
		if d.Speed != nil && d.Response != nil {
			ui.Text(c.Key("speed"), fmt.Sprintf("%s %.1f %s", contextTPSLabel, *d.Speed, i18n.T(c, "meter_speed_unit"))).
				FontSize(12).TextColor(p.MutedForeground).SingleLine()
		}
	})
}

// Latency colors: an API response over a second is slow and over three very
// slow; a first token after ten seconds is slow and after fifteen very slow.
const (
	responseSlow, responseVerySlow = 1_000_000, 3_000_000
	firstSlow, firstVerySlow       = 10_000_000, 15_000_000
)

func latencyColor(p *theme.Palette, us, slow, verySlow uint64) ui.Color {
	switch {
	case us >= verySlow:
		return p.Danger
	case us > slow:
		return p.Warning
	}
	return p.Success
}

// contextConnection is the requests' averages: the API response, or the
// speed where none is measured, the first token and the request time.
func contextConnection(c *ui.Context, d *ContextStats) {
	p := theme.Of(c)
	var figures []contextFigure
	switch {
	case d.Response != nil:
		figures = append(figures, contextFigure{label: i18n.T(c, "meter_api_response"), value: contextDuration(c, *d.Response),
			note: i18n.T(c, "meter_headers_note"), color: latencyColor(p, *d.Response, responseSlow, responseVerySlow)})
	case d.Speed != nil:
		figures = append(figures, contextFigure{label: contextTPSLabel,
			value: fmt.Sprintf("%.1f %s", *d.Speed, i18n.T(c, "meter_speed_unit"))})
	}
	if d.FirstToken != nil {
		figures = append(figures, contextFigure{label: i18n.T(c, "meter_first_response"), value: contextDuration(c, *d.FirstToken),
			note: i18n.T(c, "meter_first_note"), color: latencyColor(p, *d.FirstToken, firstSlow, firstVerySlow)})
	}
	if d.Elapsed != nil {
		figures = append(figures, contextFigure{label: i18n.T(c, "meter_request_time"), value: contextDuration(c, *d.Elapsed)})
	}
	paired := d.PairedResponse != nil && d.WaitingAfterResponse != nil
	if len(figures) == 0 && !paired {
		return
	}
	contextSection(c, "connection", i18n.T(c, "meter_connection"), "", false, func() {
		contextFigures(c, "connection", 16, figures)
		if paired {
			contextConnectionPath(c, *d.PairedResponse, *d.WaitingAfterResponse)
		}
	})
}

// contextConnectionPath draws the request path as stations joined by links:
// the execution host, the API response and the first output, each link
// labelled with its stage's average from the same requests. The links share
// the width equally, so a zero-length stage still has a readable label.
func contextConnectionPath(c *ui.Context, response, waiting uint64) {
	p := theme.Of(c)
	const station, linkLabel = 28, 16
	stops := []struct {
		icon *ui.SVG
		key  string
	}{{icons.Monitor, "meter_execution_node"}, {icons.Globe, "meter_api_response"}, {icons.Chat, "meter_first_output"}}
	ui.Column(c.Key("connection-path")).FillWidth().MinWidth(0).Gap(6).Children(func() {
		ui.Row(c.Key("stations")).FillWidth().AlignItems(ui.Center).Children(func() {
			for i, stop := range stops {
				ui.Row(c.Key(stop.key)).Size(station, station).Shrink(0).Center().Radius(station/2).
					Border(1, p.Border).Background(p.Muted).Children(func() {
					ui.Icon(c, stop.icon).FontSize(14).TextColor(p.MutedForeground)
				})
				if i == len(stops)-1 {
					break
				}
				value := []uint64{response, waiting}[i]
				color := p.Chart[i+1]
				// The duration sits above its link and an equal space below
				// keeps the link at the stations' centre; the link stops short
				// of both stations.
				ui.Column(c.Key(i)).Grow(1).Basis(0).MinWidth(0).Padding(0, 8).Gap(3).AlignItems(ui.Center).Children(func() {
					ui.Text(c, contextDuration(c, value)).FontSize(12).FontFeatures("tnum").Height(linkLabel).SingleLine()
					ui.Box(c).FillWidth().Height(1.5).Radius(1).Background(color)
					ui.Box(c).Height(linkLabel)
				})
			}
		})
		// Each label lines up with its station: the first and last at the
		// edges, the middle one at the centre where the equal links put it.
		ui.Row(c.Key("labels")).FillWidth().Children(func() {
			for i, stop := range stops {
				align := ui.Center
				switch i {
				case 0:
					align = ui.Start
				case len(stops) - 1:
					align = ui.End
				}
				ui.Text(c.Key(stop.key), i18n.T(c, stop.key)).FontSize(12).TextColor(p.MutedForeground).TextAlign(align).Grow(1).Basis(0).MinWidth(0).SingleLine()
			}
		})
	})
}
