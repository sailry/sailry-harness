package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func pointer[T any](v T) *T { return &v }

// meterStatistics are a session's statistics after two turns, the second
// retried, on a model with a compaction budget.
func meterStatistics() *protocol.ConversationStatistics {
	s := protocol.DefaultConversationStatistics()
	s.Turns, s.Responses = 2, 3
	s.Usage = &protocol.ConversationUsage{Input: 102_700, Output: 222}
	s.ContextTokens, s.ContextLimit, s.ContextModel = pointer(uint64(47_200)), pointer(uint32(1_000_000)), pointer("sonnet")
	s.Budget = &protocol.ConversationCompactionBudget{Model: "sonnet", ContextLimit: 1_000_000, TriggerTokens: 800_000, OutputReserve: 128_000}
	s.Cost = &protocol.UsageCost{UsdMicros: 141_234, Responses: 3}
	s.Generation = &protocol.UsageGeneration{ElapsedUs: 2_000_000, OutputTokens: 40}
	s.Observations.Tools = map[string]uint64{"plugin_0123456789abcdef_read": 2, "read": 1, "mcp_search_0123456789abcdef01234567": 1}
	s.Observations.Phases.WaitingUs = pointer(uint64(1_000))
	request := func(turn byte, index, input uint64) protocol.ConversationObservationRequest {
		r := protocol.DefaultConversationObservationRequest()
		r.Turn, r.TurnIndex, r.Model = protocol.TurnID{turn}, pointer(index), "sonnet"
		attempt := protocol.DefaultConversationObservationAttempt()
		attempt.Usage = &protocol.ConversationUsage{Input: input}
		attempt.Categories = []protocol.ConversationObservationCategory{{Kind: "system", Tokens: pointer(uint64(10))}}
		r.Attempts = []protocol.ConversationObservationAttempt{attempt}
		return r
	}
	unplaced := request(3, 0, 5)
	unplaced.TurnIndex = nil
	s.Observations.Recent = []protocol.ConversationObservationRequest{request(2, 2, 30), request(1, 1, 20), request(2, 2, 47), unplaced}
	return &s
}

// The meter reads the occupancy against the session's own budget, each
// turn's latest request, tools by their own names and the priced cost.
func TestContextStats(t *testing.T) {
	d := contextStats(meterStatistics(), "sonnet", 200_000, false)
	if d.Tokens == nil || *d.Tokens != 47_200 || d.Limit != 1_000_000 || d.Threshold != 800_000 || d.Reserve != 128_000 {
		t.Fatalf("occupancy %v of %d, line %d, reserve %d", d.Tokens, d.Limit, d.Threshold, d.Reserve)
	}
	if len(d.Steps) != 2 || d.Steps[0].Step != 1 || *d.Steps[1].Tokens != 47 {
		t.Errorf("steps %+v, want turn 1 then turn 2's latest request", d.Steps)
	}
	if d.Parts[0].Key != "meter_system" || *d.Parts[0].Tokens != 20 {
		t.Errorf("system across the steps is %+v, want 20", d.Parts[0])
	}
	if len(d.Tools) != 2 || d.Tools[0].Name != "read" || d.Tools[0].Count != 3 || *d.ToolCalls != 4 {
		t.Errorf("tools %+v (%d calls), want read 3 then search 1", d.Tools, *d.ToolCalls)
	}
	if d.Cost != "0.14" || d.Speed == nil || *d.Speed != 20 {
		t.Errorf("cost %q, speed %v", d.Cost, d.Speed)
	}
}

// Another model chosen since the last request leaves its occupancy behind:
// the chosen model's window, unmeasured. A partly priced session has no cost.
func TestContextStatsModelChanged(t *testing.T) {
	s := meterStatistics()
	s.Cost.Responses = 2
	d := contextStats(s, "opus", 200_000, false)
	if d.Tokens != nil || d.Limit != 200_000 || d.Model != "opus" || d.Threshold != 0 {
		t.Errorf("after a change: %v of %d on %q, line %d", d.Tokens, d.Limit, d.Model, d.Threshold)
	}
	if d.Cost != "" {
		t.Errorf("a partly priced session costs %q", d.Cost)
	}
	if empty := contextStats(nil, "opus", 200_000, true); empty.Tokens != nil || empty.Limit != 200_000 || !empty.Compacting {
		t.Errorf("without statistics: %+v", empty)
	}
}
