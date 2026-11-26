package statisticsdesktop

import (
	"fmt"
	"math"
	"strconv"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	lowCacheThreshold = 50
	slowFirstTokenUs  = uint64(10_000_000)
	slowResponseUs    = uint64(15_000_000)
)

type requestCellTone int

const (
	toneNormal requestCellTone = iota
	toneMuted
	toneWarning
)

type requestCell struct {
	text    string
	tone    requestCellTone
	tooltip string
}

type requestRowView struct {
	id    string
	cells [9]requestCell
	tsv   [9]string
}

func formatWhen(ms int64, now time.Time, loc *time.Location) string {
	if loc == nil {
		loc = time.Local
	}
	at := time.UnixMilli(ms).In(loc)
	cur := now.In(loc)
	clock := fmt.Sprintf("%02d:%02d:%02d", at.Hour(), at.Minute(), at.Second())
	if at.Year() == cur.Year() && at.Month() == cur.Month() && at.Day() == cur.Day() {
		return clock
	}
	return fmt.Sprintf("%02d/%02d %s", int(at.Month()), at.Day(), clock)
}

func formatExactTime(ms int64, loc *time.Location) string {
	if loc == nil {
		loc = time.Local
	}
	at := time.UnixMilli(ms).In(loc)
	_, offsetSec := at.Zone()
	sign := "+"
	if offsetSec < 0 {
		sign = "-"
		offsetSec = -offsetSec
	}
	offMin := offsetSec / 60
	zone := fmt.Sprintf("%s%02d:%02d", sign, offMin/60, offMin%60)
	return fmt.Sprintf("%04d/%02d/%02d %02d:%02d:%02d.%03d %s",
		at.Year(), int(at.Month()), at.Day(),
		at.Hour(), at.Minute(), at.Second(), at.Nanosecond()/1_000_000, zone)
}

// requestCacheHit returns the floored whole-percent cache hit rate, or false
// when input or cached count is absent or input is zero.
func requestCacheHit(input, cached *uint64) (int, bool) {
	if input == nil || cached == nil || *input == 0 {
		return 0, false
	}
	c := min(*cached, *input)
	r, ok := ratioPercent(c, *input)
	if !ok {
		return 0, false
	}
	return int(math.Floor(r)), true
}

func isLateStart(firstTokenUs *uint64) bool {
	return firstTokenUs != nil && *firstTokenUs > slowFirstTokenUs
}

func isLongResponse(elapsedUs *uint64) bool {
	return elapsedUs != nil && *elapsedUs >= slowResponseUs
}

func formatRequestRows(items []protocol.UsageRequest, t translateFunc, now time.Time, loc *time.Location) []requestRowView {
	unknown := t("composer_metric_unknown")
	exactUint := func(v *uint64) string {
		if v == nil {
			return unknown
		}
		return strconv.FormatUint(*v, 10)
	}
	compactUint := func(v *uint64) string {
		if v == nil {
			return unknown
		}
		return formatCompact(*v)
	}
	timingUs := func(v *uint64) string {
		if v == nil {
			return unknown
		}
		return t("usage_duration_us", "value", strconv.FormatUint(*v, 10))
	}

	out := make([]requestRowView, len(items))
	for i, req := range items {
		var inputPtr, outputPtr, cachedPtr *uint64
		if !req.MissingInputTokens {
			v := req.Tokens.Input
			inputPtr = &v
		}
		if !req.MissingOutputTokens {
			v := req.Tokens.Output
			outputPtr = &v
		}
		if req.Tokens.CachedInput > 0 || req.CachedInputReported {
			v := req.Tokens.CachedInput
			cachedPtr = &v
		}
		reasoning := req.Tokens.Reasoning

		rate, hasRate := requestCacheHit(inputPtr, cachedPtr)
		lowCache := hasRate && rate < lowCacheThreshold

		cacheTipLines := []string{fmt.Sprintf("%s: %s", t("usage_column_cached"), exactUint(cachedPtr))}
		if req.CacheWrite != nil {
			cacheTipLines = append(cacheTipLines, fmt.Sprintf("%s: %s", t("usage_cache_write"), exactUint(req.CacheWrite)))
		}

		title := t("usage_untitled")
		titleTone := toneMuted
		if req.SessionTitle != nil && *req.SessionTitle != "" {
			title = *req.SessionTitle
			titleTone = toneNormal
		}
		scope := req.ScopeName
		if scope == "" {
			scope = t("usage_unassigned")
		}

		whenText := formatWhen(req.Position.TimestampMs, now, loc)
		fullTime := formatExactTime(req.Position.TimestampMs, loc)
		firstUs := timingUs(req.FirstTokenUs)
		elapsedUs := timingUs(req.ElapsedUs)

		exactCost := unknown
		costCell := requestCell{text: unknown, tone: toneMuted, tooltip: unknown}
		if req.UsdMicros != nil {
			exactCost = formatExactUSD(*req.UsdMicros)
			costCell = requestCell{
				text:    "$" + formatPriceUSD(*req.UsdMicros),
				tone:    toneNormal,
				tooltip: exactCost,
			}
		}

		rateText := unknown
		cachedCell := requestCell{text: unknown, tone: toneMuted, tooltip: strings.Join(cacheTipLines, "\n")}
		if hasRate {
			rateText = fmt.Sprintf("%d%%", rate)
			tone := toneNormal
			if lowCache {
				tone = toneWarning
			}
			cachedCell = requestCell{
				text:    rateText,
				tone:    tone,
				tooltip: strings.Join(cacheTipLines, "\n"),
			}
		}

		firstCell := requestCell{text: unknown, tone: toneMuted, tooltip: fmt.Sprintf("%s: %s", t("usage_column_first_token"), firstUs)}
		if req.FirstTokenUs != nil {
			tone := toneNormal
			if isLateStart(req.FirstTokenUs) {
				tone = toneWarning
			}
			firstCell.text = formatDuration(req.FirstTokenUs, t)
			firstCell.tone = tone
		}

		durCell := requestCell{text: unknown, tone: toneMuted, tooltip: fmt.Sprintf("%s: %s", t("usage_column_duration"), elapsedUs)}
		if req.ElapsedUs != nil {
			tone := toneNormal
			if isLongResponse(req.ElapsedUs) {
				tone = toneWarning
			}
			durCell.text = formatDuration(req.ElapsedUs, t)
			durCell.tone = tone
		}

		inputTone := toneNormal
		if inputPtr == nil {
			inputTone = toneMuted
		}
		outputTone := toneNormal
		if outputPtr == nil {
			outputTone = toneMuted
		}

		out[i] = requestRowView{
			id: fmt.Sprintf("%s:%d:%d", req.Position.Node.String(), req.Position.TimestampMs, req.Position.Sequence),
			cells: [9]requestCell{
				{text: whenText, tone: toneMuted, tooltip: fullTime},
				{text: title, tone: titleTone, tooltip: title + "\n" + scope},
				{text: req.Model, tone: toneNormal, tooltip: fmt.Sprintf("%s: %s\n%s: %s", t("usage_column_model"), req.Model, t("usage_column_provider"), req.ProviderName)},
				{text: compactUint(inputPtr), tone: inputTone, tooltip: fmt.Sprintf("%s: %s", t("usage_column_input"), exactUint(inputPtr))},
				cachedCell,
				{text: compactUint(outputPtr), tone: outputTone, tooltip: fmt.Sprintf("%s: %s\n%s: %d", t("usage_column_output"), exactUint(outputPtr), t("usage_column_reasoning"), reasoning)},
				firstCell,
				durCell,
				costCell,
			},
			tsv: [9]string{
				fullTime,
				title + " · " + scope,
				req.Model + " · " + req.ProviderName,
				exactUint(inputPtr),
				rateText,
				exactUint(outputPtr),
				firstUs,
				elapsedUs,
				exactCost,
			},
		}
	}
	return out
}

func formatRequestsTSV(rows []requestRowView, indices []int) string {
	lines := make([]string, 0, len(indices))
	for _, idx := range indices {
		if idx >= 0 && idx < len(rows) {
			lines = append(lines, strings.Join(rows[idx].tsv[:], "\t"))
		}
	}
	return strings.Join(lines, "\n")
}
