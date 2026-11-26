package statisticsdesktop

import (
	"strings"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSummaryOverviewAndPartialHitRate(t *testing.T) {
	// Complete metrics: hit rate computed.
	complete := protocol.UsageMetrics{
		Responses: 4,
		Tokens: &protocol.ConversationUsage{
			Input:       1000,
			Output:      500,
			CachedInput: 600,
			Reasoning:   200,
		},
		Throughput: &protocol.UsageGeneration{
			OutputTokens: 500,
			ElapsedUs:    10_000_000, // 50.0 tok/s
			Responses:    4,
		},
		Cost: &protocol.UsageCost{
			UsdMicros: 999_999,
			Responses: 3, // partial pricing
		},
	}
	ov := computeOverview(complete)
	if !ov.hasTotal || ov.total != 1500 {
		t.Fatalf("total = (%d, %v), want 1500", ov.total, ov.hasTotal)
	}
	if ov.parts != [3]uint64{600, 400, 500} {
		t.Fatalf("parts = %v, want [600 400 500]", ov.parts)
	}
	if !ov.hasHit || int(ov.hit+0.5) != 60 {
		t.Fatalf("hit = (%v, %v), want 60%%", ov.hit, ov.hasHit)
	}
	if !ov.hasSpeed || ov.speed != "50.0" {
		t.Fatalf("speed = (%q, %v), want 50.0", ov.speed, ov.hasSpeed)
	}
	if !ov.hasCost || ov.cost != "0.99" || !ov.partialCost || ov.priced != 3 {
		t.Fatalf("cost = (%q, %v, partial=%v, priced=%d)", ov.cost, ov.hasCost, ov.partialCost, ov.priced)
	}

	// Incomplete token coverage: hit rate must NOT be computed.
	partial := complete
	partial.IncompleteTokenResponses = 1
	ovPartial := computeOverview(partial)
	if ovPartial.hasHit {
		t.Fatalf("partial coverage should hide hit rate, got %v", ovPartial.hit)
	}
	if ovPartial.reported != 3 || ovPartial.responses != 4 {
		t.Fatalf("coverage = %d/%d, want 3/4", ovPartial.reported, ovPartial.responses)
	}
}

func TestTrendBucketsAxisAndIncompleteExclusion(t *testing.T) {
	tr := testT(t, "en")
	// Wednesday 2026-04-08 00:00 UTC and Thursday 2026-04-09 00:00 UTC belong to the same Monday week (2026-04-06).
	wed := time.Date(2026, 4, 8, 0, 0, 0, 0, time.UTC).UnixMilli()
	thu := time.Date(2026, 4, 9, 0, 0, 0, 0, time.UTC).UnixMilli()
	nextMon := time.Date(2026, 4, 13, 0, 0, 0, 0, time.UTC).UnixMilli()

	days := []protocol.UsagePoint{
		{
			StartMs: wed,
			Metrics: protocol.UsageMetrics{
				Responses: 1,
				Tokens:    &protocol.ConversationUsage{Input: 1000, Output: 200, CachedInput: 400},
			},
		},
		{
			StartMs: thu,
			Metrics: protocol.UsageMetrics{
				Responses: 1,
				Tokens:    &protocol.ConversationUsage{Input: 500, Output: 300, CachedInput: 100},
			},
		},
		{
			// Incomplete bucket with huge subtotal: must be excluded from peak and show "—".
			StartMs: nextMon,
			Metrics: protocol.UsageMetrics{
				Responses:                2,
				IncompleteTokenResponses: 1,
				Tokens:                   &protocol.ConversationUsage{Input: 500_000, Output: 500_000},
			},
		},
	}

	trend := buildTrend(days, nil, "week", 0, tr)
	if len(trend.buckets) != 2 {
		t.Fatalf("len(buckets) = %d, want 2", len(trend.buckets))
	}
	// First bucket sums Wed + Thu: cached=500, uncached=1000, output=500 -> total=2000.
	if trend.buckets[0].incomplete || trend.buckets[0].values != [3]float64{500, 1000, 500} {
		t.Fatalf("bucket 0 = %+v", trend.buckets[0])
	}
	if trend.buckets[0].heading != "2026-04-06" && trend.buckets[0].heading != "2026-04-08 – 2026-04-09" {
		t.Fatalf("unexpected weekly heading = %q", trend.buckets[0].heading)
	}
	// Second bucket is incomplete: excluded from peak (so maximum is 2000, not 1M) and shows "—".
	if !trend.buckets[1].incomplete || trend.buckets[1].footerVal != "—" {
		t.Fatalf("bucket 1 should be incomplete with '—', got %+v", trend.buckets[1])
	}
	if trend.maximum != 2000 {
		t.Fatalf("axis maximum = %v, want 2000 (incomplete bucket excluded)", trend.maximum)
	}
}

func TestNiceAxisSteps(t *testing.T) {
	maxVal, ticks := computeAxis(0)
	if maxVal != 0 || len(ticks) != 2 || ticks[0].label != "0" || ticks[1].label != "1" {
		t.Fatalf("zero axis = (%v, %+v)", maxVal, ticks)
	}
	maxVal, ticks = computeAxis(9500)
	// raw = 2375 -> step = 2500 -> divisions = 4 -> max = 10000
	if maxVal != 10000 || len(ticks) != 5 || ticks[1].label != "2.5k" || ticks[4].label != "10k" {
		t.Fatalf("9500 axis = (%v, %+v)", maxVal, ticks)
	}
}

func TestHeatmapDynamicSpanSqrtLevelsAndTop8Models(t *testing.T) {
	tr := testT(t, "en")
	end := time.Date(2026, 4, 30, 0, 0, 0, 0, time.UTC)
	start := end.AddDate(0, 0, -30)
	models := map[string]protocol.ConversationUsage{}
	for i := 0; i < 10; i++ {
		models[string(rune('a'+i))] = protocol.ConversationUsage{Input: uint64((10 - i) * 100)}
	}
	pt := protocol.UsagePoint{
		StartMs: end.AddDate(0, 0, -1).UnixMilli(),
		Metrics: protocol.UsageMetrics{
			Responses: 10,
			Tokens:    &protocol.ConversationUsage{Input: 5500, Output: 0},
		},
		Models: models,
	}
	grid := buildHeatmap([]protocol.UsagePoint{pt}, tr, 360, start.UnixMilli(), end.UnixMilli(), 0)
	if grid.columns != 20 || grid.rows != 7 {
		t.Fatalf("grid dims = %dx%d, want 20x7", grid.columns, grid.rows)
	}
	last := grid.cells[len(grid.cells)-1]
	if last.intensity != 1 {
		t.Fatalf("peak day intensity = %v, want 1", last.intensity)
	}
	if len(last.tip.Rows) != 9 || last.tip.Rows[8].Label != "Other models" || last.tip.Rows[8].Value != "300" {
		t.Fatalf("expected top-8 + Other models, got %+v", last.tip.Rows)
	}
	if heatmapDays(2000) != 366 {
		t.Fatalf("heatmapDays(2000) = %d, want capped at 366", heatmapDays(2000))
	}
}

func TestBreakdownProviderAggregationAndUnassignedMerge(t *testing.T) {
	tr := testT(t, "en")
	provA := protocol.ProviderID(uuid.New())
	sess1 := protocol.SessionID(uuid.New())
	sess2 := protocol.SessionID(uuid.New())
	proj1 := protocol.ProjectID(uuid.New())

	names := usageCatalogNames{
		providers: map[string]string{provA.String(): "OpenAI"},
		projects:  map[string]string{proj1.String(): "Sailry"},
		hosts:     map[string]string{},
	}

	modelGroups := []scopedUsageGroup{
		{
			Group: protocol.UsageGroup{
				Key:     protocol.UsageKey{Kind: "model", Data: protocol.UsageKeyModelPayload{Provider: provA, Model: "gpt-5"}},
				Metrics: protocol.UsageMetrics{Responses: 2, Tokens: &protocol.ConversationUsage{Input: 600, Output: 400, CachedInput: 300}},
			},
		},
		{
			Group: protocol.UsageGroup{
				Key:     protocol.UsageKey{Kind: "model", Data: protocol.UsageKeyModelPayload{Provider: provA, Model: "gpt-5-mini"}},
				Metrics: protocol.UsageMetrics{Responses: 1, Tokens: &protocol.ConversationUsage{Input: 300, Output: 200, CachedInput: 100}},
			},
		},
	}

	// Provider mode aggregates model groups locally into one provider row.
	provRows := buildBreakdown(modelGroups, "provider", names, tr)
	if len(provRows) != 1 || provRows[0].name != "OpenAI" || provRows[0].total != 1500 || provRows[0].responses != 3 {
		t.Fatalf("provider breakdown = %+v", provRows)
	}

	// Project mode merges all session groups into one Unassigned row.
	projGroups := []scopedUsageGroup{
		{
			Group: protocol.UsageGroup{
				Key:     protocol.UsageKey{Kind: "project", Data: proj1},
				Metrics: protocol.UsageMetrics{Responses: 1, Tokens: &protocol.ConversationUsage{Input: 500, Output: 100}},
			},
		},
		{
			Group: protocol.UsageGroup{
				Key:     protocol.UsageKey{Kind: "session", Data: sess1},
				Metrics: protocol.UsageMetrics{Responses: 1, Tokens: &protocol.ConversationUsage{Input: 200, Output: 50}},
			},
		},
		{
			Group: protocol.UsageGroup{
				Key:     protocol.UsageKey{Kind: "session", Data: sess2},
				Metrics: protocol.UsageMetrics{Responses: 2, Tokens: &protocol.ConversationUsage{Input: 300, Output: 50}},
			},
		},
	}
	projRows := buildBreakdown(projGroups, "project", names, tr)
	if len(projRows) != 2 {
		t.Fatalf("expected 2 project rows (Sailry + merged Unassigned), got %+v", projRows)
	}
	// Both have total 600; Sailry sorts before Unassigned alphabetically.
	if projRows[0].name != "Sailry" || projRows[1].name != "Unassigned" || projRows[1].total != 600 || projRows[1].responses != 3 {
		t.Fatalf("unexpected project breakdown: %+v", projRows)
	}
}

func TestRequestRowsWarningsMissingVsZeroAndTSV(t *testing.T) {
	tr := testT(t, "en")
	loc := time.FixedZone("UTC", 0)
	now := time.Date(2026, 4, 10, 15, 0, 0, 0, loc)
	title := "Fix race"

	items := []protocol.UsageRequest{
		{
			Position:            protocol.UsagePosition{TimestampMs: now.Add(-10 * time.Minute).UnixMilli(), Sequence: 1},
			SessionTitle:        &title,
			ScopeName:           "Sailry",
			ProviderName:        "Anthropic",
			Model:               "claude-sonnet",
			Tokens:              protocol.ConversationUsage{Input: 1000, Output: 250, CachedInput: 200, Reasoning: 50},
			CachedInputReported: true,
			UsdMicros:           u64(123_456),
			FirstTokenUs:        u64(11_000_000), // > 10s warning
			ElapsedUs:           u64(15_000_000), // >= 15s warning
		},
		{
			// Missing input & unreported zero cached vs reported zero output.
			Position:            protocol.UsagePosition{TimestampMs: now.Add(-48 * time.Hour).UnixMilli(), Sequence: 2},
			Model:               "gpt-5",
			ProviderName:        "OpenAI",
			Tokens:              protocol.ConversationUsage{Input: 0, Output: 0, CachedInput: 0},
			MissingInputTokens:  true,
			MissingOutputTokens: false,
			CachedInputReported: false,
		},
	}

	rows := formatRequestRows(items, tr, now, loc)
	if len(rows) != 2 {
		t.Fatalf("len(rows) = %d", len(rows))
	}
	r0 := rows[0]
	if r0.cells[0].text != "14:50:00" {
		t.Fatalf("today time = %q, want 14:50:00", r0.cells[0].text)
	}
	if r0.cells[4].text != "20%" || r0.cells[4].tone != toneWarning {
		t.Fatalf("low cache cell = %+v", r0.cells[4])
	}
	if r0.cells[6].tone != toneWarning || r0.cells[7].tone != toneWarning {
		t.Fatalf("expected TTFT & duration warnings: %+v, %+v", r0.cells[6], r0.cells[7])
	}
	if r0.cells[8].text != "$0.1234" || r0.cells[8].tooltip != "0.123456" {
		t.Fatalf("cost cell = %+v", r0.cells[8])
	}

	r1 := rows[1]
	if !strings.HasPrefix(r1.cells[0].text, "04/08 ") {
		t.Fatalf("older date = %q, want 04/08 ...", r1.cells[0].text)
	}
	if r1.cells[3].text != "—" || r1.cells[4].text != "—" || r1.cells[5].text != "0" {
		t.Fatalf("missing vs zero cells: input=%+v cached=%+v output=%+v", r1.cells[3], r1.cells[4], r1.cells[5])
	}

	tsv := formatRequestsTSV(rows, []int{0})
	if !strings.Contains(tsv, "Fix race · Sailry\tclaude-sonnet · Anthropic\t1000\t20%\t250\t") || !strings.HasSuffix(tsv, "\t0.123456") {
		t.Fatalf("unexpected TSV: %q", tsv)
	}
}

func TestUsageStateFiltersPaginationAndRouting(t *testing.T) {
	var home, remote protocol.NodeID
	home[0] = 1
	remote[0] = 2
	projID := protocol.ProjectID(uuid.New())
	provID := protocol.ProviderID(uuid.New())
	clock := time.Date(2026, 4, 10, 12, 0, 0, 0, time.UTC)

	st := newUsagePageState(home, func() time.Time { return clock })
	if st.rangeKey != "30d" || !st.allNodes() {
		t.Fatalf("initial state = %+v", st)
	}

	// Selecting a remote project routes queries to that node.
	remoteProject := makeResourceID(nodeHex(remote), projID.String())
	if !st.selectFilter("project", remoteProject) {
		t.Fatal("expected project filter change")
	}
	if st.allNodes() || st.node != nodeHex(remote) || len(st.query.Projects) != 1 || st.query.Projects[0] != projID {
		t.Fatalf("project routing failed: %+v", st)
	}

	// Selecting a provider from another node while a project is active is rejected.
	if st.selectFilter("provider", makeResourceID(nodeHex(home), provID.String())) {
		t.Fatal("cross-node provider selection under a project should be rejected")
	}
	// Selecting a provider on the same node succeeds.
	if !st.selectFilter("provider", makeResourceID(nodeHex(remote), provID.String())) {
		t.Fatal("same-node provider selection should succeed")
	}

	// Switching breakdown model -> provider stays on Dimension=model (no new query needed).
	if st.selectBreakdown("provider") {
		t.Fatal("model -> provider should aggregate locally without refetch")
	}
	// Switching to project requires refetch with Dimension=project.
	if !st.selectBreakdown("project") || st.query.Dimension != protocol.UsageDimensionProject {
		t.Fatal("provider -> project should trigger refetch with project dimension")
	}

	// Simulate ready report with HasMore to test cursor pagination.
	pos := protocol.UsagePosition{TimestampMs: clock.Add(-time.Hour).UnixMilli(), Node: remote, Sequence: 42}
	gen := st.nextGeneration()
	accepted := st.accept(usageViewState{
		connected: true,
		report: &usageData{
			node:     nodeHex(remote),
			query:    st.query,
			requests: protocol.UsagePage{Items: []protocol.UsageRequest{{Position: pos}}, HasMore: true},
		},
	}, gen)
	if !accepted || !st.ready() {
		t.Fatal("expected ready state after accepting report")
	}
	if !st.turnPage(2) || st.page != 2 || st.query.Before == nil || *st.query.Before != pos {
		t.Fatalf("turnPage(2) failed: page=%d before=%+v", st.page, st.query.Before)
	}
	// Stale generation reply must be ignored.
	if st.accept(usageViewState{connected: true}, gen-1) {
		t.Fatal("stale generation should be rejected")
	}
}
