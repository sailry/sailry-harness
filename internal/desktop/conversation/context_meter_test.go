package conversation_test

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func amount(n uint64) *uint64 { return &n }

// meterStats is a session halfway to its compaction line with every section
// measured.
func meterStats() *conversation.ContextStats {
	speed := 18.8
	return &conversation.ContextStats{
		Model: "claude-sonnet-5-5", Tokens: amount(47_200), Limit: 1_000_000, Threshold: 800_000, Reserve: 128_000,
		Steps: []conversation.ContextStep{
			{ID: "a", Step: 1, Tokens: amount(20_000), Parts: []conversation.ContextPart{{Key: "meter_system", Tokens: amount(5_000), Chart: 4}}},
			{ID: "b", Step: 2, Tokens: amount(47_200), Parts: []conversation.ContextPart{{Key: "meter_system", Tokens: amount(6_000), Chart: 4}}},
		},
		Parts: []conversation.ContextPart{{Key: "meter_system", Tokens: amount(11_000), Chart: 4}},
		Turns: amount(2), Cost: "0.14", Input: amount(102_700), Output: amount(222), Cached: amount(51_350),
		ToolCalls: amount(3), Tools: []conversation.ContextTool{{Name: "read", Count: 3}},
		Active: amount(14_370_000), Phases: []conversation.ContextPhase{{Key: "meter_wait", Micros: 12_190_000, Chart: 2}},
		Speed: &speed, Response: amount(1_740_000), FirstToken: amount(3_810_000), Elapsed: amount(3_150_000),
	}
}

func meterTester(t *testing.T, s *conversation.ContextMeterState, d *conversation.ContextStats) *ui.Tester {
	t.Helper()
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		ui.Column(c).Fill().Justify(ui.End).AlignItems(ui.End).Padding(24).Children(func() {
			conversation.ContextMeter(c, s, d)
		})
	}, 900, 900)
	tt.SetLocale("en")
	tt.Frame()
	tt.Frame()
	return tt
}

func meterSettle(tt *ui.Tester) {
	for range 4 {
		tt.Advance(80 * time.Millisecond)
		tt.Frame()
	}
}

// The meter's state follows the context used against the compaction line,
// or the window where there is none, and reads as unknown without either.
func TestContextMeterState(t *testing.T) {
	for _, c := range []struct {
		tokens, limit, threshold uint64
		compacting               bool
		want                     string
	}{
		{100, 1000, 800, false, "meter_state_plenty"},
		{500, 1000, 800, false, "meter_state_filling"},
		{700, 1000, 800, false, "meter_state_near"},
		{700, 1000, 0, false, "meter_state_filling"},
		{100, 0, 0, false, "meter_state_unknown"},
		{100, 1000, 800, true, "meter_compacting"},
	} {
		d := &conversation.ContextStats{Tokens: amount(c.tokens), Limit: c.limit, Threshold: c.threshold, Compacting: c.compacting}
		if got, _ := d.State(); got != c.want {
			t.Errorf("%d of %d (line %d): %s, want %s", c.tokens, c.limit, c.threshold, got, c.want)
		}
	}
}

// A click opens the summary: the state, the room before compaction and the
// figures, no ratio of used to window. Show details expands every section,
// Show less returns.
func TestContextMeterExpands(t *testing.T) {
	s := &conversation.ContextMeterState{}
	tt := meterTester(t, s, meterStats())
	name := i18n.In("en", "meter_title") + " · " + i18n.In("en", "meter_state_plenty")
	meter, ok := tt.Access(name)
	if !ok {
		t.Fatalf("no meter %q: %q", name, tt.Texts())
	}
	tt.ClickAt(meter.Bounds.X+meter.Bounds.W/2, meter.Bounds.Y+meter.Bounds.H/2)
	meterSettle(tt)
	texts := tt.Texts()
	for _, want := range []string{"4.7%", i18n.In("en", "meter_state_plenty"), "Until compaction 752.8k", "$0.14", "102.9k", "18.8 tok/s"} {
		if !slices.Contains(texts, want) {
			t.Errorf("the summary lacks %q: %q", want, texts)
		}
	}
	if slices.Contains(texts, "/ 1M") || slices.Contains(texts, i18n.In("en", "meter_session_usage")) {
		t.Errorf("the summary shows the details: %q", texts)
	}
	details, ok := tt.Access(i18n.In("en", "meter_show_details"))
	if !ok {
		t.Fatalf("no Show details: %q", texts)
	}
	tt.ClickAt(details.Bounds.X+details.Bounds.W/2, details.Bounds.Y+details.Bounds.H/2)
	meterSettle(tt)
	if !s.Open || !s.Expanded {
		t.Fatalf("Show details left the popover open %v, expanded %v", s.Open, s.Expanded)
	}
	texts = tt.Texts()
	for _, key := range []string{"meter_session_usage", "meter_session_time", "meter_connection", "meter_hide_details"} {
		if !slices.Contains(texts, i18n.In("en", key)) {
			t.Errorf("the details lack %q: %q", i18n.In("en", key), texts)
		}
	}
	for _, want := range []string{"47.2k", "/ 1M", "1.74 s", "3.81 s", "14.37 s"} {
		if !slices.Contains(texts, want) {
			t.Errorf("the details lack %q: %q", want, texts)
		}
	}
	less, _ := tt.Access(i18n.In("en", "meter_hide_details"))
	tt.ClickAt(less.Bounds.X+less.Bounds.W/2, less.Bounds.Y+less.Bounds.H/2)
	meterSettle(tt)
	if s.Expanded {
		t.Error("Show less kept the details")
	}
}

// Resting on the meter opens the summary without a click, and it stays while
// the pointer crosses to it; leaving both closes it after the grace.
func TestContextMeterHover(t *testing.T) {
	s := &conversation.ContextMeterState{}
	tt := meterTester(t, s, meterStats())
	name := i18n.In("en", "meter_title") + " · " + i18n.In("en", "meter_state_plenty")
	meter, ok := tt.Access(name)
	if !ok {
		t.Fatalf("no meter %q: %q", name, tt.Texts())
	}
	tt.Move(meter.Bounds.X+meter.Bounds.W/2, meter.Bounds.Y+meter.Bounds.H/2)
	tt.Frame()
	if s.Open {
		t.Fatal("the summary opened before the pointer rested")
	}
	meterSettle(tt)
	if !s.Open || !tt.HasText("$0.14") {
		t.Fatalf("resting on the meter left the summary closed: %q", tt.Texts())
	}
	figure, _ := tt.Access("$0.14")
	tt.Move(figure.Bounds.X+2, figure.Bounds.Y+2)
	meterSettle(tt)
	if !s.Open {
		t.Fatal("moving onto the summary closed it")
	}
	tt.Move(4, 4)
	meterSettle(tt)
	if s.Open {
		t.Error("leaving the meter and the summary kept it open")
	}
}

// A step of the history chooses the details' categories; again, the whole.
func TestContextMeterStep(t *testing.T) {
	s := &conversation.ContextMeterState{Open: true, Expanded: true}
	tt := meterTester(t, s, meterStats())
	meterSettle(tt)
	step := "Step 1 · 20k"
	bar, ok := tt.Access(step)
	if !ok {
		t.Fatalf("no step bar %q: %q", step, tt.Texts())
	}
	tt.ClickAt(bar.Bounds.X+bar.Bounds.W/2, bar.Bounds.Y+bar.Bounds.H-1)
	meterSettle(tt)
	if s.Selected != "a" {
		t.Fatalf("the step chosen is %q, want a", s.Selected)
	}
	tt.ClickAt(bar.Bounds.X+bar.Bounds.W/2, bar.Bounds.Y+bar.Bounds.H-1)
	meterSettle(tt)
	if s.Selected != "" {
		t.Errorf("a second click kept step %q", s.Selected)
	}
}
