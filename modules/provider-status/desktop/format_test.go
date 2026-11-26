package providerstatusdesktop

import (
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func f64(v float64) *float64 { return &v }
func i64(v int64) *int64     { return &v }
func bptr(v bool) *bool      { return &v }
func sptr(v string) *string  { return &v }

func testTranslator(t *testing.T, lang string) translateFunc {
	t.Helper()
	cat := i18n.New(Locales)
	return func(key string) string {
		return cat.In(lang, key)
	}
}

func TestLocalesLoadCleanly(t *testing.T) {
	cat := i18n.New(Locales)
	if got := cat.In("en", "provider_status_usage"); got != "Remaining usage" {
		t.Fatalf("en provider_status_usage = %q", got)
	}
	if got := cat.In("zh-CN", "provider_status_usage"); got != "剩余用量" {
		t.Fatalf("zh-CN provider_status_usage = %q", got)
	}
}

func TestFormatAmount(t *testing.T) {
	cases := []struct {
		in   float64
		want string
	}{
		{0, "0"},
		{-0.001, "0"},
		{100, "100"},
		{12.50, "12.5"},
		{12.34, "12.34"},
		{12.345, "12.35"},
		{0.05, "0.05"},
		{99.999, "100"},
	}
	for _, tc := range cases {
		if got := formatAmount(tc.in); got != tc.want {
			t.Errorf("formatAmount(%v) = %q, want %q", tc.in, got, tc.want)
		}
	}
}

func TestRemainingPercent(t *testing.T) {
	if _, ok := remainingPercent(nil); ok {
		t.Fatal("nil used should not be measured")
	}
	cases := []struct {
		used float64
		want float64
	}{
		{0, 100},
		{27.5, 72.5},
		{100, 0},
		{135, 0},
		{-10, 100},
	}
	for _, tc := range cases {
		got, ok := remainingPercent(f64(tc.used))
		if !ok || got != tc.want {
			t.Errorf("remainingPercent(%v) = (%v, %v), want (%v, true)", tc.used, got, ok, tc.want)
		}
	}
}

func TestFormatResetTime(t *testing.T) {
	loc := time.FixedZone("UTC+8", 8*3600)
	ts := time.Date(2026, 4, 9, 13, 5, 42, 0, loc).UnixMilli()
	if got := formatResetTime(ts, loc); got != "2026-04-09 13:05" {
		t.Fatalf("formatResetTime = %q, want %q", got, "2026-04-09 13:05")
	}
}

func TestBuildFooterAndProviderMetrics(t *testing.T) {
	loc := time.FixedZone("UTC", 0)
	tr := testTranslator(t, "en")
	reset5h := time.Date(2026, 5, 1, 12, 30, 0, 0, loc).UnixMilli()
	reset7d := time.Date(2026, 5, 7, 0, 0, 0, 0, loc).UnixMilli()

	snapshot := &protocol.ProviderAccountUsage{
		Windows: []protocol.ProviderAccountUsageWindow{
			{Key: "five_hour", Used: f64(35.5), ResetMs: i64(reset5h)},
			{Key: "seven_day", Used: f64(115), ResetMs: i64(reset7d)},
			{Key: "seven_day_sonnet", Used: f64(20)},
		},
		Credits: &protocol.ProviderAccountUsageCredits{Balance: f64(42.5)},
		Extra: &protocol.ProviderAccountUsageExtra{
			Used:     f64(1250),
			Limit:    f64(5000),
			Currency: sptr("USD"),
			Enabled:  bptr(true),
		},
	}

	footer, ok := buildFooterMetric(snapshot, tr, loc)
	if !ok {
		t.Fatal("expected visible footer metric")
	}
	if footer.ID != "provider-status:usage" || footer.Order != 10 || footer.Icon != icons.Gauge {
		t.Fatalf("unexpected footer header: %+v", footer)
	}
	// Minimum remaining across 64.5%, 0% (clamped from -15%), 80% is 0%.
	if footer.Value != "0%" {
		t.Fatalf("footer.Value = %q, want 0%%", footer.Value)
	}
	// 3 meters + 2 resets + credits + extra = 7 rows.
	if len(footer.Rows) != 7 {
		t.Fatalf("len(footer.Rows) = %d, want 7: %+v", len(footer.Rows), footer.Rows)
	}
	if footer.Rows[0].Label != "5h" || footer.Rows[0].Value != "64.5%" || !footer.Rows[0].Meter || footer.Rows[0].Progress != 64.5 {
		t.Fatalf("unexpected row 0: %+v", footer.Rows[0])
	}
	if footer.Rows[1].Label != "Reset time" || footer.Rows[1].Value != "2026-05-01 12:30" || footer.Rows[1].Meter {
		t.Fatalf("unexpected row 1: %+v", footer.Rows[1])
	}
	if footer.Rows[2].Label != "7d" || footer.Rows[2].Value != "0%" || !footer.Rows[2].Meter || footer.Rows[2].Progress != 0 {
		t.Fatalf("unexpected row 2: %+v", footer.Rows[2])
	}
	if footer.Rows[5].Label != "Account credits" || footer.Rows[5].Value != "42.5" {
		t.Fatalf("unexpected credits row: %+v", footer.Rows[5])
	}
	if footer.Rows[6].Label != "Extra usage" || footer.Rows[6].Value != "12.5 / 50 USD" {
		t.Fatalf("unexpected extra row: %+v", footer.Rows[6])
	}

	// Provider metric must keep ONLY meters and credits (omitting resets and extra).
	providerMetric, ok := buildProviderMetric(snapshot, tr, loc)
	if !ok {
		t.Fatal("expected visible provider metric")
	}
	if len(providerMetric.Rows) != 4 {
		t.Fatalf("len(providerMetric.Rows) = %d, want 4: %+v", len(providerMetric.Rows), providerMetric.Rows)
	}
	for _, r := range providerMetric.Rows {
		if r.Label == "Reset time" || r.Label == "Extra usage" {
			t.Fatalf("provider rows should omit resets and extra, got %+v", r)
		}
	}

	// Provider inline lines show at most 2 meters.
	ch := buildChannelUsage(snapshot, tr, loc)
	if ch == nil {
		t.Fatal("expected ChannelUsage")
	}
	if len(ch.Lines) != 2 {
		t.Fatalf("len(ch.Lines) = %d, want 2: %+v", len(ch.Lines), ch.Lines)
	}
	if ch.Lines[0].Label != "5h" || ch.Lines[0].Value != "64.5%" || !ch.Lines[0].Meter {
		t.Fatalf("unexpected line 0: %+v", ch.Lines[0])
	}
	if ch.Lines[1].Label != "7d" || ch.Lines[1].Value != "0%" || !ch.Lines[1].Meter {
		t.Fatalf("unexpected line 1: %+v", ch.Lines[1])
	}
}

func TestCreditsFallbackAndHiddenEmpty(t *testing.T) {
	tr := testTranslator(t, "en")
	loc := time.UTC

	// Empty snapshot is hidden.
	if _, ok := buildFooterMetric(nil, tr, loc); ok {
		t.Fatal("nil snapshot should be hidden")
	}
	if _, ok := buildFooterMetric(&protocol.ProviderAccountUsage{}, tr, loc); ok {
		t.Fatal("empty snapshot should be hidden")
	}
	// Extra alone without measured windows or credits has no summary value -> hidden.
	if _, ok := buildFooterMetric(&protocol.ProviderAccountUsage{
		Extra: &protocol.ProviderAccountUsageExtra{Used: f64(100), Limit: f64(1000)},
	}, tr, loc); ok {
		t.Fatal("extra-only snapshot without windows or credits should be hidden")
	}

	// Unlimited credits fallback when no windows are measured.
	unlimited := &protocol.ProviderAccountUsage{
		Credits: &protocol.ProviderAccountUsageCredits{Unlimited: bptr(true)},
	}
	footer, ok := buildFooterMetric(unlimited, tr, loc)
	if !ok || footer.Value != "Unlimited" {
		t.Fatalf("unlimited footer = (%+v, %v), want Unlimited", footer, ok)
	}
	ch := buildChannelUsage(unlimited, tr, loc)
	if ch == nil || len(ch.Lines) != 1 || ch.Lines[0].Meter || ch.Lines[0].Value != "Unlimited" {
		t.Fatalf("unlimited channel usage = %+v", ch)
	}
}
