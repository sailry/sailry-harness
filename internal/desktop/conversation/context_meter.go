package conversation

import (
	"fmt"
	"math"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// ContextStats is what a session's context meter shows: the window's
// occupancy, the session's usage, its time and its connection. A nil amount
// is unmeasured. Durations are in microseconds.
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

	// Total input includes cache reads. Current counts describe one request;
	// Input and Cached cover the session.
	CurrentInput, CurrentCached *uint64

	Turns                            *uint64
	Cost                             string
	Input, Output, Cached, Reasoning *uint64
	Tools                            []ContextTool
	ToolCalls                        *uint64
	Children, Delegated              uint64

	Active *uint64
	Phases []ContextPhase
	Speed  *float64

	Response, FirstToken, Elapsed        *uint64
	PairedResponse, WaitingAfterResponse *uint64
}

// ContextStep is a turn's latest request as a bar of the history.
type ContextStep struct {
	Speed        *float64
	FirstTokenUs *uint64
	ID           string
	Step         uint64
	Tokens       *uint64
	Estimated    bool
	Model        string
	Limit        uint64
	Parts        []ContextPart
	Complete     bool
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
	// The summary opens as the pointer rests on the meter and closes as it
	// leaves the meter and the popover, unless a click pinned it or the
	// details were opened: rest and left are when those began, pinned that
	// a click opened it, over that the pointer was on the popover last frame.
	// A popover closed under the pointer (a click, Escape) stays closed
	// until the pointer leaves (shut); was is whether it was open last frame.
	rest, left              time.Time
	pinned, over, shut, was bool
}

// Hover timing of the meter's popover: the rest before it opens and the
// grace after the pointer leaves, so crossing the gap to it keeps it.
const (
	meterRest  = 250 * time.Millisecond
	meterGrace = 200 * time.Millisecond
)

// hover opens and closes the popover by where the pointer is, inside
// meaning on the meter or the popover.
func (s *ContextMeterState) hover(c *ui.Context, inside bool) {
	now := c.Now()
	closed := s.was && !s.Open
	s.was = s.Open
	if inside {
		s.left = time.Time{}
		if closed {
			s.shut = true
		}
		// Another popover or menu open keeps it closed, as it does tips.
		if s.Open || s.shut || kit.TipsHidden(c) {
			s.rest = time.Time{}
			return
		}
		if s.rest.IsZero() {
			s.rest = now
		}
		if wait := meterRest - now.Sub(s.rest); wait > 0 {
			c.After(wait)
			return
		}
		s.Open, s.pinned = true, false
		return
	}
	s.rest, s.shut = time.Time{}, false
	if !s.Open || s.pinned || s.Expanded {
		s.left = time.Time{}
		return
	}
	if s.left.IsZero() {
		s.left = now
	}
	if wait := meterGrace - now.Sub(s.left); wait > 0 {
		c.After(wait)
		return
	}
	s.Open, s.left, s.was = false, time.Time{}, false
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

func (d *ContextStats) remaining(limit uint64) *uint64 {
	if d.Tokens == nil || limit == 0 {
		return nil
	}
	remaining := limit - min(limit, *d.Tokens)
	return &remaining
}

const contextUnknown = "—"

// ContextMeter is the composer's context indicator left of the model chip.
// It opens a summary of the window and input cache, with session details on
// demand. The toolbar ring is muted until compaction is near.
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
	// The ring needs no hover background or tooltip: resting on it shows
	// the summary itself, and a click pins it open.
	b := ui.ButtonBase(c.Key("context-meter")).Label(hint).Height(24).PaddingX(4).Radius(12).Shrink(0).
		Center().Cursor(ui.CursorPointer)
	s.hover(c, b.Hovered() || s.over)
	b.HasPopup(ui.PopupDialog).Expanded(s.Open).
		Children(func() { kit.Ring(c, percent, d.Compacting, tone, "") }).
		OnClick(func() {
			if s.Open && !s.pinned {
				s.pinned = true
				return
			}
			s.Open, s.pinned = !s.Open, !s.Open
		})
	panel := kit.Popover(c, b, &s.Open, func() {
		if s.Expanded {
			contextFull(c, s, d)
			return
		}
		_, window := c.Size()
		ui.Scroll(c.Key("summary-scroll")).FillWidth().MaxHeight(min(usagePanelHeight, max(window-usagePanelRoom, usagePanelLeast))).
			PaddingX(16).Children(func() {
			contextSummary(c, d)
		})
		contextToggle(c, s)
	})
	if panel != (ui.Element{}) {
		width := float32(320)
		if s.Expanded {
			width = 360
		}
		panel.AttachTo(b, ui.AnchorTopEnd, ui.AnchorBottomEnd).Margin(0, 0, 8, 0).Padding(0).Width(width)
	}
	s.over = panel != (ui.Element{}) && panel.Hovered()
}

func contextSummary(c *ui.Context, d *ContextStats) {
	p := theme.Of(c)
	key, tone := d.State()
	percent, percentText := d.percent(), contextUnknown
	if percent >= 0 {
		percentText = percentLabel(percent)
	}
	basis := ""
	if d.Tokens != nil {
		basis = i18n.T(c, "meter_basis_observed")
		if d.Estimated {
			basis = i18n.T(c, "meter_basis_estimate")
		}
	}
	model := d.Model
	if model == "" {
		model = contextUnknown
	}
	ui.Column(c.Key("summary")).FillWidth().Padding(14, 0).Gap(6).Children(func() {
		ui.Row(c.Key("window")).FillWidth().Gap(14).AlignItems(ui.Center).Children(func() {
			kit.RingSized(c.Key("ring"), 64, 5, max(percent, 0), d.Compacting, tone, i18n.T(c, key)).
				AlignItems(ui.Center).Justify(ui.Center).Children(func() {
				ui.Text(c, percentText).FontSize(14).FontWeight(500).FontFeatures("tnum").SingleLine()
			})
			ui.Column(c.Key("counts")).Grow(1).MinWidth(0).Children(func() {
				contextRow(c.Key("used"), i18n.T(c, "meter_used_tokens"), tokenLabel(d.Tokens, d.Estimated), 0, basis)
				contextRow(c.Key("remaining"), i18n.T(c, "meter_available_tokens"), tokenLabel(d.remaining(d.Limit), d.Estimated), 0, "")
				contextRow(c.Key("limit"), i18n.T(c, "meter_window_limit"), contextBoundLabel(d.Limit), 0, "")
			})
		})
		if d.Compacting {
			ui.Text(c.Key("compacting"), i18n.T(c, key)).FontSize(12).TextColor(p.MutedForeground).SingleLine()
		}
		ui.Divider(c.Key("window-divider")).Background(p.Border)
		ui.Column(c.Key("model")).FillWidth().Children(func() {
			contextRow(c.Key("threshold"), i18n.T(c, "meter_threshold"), contextBoundLabel(d.Threshold), 0, "")
			contextRow(c.Key("name"), i18n.T(c, "meter_model"), model, 0, d.Model)
		})
		ui.Divider(c.Key("cache-divider")).Background(p.Border)
		ui.Column(c.Key("cache")).FillWidth().Children(func() {
			note := i18n.T(c, "meter_cache_read") + " / " + i18n.T(c, "meter_input")
			contextRow(c.Key("current"), i18n.T(c, "meter_last_cache"), contextCacheLabel(d.CurrentInput, d.CurrentCached), 0, note)
			contextRow(c.Key("total"), i18n.T(c, "meter_total_cache"), contextCacheLabel(d.Input, d.Cached), 0, note)
		})
	})
}

func contextBoundLabel(n uint64) string {
	if n == 0 {
		return contextUnknown
	}
	return tokenLabel(&n, false)
}

func contextCacheLabel(input, cached *uint64) string {
	if input == nil && cached == nil {
		return contextUnknown
	}
	rate := contextUnknown
	if input != nil && *input > 0 && cached != nil && *cached <= *input {
		rate = percentLabel(float64(*cached) * 100 / float64(*input))
	}
	return rate + " · " + tokenLabel(cached, false) + " / " + tokenLabel(input, false)
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
		return contextUnknown
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
