package statisticsdesktop

import (
	"fmt"
	"math"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type tokenCoverageInfo struct {
	tokens    *protocol.ConversationUsage
	responses uint64
	reported  uint64
}

func tokenCoverage(m protocol.UsageMetrics) tokenCoverageInfo {
	var reported uint64
	if m.Tokens != nil && m.Responses > m.IncompleteTokenResponses {
		reported = m.Responses - m.IncompleteTokenResponses
	}
	return tokenCoverageInfo{
		tokens:    m.Tokens,
		responses: m.Responses,
		reported:  reported,
	}
}

type overviewSummary struct {
	hasTotal     bool
	total        uint64
	hasParts     bool
	parts        [3]uint64
	shares       [3]float64
	shareKnown   [3]bool
	hasReasoning bool
	reasoning    uint64
	hasHit       bool
	hit          float64
	responses    uint64
	reported     uint64
	hasSpeed     bool
	speed        string
	hasCost      bool
	cost         string
	priced       uint64
	partialCost  bool
}

func computeOverview(totals protocol.UsageMetrics) overviewSummary {
	cov := tokenCoverage(totals)
	out := overviewSummary{
		responses: cov.responses,
		reported:  cov.reported,
	}
	if p, ok := tokenParts(cov.tokens); ok {
		out.hasParts = true
		out.parts = p
		out.total = p[0] + p[1] + p[2]
		out.hasTotal = true
		for i := 0; i < 3; i++ {
			out.shares[i], out.shareKnown[i] = ratioPercent(p[i], out.total)
		}
		out.hasReasoning = true
		out.reasoning = cov.tokens.Reasoning
		if cov.reported == cov.responses {
			out.hit, out.hasHit = ratioPercent(p[0], cov.tokens.Input)
		}
	}
	out.speed, out.hasSpeed = formatSpeed(totals.Throughput)
	if totals.Cost != nil {
		out.hasCost = true
		out.cost = formatAmountUSD(totals.Cost.UsdMicros)
		out.priced = totals.Cost.Responses
		out.partialCost = out.priced < cov.responses
	}
	return out
}

func formatPercentOneDecimal(v float64, ok bool) (string, bool) {
	if !ok {
		return "", false
	}
	rounded := math.Round(v*10) / 10
	return fmt.Sprintf("%.1f%%", rounded), true
}
