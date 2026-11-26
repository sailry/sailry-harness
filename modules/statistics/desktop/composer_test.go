package statisticsdesktop

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func u64(v uint64) *uint64 { return &v }

func testT(t *testing.T, lang string) translateFunc {
	t.Helper()
	cat := i18n.New(Locales)
	return func(key string, args ...any) string {
		return cat.In(lang, key, args...)
	}
}

func findMetric(metrics []conversation.Metric, id string) (conversation.Metric, bool) {
	for _, m := range metrics {
		if m.ID == id {
			return m, true
		}
	}
	return conversation.Metric{}, false
}

func TestLocalesLoadWithoutCollisions(t *testing.T) {
	cat := i18n.New(Locales)
	if got := cat.In("en", "usage"); got != "Usage" {
		t.Fatalf("en usage = %q", got)
	}
	if got := cat.In("zh-CN", "usage"); got != "用量" {
		t.Fatalf("zh-CN usage = %q", got)
	}
	if got := cat.In("en", "composer_tokens"); got != "Tokens" {
		t.Fatalf("en composer_tokens = %q", got)
	}
	if got := cat.In("zh-CN", "usage_speed"); got != "全程 TPS" {
		t.Fatalf("zh-CN usage_speed = %q", got)
	}
}

func TestMicroDollarTruncation(t *testing.T) {
	if got := formatAmountUSD(999_999); got != "0.99" {
		t.Fatalf("formatAmountUSD(999999) = %q, want 0.99", got)
	}
	if got := formatAmountUSD(1_239_999); got != "1.23" {
		t.Fatalf("formatAmountUSD(1239999) = %q, want 1.23", got)
	}
	if got := formatPriceUSD(123_499); got != "0.1234" {
		t.Fatalf("formatPriceUSD(123499) = %q, want 0.1234", got)
	}
	if got := formatExactUSD(999_999); got != "0.999999" {
		t.Fatalf("formatExactUSD(999999) = %q, want 0.999999", got)
	}
}

func TestRecordedGate(t *testing.T) {
	tr := testT(t, "en")
	if got := buildComposerMetrics(false, nil, tr); len(got) != 0 {
		t.Fatalf("unrecorded nil stats should produce no metrics, got %+v", got)
	}
	if got := buildComposerMetrics(false, &protocol.ConversationStatistics{}, tr); len(got) != 0 {
		t.Fatalf("unrecorded zero stats should produce no metrics, got %+v", got)
	}
	// hasMessages alone activates composer_speed with "—".
	got := buildComposerMetrics(true, nil, tr)
	if len(got) != 1 || got[0].ID != "statistics:composer_speed" || got[0].Value != "—" {
		t.Fatalf("hasMessages with nil stats = %+v", got)
	}
}

func TestComposerMetricsCompleteSession(t *testing.T) {
	tr := testT(t, "en")
	stats := &protocol.ConversationStatistics{
		Turns:     4,
		Responses: 6,
		Usage: &protocol.ConversationUsage{
			Input:       10_000,
			Output:      2_500,
			CachedInput: 7_500,
		},
		Cost: &protocol.UsageCost{
			UsdMicros: 999_999,
			Responses: 6,
		},
		Throughput: &protocol.UsageGeneration{
			OutputTokens: 2_500,
			ElapsedUs:    50_000_000, // 50 tok/s
			Responses:    6,
		},
		Observations: protocol.ConversationObservationSummary{
			Latency: protocol.ConversationObservationLatency{
				FirstTokenUs: u64(450_000), // 450.0 ms
			},
			Recent: []protocol.ConversationObservationRequest{{
				Attempts: []protocol.ConversationObservationAttempt{{
					FirstTokenUs: u64(1_250_000), // 1.25 s
					ElapsedUs:    u64(5_000_000), // 5 s -> 500/5 = 100.0 tok/s
					Usage:        &protocol.ConversationUsage{Input: 1000, Output: 500},
				}},
			}},
		},
		Delegated: protocol.UsageMetrics{
			Responses: 2,
			Tokens: &protocol.ConversationUsage{
				Input:  4_000,
				Output: 1_000,
			},
			Cost: &protocol.UsageCost{
				UsdMicros: 500_000,
				Responses: 1, // partial pricing on subagents
			},
		},
	}

	metrics := buildComposerMetrics(true, stats, tr)
	if len(metrics) != 5 {
		t.Fatalf("len(metrics) = %d, want 5", len(metrics))
	}

	// Tokens: combined 12.5k + 5.0k = 17.5k
	tok, ok := findMetric(metrics, "statistics:composer_tokens")
	if !ok || tok.Order != 0 || tok.Icon != icons.ChartPie || tok.Value != "17.5k" {
		t.Fatalf("unexpected tokens metric: %+v", tok)
	}
	if len(tok.Rows) != 2 || tok.Rows[0].Value != "12.5k" || tok.Rows[1].Value != "5.0k" {
		t.Fatalf("unexpected tokens rows: %+v", tok.Rows)
	}

	// Speed: own throughput only = 50.0 tok/s
	spd, ok := findMetric(metrics, "statistics:composer_speed")
	if !ok || spd.Order != 10 || spd.Icon != icons.Gauge || spd.Value != "50.0 tok/s" {
		t.Fatalf("unexpected speed metric: %+v", spd)
	}
	if len(spd.Rows) != 3 || spd.Rows[0].Value != "450.0 ms" || spd.Rows[1].Value != "100.0 tok/s" || spd.Rows[2].Value != "1.25 s" {
		t.Fatalf("unexpected speed rows: %+v", spd.Rows)
	}

	// Cost: 999999 + 500000 = 1499999 -> truncated 1.49, plus '+' because 7 < 8 priced
	costM, ok := findMetric(metrics, "statistics:composer_cost")
	if !ok || costM.Order != 20 || costM.Icon != icons.DollarSign || costM.Value != "1.49+" {
		t.Fatalf("unexpected cost metric: %+v", costM)
	}
	if len(costM.Rows) != 2 || costM.Rows[0].Value != "$0.99" || costM.Rows[1].Value != "$0.50+" {
		t.Fatalf("unexpected cost rows: %+v", costM.Rows)
	}

	// Cache: own session only = 7500 / 10000 = 75%
	cacheM, ok := findMetric(metrics, "statistics:composer_cache")
	if !ok || cacheM.Order != 30 || cacheM.Icon != icons.MemoryStick || cacheM.Value != "75%" {
		t.Fatalf("unexpected cache metric: %+v", cacheM)
	}
	if len(cacheM.Rows) != 3 || cacheM.Rows[0].Value != "7.5k" || cacheM.Rows[1].Value != "2.5k" || cacheM.Rows[2].Value != "10.0k" {
		t.Fatalf("unexpected cache rows: %+v", cacheM.Rows)
	}

	// Turns: own turns = 4, hover row Responses = 6
	turnsM, ok := findMetric(metrics, "statistics:composer_turns")
	if !ok || turnsM.Order != 40 || turnsM.Icon != icons.MessagesSquare || turnsM.Value != "4" {
		t.Fatalf("unexpected turns metric: %+v", turnsM)
	}
	if len(turnsM.Rows) != 1 || turnsM.Rows[0].Value != "6" {
		t.Fatalf("unexpected turns rows: %+v", turnsM.Rows)
	}
}

func TestComposerMetricsPartialCoverageNeverComputesCacheHitRate(t *testing.T) {
	tr := testT(t, "en")
	stats := &protocol.ConversationStatistics{
		Turns:                    2,
		Responses:                3,
		IncompleteTokenResponses: 1,
		Usage: &protocol.ConversationUsage{
			Input:       2_000,
			Output:      500,
			CachedInput: 1_500,
		},
	}

	metrics := buildComposerMetrics(true, stats, tr)

	tok, ok := findMetric(metrics, "statistics:composer_tokens")
	if !ok || tok.Value != "Subtotal: 2.5k" {
		t.Fatalf("partial tokens = (%+v, %v), want Subtotal: 2.5k", tok, ok)
	}
	if len(tok.Rows) != 3 || tok.Rows[2].Label != "2 of 3 responses reported both token counts" {
		t.Fatalf("expected coverage row in partial tokens, got %+v", tok.Rows)
	}

	// Cache must show "—" when partial token coverage exists, NEVER 75%.
	cacheM, ok := findMetric(metrics, "statistics:composer_cache")
	if !ok || cacheM.Value != "—" {
		t.Fatalf("partial cache = (%+v, %v), want —", cacheM, ok)
	}
	if len(cacheM.Rows) != 4 || cacheM.Rows[3].Label != "2 of 3 responses reported both token counts" {
		t.Fatalf("expected coverage row in partial cache, got %+v", cacheM.Rows)
	}

	// Speed is still visible with "—" when unmeasured.
	spd, ok := findMetric(metrics, "statistics:composer_speed")
	if !ok || spd.Value != "—" {
		t.Fatalf("unmeasured speed = (%+v, %v), want —", spd, ok)
	}
}
