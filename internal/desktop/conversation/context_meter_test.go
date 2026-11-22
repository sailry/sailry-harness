package conversation_test

import (
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func amount(n uint64) *uint64 { return &n }

func meterStats() *conversation.ContextStats {
	speed := 18.8
	return &conversation.ContextStats{
		Model: "claude-sonnet-5-5", Tokens: amount(47_200), Limit: 1_000_000, Threshold: 800_000, Reserve: 128_000,
		Steps: []conversation.ContextStep{
			{ID: "a", Step: 1, Speed: &speed, FirstTokenUs: amount(120_000), Tokens: amount(20_000), Parts: []conversation.ContextPart{{Key: "meter_system", Tokens: amount(5_000), Chart: 4}}},
			{ID: "b", Step: 2, Tokens: amount(47_200), Parts: []conversation.ContextPart{{Key: "meter_system", Tokens: amount(6_000), Chart: 4}}},
		},
		Parts:        []conversation.ContextPart{{Key: "meter_system", Tokens: amount(11_000), Chart: 4}},
		CurrentInput: amount(45_000), CurrentCached: amount(27_000),
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

func clickMeter(t *testing.T, tt *ui.Tester, d *conversation.ContextStats) {
	t.Helper()
	key, _ := d.State()
	name := i18n.In("en", "meter_title") + " · " + i18n.In("en", key)
	if err := tt.Click(name); err != nil {
		t.Fatal(err)
	}
	meterSettle(tt)
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
		{103_700, 1_000_000, 800_000, false, "meter_state_plenty"},
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

func wantMeterRows(t *testing.T, tt *ui.Tester, rows [][2]string) {
	t.Helper()
	var bottom float32
	nodes := tt.AccessAll()
	for _, row := range rows {
		label := i18n.In("en", row[0])
		value := row[1]
		r, ok := tt.Find(label)
		if !ok {
			t.Fatalf("missing label %q in %q", label, tt.Texts())
		}
		if r.Y < bottom {
			t.Errorf("label %q at %v is not below the preceding row", label, r)
		}
		bottom = r.Y + r.H
		if !slices.ContainsFunc(nodes, func(node ui.AccessInfo) bool {
			v := node.Bounds
			return node.Name == value && v.X >= r.X+r.W && v.Y < bottom && v.Y+v.H > r.Y
		}) {
			t.Errorf("missing value %q beside %q in %q", value, label, tt.Texts())
		}
	}
}

func TestContextMeterExpands(t *testing.T) {
	s := &conversation.ContextMeterState{}
	d := meterStats()
	tt := meterTester(t, s, d)
	clickMeter(t, tt, d)
	texts := tt.Texts()
	if !tt.HasText("4.7%") {
		t.Errorf("the summary lacks the occupancy percentage: %q", texts)
	}
	rows := [][2]string{
		{"meter_used_tokens", "47.2k"},
		{"meter_available_tokens", "952.8k"},
		{"meter_window_limit", "1M"},
		{"meter_threshold", "800k"},
		{"meter_model", "claude-sonnet-5-5"},
		{"meter_last_cache", "60% · 27k / 45k"},
		{"meter_total_cache", "50% · 51.4k / 102.7k"},
	}
	wantMeterRows(t, tt, rows)
	for _, key := range []string{"meter_session_usage", "meter_details", "meter_session_time", "meter_connection"} {
		if label := i18n.In("en", key); slices.Contains(texts, label) {
			t.Errorf("the summary shows %q from the details", label)
		}
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
	for _, row := range rows {
		if label := i18n.In("en", row[0]); slices.Contains(texts, label) {
			t.Errorf("the expanded view still shows the summary row %q", label)
		}
	}
	for _, key := range []string{"meter_session_usage", "meter_input", "meter_output", "meter_tool_calls", "meter_session_time", "meter_connection"} {
		if !slices.Contains(texts, i18n.In("en", key)) {
			t.Errorf("the details lack %q: %q", i18n.In("en", key), texts)
		}
	}
	for _, key := range []string{"meter_title", "meter_cache_hit"} {
		label := i18n.In("en", key)
		if bar, ok := tt.AccessByRole(ui.RoleGroup, label); !ok || bar.Offscreen || bar.Bounds.W <= 0 || bar.Bounds.H <= 0 {
			t.Errorf("the details lack the %q bar: %+v", label, bar)
		}
	}
	for _, want := range []string{"claude-sonnet-5-5", "47.2k", "/ 1M", "4.7%", "Until compaction 752.8k", "Output reserve 128k",
		"2 turns · ≈ $0.14", "102.7k", "222", "50%", "1.74 s", "3.81 s", "14.37 s", "12.19 s", "TPS 18.8 tok/s"} {
		if !slices.Contains(texts, want) {
			t.Errorf("the details lack %q: %q", want, texts)
		}
	}
	less, ok := tt.AccessByRole(ui.RoleButton, i18n.In("en", "meter_hide_details"))
	if !ok {
		t.Fatalf("no Show less: %q", texts)
	}
	tt.ClickAt(less.Bounds.X+less.Bounds.W/2, less.Bounds.Y+less.Bounds.H/2)
	meterSettle(tt)
	if s.Expanded {
		t.Error("Show less kept the details")
	}
	wantMeterRows(t, tt, rows)
}

func TestContextMeterWindowEstimates(t *testing.T) {
	for _, tc := range []struct {
		name                   string
		stats                  conversation.ContextStats
		used, available, limit string
	}{
		{"missing usage", conversation.ContextStats{Limit: 1000}, "—", "—", "1k"},
		{"missing limit", conversation.ContextStats{Tokens: amount(300)}, "300", "—", "—"},
		{"estimate", conversation.ContextStats{Tokens: amount(300), Estimated: true, Limit: 1000}, "≈ 300", "≈ 700", "1k"},
		{"over limit", conversation.ContextStats{Tokens: amount(1100), Limit: 1000}, "1.1k", "0", "1k"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			tt := meterTester(t, &conversation.ContextMeterState{}, &tc.stats)
			clickMeter(t, tt, &tc.stats)
			wantMeterRows(t, tt, [][2]string{
				{"meter_used_tokens", tc.used},
				{"meter_available_tokens", tc.available},
				{"meter_window_limit", tc.limit},
				{"meter_threshold", "—"},
				{"meter_model", "—"},
			})
		})
	}
}

func TestContextMeterCacheMeasurements(t *testing.T) {
	for _, tc := range []struct {
		name          string
		input, cached *uint64
		want          string
	}{
		{"missing usage", nil, nil, "—"},
		{"missing cache read", amount(1000), nil, "— · — / 1k"},
		{"missing input", nil, amount(500), "— · 500 / —"},
		{"zero cache read", amount(1000), amount(0), "0.0% · 0 / 1k"},
		{"zero input", amount(0), amount(0), "— · 0 / 0"},
		{"cache exceeds input", amount(1000), amount(1500), "— · 1.5k / 1k"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			d := &conversation.ContextStats{CurrentInput: tc.input, CurrentCached: tc.cached, Input: amount(2000)}
			tt := meterTester(t, &conversation.ContextMeterState{}, d)
			clickMeter(t, tt, d)
			wantMeterRows(t, tt, [][2]string{
				{"meter_last_cache", tc.want},
				{"meter_total_cache", "— · — / 2k"},
			})
		})
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
	if !s.Open || !tt.HasText("47.2k") {
		t.Fatalf("resting on the meter left the summary closed: %q", tt.Texts())
	}
	figure, _ := tt.Access("47.2k")
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
	d := meterStats()
	d.Steps[0].Model = "gpt-6.2-astra"
	tt := meterTester(t, s, d)
	meterSettle(tt)
	stepBar := func() ui.Rect {
		t.Helper()
		step := "Step 1  20k  " + d.Steps[0].Model + "  TPS 18.8  TTFT 120.0 ms"
		bar, ok := tt.Access(step)
		if !ok {
			t.Fatalf("no step bar %q: %q", step, tt.Texts())
		}
		return bar.Bounds
	}
	bar := stepBar()
	tt.ClickAt(bar.X+bar.W/2, bar.Y+bar.H-1)
	tt.Move(0, 0)
	meterSettle(tt)
	if s.Selected != "a" {
		t.Fatalf("the step chosen is %q, want a", s.Selected)
	}
	textBounds := func(text string) ui.Rect {
		t.Helper()
		bounds, ok := tt.Find(text)
		if !ok || bounds.W <= 0 || bounds.H <= 0 {
			t.Fatalf("missing selected step field %q: %q", text, tt.Texts())
		}
		return bounds
	}
	for _, model := range []string{d.Steps[0].Model, strings.Repeat("provider-long-model-", 8)} {
		d.Steps[0].Model = model
		meterSettle(tt)
		title, tokens, modelText := textBounds("Step 1"), textBounds("20k"), textBounds(model)
		tps, ttft := textBounds("TPS 18.8"), textBounds("TTFT 120.0 ms")
		bar = stepBar()
		if title.Y < bar.Y+bar.H || tokens.Y != title.Y || modelText.Y != title.Y || tokens.X < title.X+title.W || modelText.X < tokens.X+tokens.W {
			t.Errorf("identity fields are not ordered on one line below the bar: bar %v, title %v, tokens %v, model %v", bar, title, tokens, modelText)
		}
		if tps.Y < title.Y+title.H || ttft.Y != tps.Y || ttft.X < tps.X+tps.W {
			t.Errorf("TPS and TTFT do not share a second line: title %v, TPS %v, TTFT %v", title, tps, ttft)
		}
		window, ok := tt.AccessByRole(ui.RoleGroup, i18n.In("en", "meter_title"))
		if !ok || modelText.X+modelText.W > window.Bounds.X+window.Bounds.W+1 {
			t.Errorf("model extends beyond the context meter: model %v, context bar %v", modelText, window.Bounds)
		}
	}
	tt.ClickAt(bar.X+bar.W/2, bar.Y+bar.H-1)
	meterSettle(tt)
	if s.Selected != "" {
		t.Errorf("a second click kept step %q", s.Selected)
	}
}

// Resting on a step shows its fields a line each, in order; those not
// measured are left out rather than shown empty.
func TestContextMeterStepTip(t *testing.T) {
	d := meterStats()
	d.Steps[0].Model = "gpt-6.2-astra"
	s := &conversation.ContextMeterState{Open: true, Expanded: true}
	tt := meterTester(t, s, d)
	meterSettle(tt)
	// Texts also holds the bars' accessible names, each step's fields on one
	// line, so the tip is what resting adds to the texts shown before.
	before := tt.Texts()
	added := func() []string {
		rest := map[string]int{}
		for _, text := range before {
			rest[text]++
		}
		var out []string
		for _, text := range tt.Texts() {
			if rest[text] > 0 {
				rest[text]--
				continue
			}
			out = append(out, text)
		}
		slices.Sort(out)
		return out
	}
	rest := func(label string) ui.AccessInfo {
		t.Helper()
		bar, ok := tt.Access(label)
		if !ok {
			t.Fatalf("no step bar %q: %q", label, tt.Texts())
		}
		tt.Move(bar.Bounds.X+bar.Bounds.W/2, bar.Bounds.Y+bar.Bounds.H-1)
		tt.Advance(time.Second)
		tt.Frame()
		return bar
	}
	full := "Step 1  20k  gpt-6.2-astra  TPS 18.8  TTFT 120.0 ms"
	bar := rest(full)
	if want := "Step 1\nTokens 20k\nModel gpt-6.2-astra\nTPS 18.8\nTTFT 120.0 ms"; bar.Description != want {
		t.Errorf("step description %q, want %q", bar.Description, want)
	}
	want := []string{"Step 1", "Tokens", "20k", "Model", "gpt-6.2-astra", "TPS", "18.8", "TTFT", "120.0 ms"}
	if got := added(); !slices.Equal(got, slices.Sorted(slices.Values(want))) {
		t.Fatalf("resting on step 1 added %q, want each field apart: %q", got, want)
	}
	// Each field is a row below the one before, its label beside its value.
	above := float32(-1)
	texts := tt.Texts()
	for _, row := range [][2]string{{"Step 1", ""}, {"Tokens", "20k"}, {"Model", "gpt-6.2-astra"}, {"TPS", "18.8"}, {"", "120.0 ms"}} {
		text := row[1]
		if text == "" {
			text = row[0]
		}
		r, ok := tt.Find(text)
		if !ok {
			t.Fatalf("the tip lacks %q: %q", text, tt.Texts())
		}
		if r.Y <= above {
			t.Errorf("%q is not on a line below the previous field", text)
		}
		above = r.Y
		if row[0] != "" && row[1] != "" {
			i := slices.Index(texts, row[1])
			if i <= 0 || texts[i-1] != row[0] {
				t.Errorf("value %q does not follow its label %q in %q", row[1], row[0], texts)
			}
			if count(texts, row[0]) == 1 {
				if b, _ := tt.Find(row[0]); b.Y != r.Y || b.X >= r.X {
					t.Errorf("value %q at %v is not beside its label %q at %v", row[1], r, row[0], b)
				}
			}
		}
	}
	sparse := rest("Step 2  47.2k  TPS —  TTFT —")
	if want := "Step 2\nTokens 47.2k"; sparse.Description != want {
		t.Errorf("sparse step description %q, want %q", sparse.Description, want)
	}
	if got, want := added(), []string{"47.2k", "Step 2", "Tokens"}; !slices.Equal(got, want) {
		t.Errorf("resting on step 2 added %q, want only its measured fields %q", got, want)
	}
	tt.ClickAt(bar.Bounds.X+bar.Bounds.W/2, bar.Bounds.Y+bar.Bounds.H-1)
	meterSettle(tt)
	if s.Selected != "a" {
		t.Errorf("a click under the tip chose step %q, want a", s.Selected)
	}
}

func TestContextMeterConnectionPath(t *testing.T) {
	for _, tc := range []struct {
		name              string
		response, waiting *uint64
		visible           bool
	}{
		{"zero duration", amount(0), amount(5_960_000), true},
		{"missing paired response", nil, amount(5_960_000), false},
		{"missing waiting duration", amount(0), nil, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			d := meterStats()
			d.PairedResponse, d.WaitingAfterResponse = tc.response, tc.waiting
			tt := meterTester(t, &conversation.ContextMeterState{Open: true, Expanded: true}, d)
			meterSettle(tt)
			texts := tt.Texts()
			for _, label := range []string{i18n.In("en", "meter_execution_node"), i18n.In("en", "meter_first_output"), "0.0 ms", "5.96 s"} {
				if slices.Contains(texts, label) != tc.visible {
					t.Errorf("%q visibility differs from %t: %q", label, tc.visible, texts)
				}
			}
		})
	}
}
