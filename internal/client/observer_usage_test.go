package client

import (
	"math"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestUsageSummaryMergesIncompleteTokenResponses(t *testing.T) {
	var sources []usageSource
	for index, metrics := range []protocol.UsageMetrics{
		{Responses: 2, IncompleteTokenResponses: 1, Tokens: &protocol.ConversationUsage{Input: 4, Output: 3}},
		{Responses: 3, IncompleteTokenResponses: 3},
		{Responses: 1, Tokens: &protocol.ConversationUsage{}},
		{Responses: 2, IncompleteTokenResponses: 1, Tokens: &protocol.ConversationUsage{Input: 7, Output: 1}},
	} {
		node := protocol.NodeID{byte(index + 1)}
		report := &protocol.UsageReport{
			Node:   node,
			Totals: metrics,
			Days:   []protocol.UsagePoint{{StartMs: 86400000, Metrics: metrics}},
			Recent: []protocol.UsagePoint{{StartMs: 88200000, Metrics: metrics}},
		}
		sources = append(sources, usageSource{Node: node, Entry: node, View: usageView{Report: report}})
	}
	summary, err := summarizeUsage(sources)
	if err != nil || summary == nil || len(summary.Days) != 1 || len(summary.Recent) != 1 {
		t.Fatalf("unexpected merged usage summary: %+v, %v", summary, err)
	}
	want := protocol.UsageMetrics{Responses: 8, IncompleteTokenResponses: 5, Tokens: &protocol.ConversationUsage{Input: 11, Output: 4}}
	for name, got := range map[string]protocol.UsageMetrics{"totals": summary.Totals, "day": summary.Days[0].Metrics, "recent": summary.Recent[0].Metrics} {
		if !reflect.DeepEqual(got, want) {
			t.Errorf("%s metrics = %+v, want %+v", name, got, want)
		}
	}
}

func TestUsageSummaryRejectsIncompleteTokenResponseOverflow(t *testing.T) {
	source := func(incomplete uint64) usageSource {
		return usageSource{View: usageView{Report: &protocol.UsageReport{
			Totals: protocol.UsageMetrics{IncompleteTokenResponses: incomplete},
		}}}
	}
	sources := []usageSource{source(math.MaxUint64 - 1), source(1)}
	summary, err := summarizeUsage(sources)
	if err != nil || summary == nil || summary.Totals.IncompleteTokenResponses != math.MaxUint64 {
		t.Fatalf("usage summary at the counter limit = %+v, %v", summary, err)
	}
	summary, err = summarizeUsage(append(sources, source(1)))
	if summary != nil || faultCode(err) != protocol.ErrorCodeInvalidRequest {
		t.Fatalf("overflowed usage summary = %+v, %v", summary, err)
	}
}
