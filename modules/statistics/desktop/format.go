package statisticsdesktop

import (
	"fmt"
	"math"
	"strconv"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type translateFunc func(key string, args ...any) string

func usageTotal(u *protocol.ConversationUsage) (uint64, bool) {
	if u == nil {
		return 0, false
	}
	return u.Input + u.Output, true
}

func formatCompact(value uint64) string {
	type scale struct {
		unit   uint64
		div    float64
		suffix string
	}
	for _, s := range []scale{
		{1_000_000_000_000, 1e12, "T"},
		{1_000_000_000, 1e9, "B"},
		{1_000_000, 1e6, "M"},
		{1_000, 1e3, "k"},
	} {
		if value >= s.unit {
			v := math.Round((float64(value)/s.div)*10) / 10
			return fmt.Sprintf("%.1f%s", v, s.suffix)
		}
	}
	return strconv.FormatUint(value, 10)
}

func formatGrouped(value uint64) string {
	s := strconv.FormatUint(value, 10)
	if len(s) <= 3 {
		return s
	}
	var b strings.Builder
	b.Grow(len(s) + (len(s)-1)/3)
	rem := len(s) % 3
	if rem > 0 {
		b.WriteString(s[:rem])
		if rem < len(s) {
			b.WriteByte(',')
		}
	}
	for i := rem; i < len(s); i += 3 {
		if i > rem {
			b.WriteByte(',')
		}
		b.WriteString(s[i : i+3])
	}
	return b.String()
}

func ratioPercent(part, whole uint64) (float64, bool) {
	if whole == 0 {
		return 0, false
	}
	return float64(part) * 100 / float64(whole), true
}

// formatAmountUSD formats micro-dollars to 2 decimal places by truncating via
// exact uint64 integer division (e.g. 999999 -> "0.99").
func formatAmountUSD(micros uint64) string {
	return fmt.Sprintf("%d.%02d", micros/1_000_000, (micros%1_000_000)/10_000)
}

// formatPriceUSD formats per-request micro-dollars to 4 decimal places by
// truncating via exact uint64 integer division.
func formatPriceUSD(micros uint64) string {
	return fmt.Sprintf("%d.%04d", micros/1_000_000, (micros%1_000_000)/100)
}

// formatExactUSD formats micro-dollars to full 6-decimal precision.
func formatExactUSD(micros uint64) string {
	return fmt.Sprintf("%d.%06d", micros/1_000_000, micros%1_000_000)
}

func formatCacheHit(usage *protocol.ConversationUsage) (string, bool) {
	if usage == nil || usage.Input == 0 {
		return "", false
	}
	pct := math.Round(100 * float64(usage.CachedInput) / float64(usage.Input))
	return fmt.Sprintf("%.0f%%", pct), true
}

func formatSpeed(throughput *protocol.UsageGeneration) (string, bool) {
	if throughput == nil || throughput.ElapsedUs == 0 {
		return "", false
	}
	rate := math.Round((float64(throughput.OutputTokens)*1_000_000/float64(throughput.ElapsedUs))*10) / 10
	return fmt.Sprintf("%.1f", rate), true
}

func formatDuration(us *uint64, t translateFunc) string {
	if us == nil {
		return t("composer_metric_unknown")
	}
	if *us < 1_000_000 {
		v := math.Round((float64(*us)/1000)*10) / 10
		return t("usage_duration_ms", "value", fmt.Sprintf("%.1f", v))
	}
	v := math.Round((float64(*us)/1_000_000)*100) / 100
	return t("usage_duration_value", "value", fmt.Sprintf("%.2f", v))
}

func formatCost(cost *protocol.UsageCost, responses uint64) (string, bool) {
	if cost == nil {
		return "", false
	}
	s := formatAmountUSD(cost.UsdMicros)
	if cost.Responses < responses {
		s += "+"
	}
	return s, true
}
