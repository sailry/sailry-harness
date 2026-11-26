package statisticsdesktop

import (
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	packageName       = "statistics"
	composerTokensID  = "composer_tokens"
	composerSpeedID   = "composer_speed"
	composerCostID    = "composer_cost"
	composerCacheID   = "composer_cache"
	composerTurnsID   = "composer_turns"
	composerTokensOrd = 0
	composerSpeedOrd  = 10
	composerCostOrd   = 20
	composerCacheOrd  = 30
	composerTurnsOrd  = 40
)

type composerTokensSummary struct {
	total     uint64
	hasTotal  bool
	responses uint64
	reported  uint64
	partial   bool
}

func summarizeComposerTokens(usage *protocol.ConversationUsage, responses, incomplete uint64) composerTokensSummary {
	var reported uint64
	if usage != nil && responses > incomplete {
		reported = responses - incomplete
	}
	total, hasTotal := usageTotal(usage)
	return composerTokensSummary{
		total:     total,
		hasTotal:  hasTotal,
		responses: responses,
		reported:  reported,
		partial:   reported < responses,
	}
}

func latestObservationAttempt(stats *protocol.ConversationStatistics) *protocol.ConversationObservationAttempt {
	if stats == nil || len(stats.Observations.Recent) == 0 {
		return nil
	}
	req := &stats.Observations.Recent[len(stats.Observations.Recent)-1]
	if len(req.Attempts) == 0 {
		return nil
	}
	return &req.Attempts[len(req.Attempts)-1]
}

func attemptSpeed(attempt *protocol.ConversationObservationAttempt) (string, bool) {
	if attempt == nil || attempt.ElapsedUs == nil || attempt.Usage == nil {
		return "", false
	}
	elapsed := *attempt.ElapsedUs
	if attempt.FirstTokenUs != nil && *attempt.FirstTokenUs > elapsed {
		return "", false
	}
	return formatSpeed(&protocol.UsageGeneration{
		OutputTokens: attempt.Usage.Output,
		ElapsedUs:    elapsed,
	})
}

func isRecorded(hasMessages bool, stats *protocol.ConversationStatistics) bool {
	if hasMessages {
		return true
	}
	if stats == nil {
		return false
	}
	if stats.Turns > 0 || stats.Responses > 0 || stats.Delegated.Responses > 0 {
		return true
	}
	if stats.Usage != nil && (stats.Usage.Input > 0 || stats.Usage.Output > 0) {
		return true
	}
	if stats.Delegated.Tokens != nil && (stats.Delegated.Tokens.Input > 0 || stats.Delegated.Tokens.Output > 0) {
		return true
	}
	return false
}

// buildComposerMetrics projects conversation statistics into the five composer
// statistics metrics (tokens, speed, cost, cache, turns). Works identically for
// both live sessions and Demo conversations.
func buildComposerMetrics(hasMessages bool, stats *protocol.ConversationStatistics, t translateFunc) []conversation.Metric {
	if !isRecorded(hasMessages, stats) {
		return nil
	}
	unknown := t("composer_metric_unknown")
	var (
		usage      *protocol.ConversationUsage
		throughput *protocol.UsageGeneration
		delegated  protocol.UsageMetrics
		turns      uint64
		responses  uint64
		incomplete uint64
		ownCost    *protocol.UsageCost
		avgTTFT    *uint64
	)
	if stats != nil {
		usage = stats.Usage
		throughput = stats.Throughput
		delegated = stats.Delegated
		turns = stats.Turns
		responses = stats.Responses
		incomplete = stats.IncompleteTokenResponses
		ownCost = stats.Cost
		avgTTFT = stats.Observations.Latency.FirstTokenUs
	}
	latest := latestObservationAttempt(stats)
	var latestTTFT *uint64
	if latest != nil {
		latestTTFT = latest.FirstTokenUs
	}

	subtotal := func(hasVal bool, val uint64, partial bool) (string, bool) {
		if !hasVal {
			return "", false
		}
		if partial {
			return t("usage_token_subtotal", "value", formatCompact(val)), true
		}
		return formatCompact(val), true
	}
	rowOrUnknown := func(key, value string, ok bool) conversation.Row {
		if !ok {
			value = unknown
		}
		return conversation.Row{Label: t(key), Value: value}
	}
	coverageRow := func(reported, totalResp uint64) conversation.Row {
		return conversation.Row{
			Label: t("usage_token_coverage", "reported", formatGrouped(reported), "responses", formatGrouped(totalResp)),
		}
	}
	formatRate := func(rate string, ok bool) string {
		if !ok {
			return unknown
		}
		return rate + " " + t("usage_speed_unit")
	}
	chargeRow := func(key string, cost *protocol.UsageCost, resp uint64) conversation.Row {
		val, ok := formatCost(cost, resp)
		if !ok {
			return conversation.Row{Label: t(key), Value: unknown}
		}
		return conversation.Row{Label: t(key), Value: "$" + val}
	}

	own := summarizeComposerTokens(usage, responses, incomplete)
	child := summarizeComposerTokens(delegated.Tokens, delegated.Responses, delegated.IncompleteTokenResponses)
	combinedHasTokens := own.hasTotal || child.hasTotal
	combinedTokens := own.total + child.total
	partialTokens := own.partial || child.partial

	var metrics []conversation.Metric

	// Order 0: composer_tokens (chart-pie)
	if tokVal, ok := subtotal(combinedHasTokens, combinedTokens, partialTokens); ok {
		ownVal, ownOK := subtotal(own.hasTotal, own.total, own.partial)
		childVal, childOK := subtotal(child.hasTotal, child.total, child.partial)
		rows := []conversation.Row{
			rowOrUnknown("composer_this_session", ownVal, ownOK),
			rowOrUnknown("composer_subagents", childVal, childOK),
		}
		if partialTokens {
			rows = append(rows, coverageRow(own.reported+child.reported, own.responses+child.responses))
		}
		metrics = append(metrics, conversation.Metric{
			ID:    packageName + ":" + composerTokensID,
			Order: composerTokensOrd,
			Label: t("composer_tokens"),
			Icon:  icons.ChartPie,
			Value: tokVal,
			Rows:  rows,
		})
	}

	// Order 10: composer_speed (gauge) — always visible when recorded.
	speedVal, speedOK := formatSpeed(throughput)
	latestSpeedVal, latestSpeedOK := attemptSpeed(latest)
	metrics = append(metrics, conversation.Metric{
		ID:    packageName + ":" + composerSpeedID,
		Order: composerSpeedOrd,
		Label: t("usage_speed"),
		Icon:  icons.Gauge,
		Value: formatRate(speedVal, speedOK),
		Rows: []conversation.Row{
			{Label: t("composer_average_ttft"), Value: formatDuration(avgTTFT, t)},
			{Label: t("composer_latest_tps"), Value: formatRate(latestSpeedVal, latestSpeedOK)},
			{Label: t("composer_latest_ttft"), Value: formatDuration(latestTTFT, t)},
		},
	})

	// Order 20: composer_cost (dollar-sign)
	if ownCost != nil || delegated.Cost != nil {
		var combinedMicros, combinedPriced uint64
		if ownCost != nil {
			combinedMicros += ownCost.UsdMicros
			combinedPriced += ownCost.Responses
		}
		if delegated.Cost != nil {
			combinedMicros += delegated.Cost.UsdMicros
			combinedPriced += delegated.Cost.Responses
		}
		combinedCost := &protocol.UsageCost{UsdMicros: combinedMicros, Responses: combinedPriced}
		costVal, _ := formatCost(combinedCost, responses+delegated.Responses)
		metrics = append(metrics, conversation.Metric{
			ID:    packageName + ":" + composerCostID,
			Order: composerCostOrd,
			Label: t("composer_cost"),
			Icon:  icons.DollarSign,
			Value: costVal,
			Rows: []conversation.Row{
				chargeRow("composer_this_session", ownCost, responses),
				chargeRow("composer_subagents", delegated.Cost, delegated.Responses),
			},
		})
	}

	// Order 30: composer_cache (memory-stick) — own session only; never compute hit rate over partial subtotal.
	if usage != nil {
		var (
			cacheVal string
			cacheOK  bool
		)
		if own.partial {
			cacheVal, cacheOK = unknown, true
		} else {
			cacheVal, cacheOK = formatCacheHit(usage)
		}
		if cacheOK {
			var uncached uint64
			if usage.Input > usage.CachedInput {
				uncached = usage.Input - usage.CachedInput
			}
			rows := []conversation.Row{
				{Label: t("composer_cached_input"), Value: formatCompact(usage.CachedInput)},
				{Label: t("composer_uncached_input"), Value: formatCompact(uncached)},
				{Label: t("composer_input_tokens"), Value: formatCompact(usage.Input)},
			}
			if own.partial {
				rows = append(rows, coverageRow(own.reported, own.responses))
			}
			metrics = append(metrics, conversation.Metric{
				ID:    packageName + ":" + composerCacheID,
				Order: composerCacheOrd,
				Label: t("composer_cache"),
				Icon:  icons.MemoryStick,
				Value: cacheVal,
				Rows:  rows,
			})
		}
	}

	// Order 40: composer_turns (messages-square)
	if stats != nil {
		metrics = append(metrics, conversation.Metric{
			ID:    packageName + ":" + composerTurnsID,
			Order: composerTurnsOrd,
			Label: t("composer_turns"),
			Icon:  icons.MessagesSquare,
			Value: formatCompact(turns),
			Rows: []conversation.Row{
				{Label: t("composer_responses"), Value: formatCompact(responses)},
			},
		})
	}

	return metrics
}
