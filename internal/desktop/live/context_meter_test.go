package live

import (
	"encoding/json"
	"path/filepath"
	"testing"
	"testing/synctest"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func pointer[T any](v T) *T { return &v }

// meterStatistics are a session's statistics after two turns, the second
// retried, on a model with a compaction budget.
func meterStatistics() *protocol.ConversationStatistics {
	s := protocol.DefaultConversationStatistics()
	s.Turns, s.Responses = 2, 3
	s.Usage = &protocol.ConversationUsage{Input: 102_700, Output: 222, CachedInput: 51_350}
	s.Observations.CacheRead = &s.Usage.CachedInput
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
	unplaced := request(3, 0, 45_000)
	unplaced.TurnIndex = nil
	last := unplaced.Attempts[0]
	last.CacheReported, last.Usage.CachedInput = true, 27_000
	unplaced.Attempts = append(request(3, 0, 5).Attempts, last)
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

func TestContextStatsUnknownCacheUsage(t *testing.T) {
	s := meterStatistics()
	last := &s.Observations.Recent[3].Attempts[1]
	last.Usage.CachedInput = 0
	latest := contextStats(s, "sonnet", 0, false)
	if latest.CurrentCached == nil || *latest.CurrentCached != 0 {
		t.Fatalf("reported zero cache reads = %v", latest.CurrentCached)
	}

	last.CacheReported = false
	s.Observations.CacheRead = nil
	unknown := contextStats(s, "sonnet", 0, false)
	if unknown.CurrentInput == nil || *unknown.CurrentInput != 45_000 || unknown.CurrentCached != nil || unknown.Cached != nil {
		t.Fatalf("unreported cache reads: %+v", unknown)
	}

	last.Usage = nil
	s.Observations.CacheRead = &s.Usage.CachedInput
	s.IncompleteTokenResponses = 1
	partial := contextStats(s, "sonnet", 0, false)
	if partial.CurrentInput != nil || partial.CurrentCached != nil || partial.Cached != nil || partial.Input == nil || *partial.Input != 102_700 {
		t.Fatalf("incomplete token coverage: %+v", partial)
	}
}

func TestContextMeterFollowsConversationUpdates(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		inventory, history := liveChildFixture()
		stats := meterStatistics()
		stats.ContextTokens = pointer(uint64(790_000))
		inventory.Sessions[0].Config.Model = *stats.ContextModel
		history.Statistics = stats
		id := history.Page.Session
		fixture, source, _ := childTestRuntime(t, inventory, history)
		r, err := NewWithPreferences(t.Context(), fixture.controller, preferences.Open(filepath.Join(t.TempDir(), "preferences.json")))
		if err != nil {
			t.Fatal(err)
		}
		defer r.Close()
		tester := ui.NewTesterWith(func(c *ui.Context) {
			theme.Apply(c, "", theme.Dark)
			r.beginConversationFrame()
			cv := r.App.Data.Find(id.String())
			comp := r.App.Composer(cv.ID)
			r.conversation(c, cv, comp)
			ui.Column(c).Fill().Justify(ui.End).AlignItems(ui.End).Padding(24).Children(func() {
				if comp.Indicator != nil {
					comp.Indicator(c)
				}
			})
			r.finishFrame()
			r.finishConversationFrame()
		}, 640, 400, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
		tester.SetLocale("en")
		tester.Frame()
		wantState := func(key string) {
			t.Helper()
			synctest.Wait()
			tester.Frame()
			label := i18n.In("en", "meter_title") + " · " + i18n.In("en", key)
			if _, ok := tester.Find(label); !ok {
				t.Fatalf("missing context meter state %q in %+v", label, tester.AccessAll())
			}
		}
		sequence := history.Sequence
		apply := func(kind string, data any) {
			sequence++
			source.chat.updates <- protocol.Update{Kind: "conversation_frame", Data: protocol.ConversationFrame{
				Node: inventory.Node, Session: id, Sequence: sequence, Change: protocol.ConversationChange{Kind: kind, Data: data},
			}}
		}
		wantState("meter_state_near")
		if err := tester.Click(i18n.In("en", "meter_title") + " · " + i18n.In("en", "meter_state_near")); err != nil {
			t.Fatal(err)
		}
		for range 4 {
			tester.Advance(80 * time.Millisecond)
			tester.Frame()
		}
		for _, value := range []string{"60% · 27k / 45k", "50% · 51.4k / 102.7k"} {
			if !tester.HasText(value) {
				t.Errorf("missing cache usage %q in %q", value, tester.Texts())
			}
		}
		run := history.Page.Runs[0]
		draft := protocol.ConversationDraft{ID: "compaction", Turn: run.Turn, Author: "sailry", Parts: []protocol.ConversationPart{
			{Kind: "resource", Data: json.RawMessage(`{"type":"compacting","started_ms":1}`)},
		}}
		apply("delta", draft)
		wantState("meter_compacting")
		apply("entry", protocol.ConversationEntry{
			ID: draft.ID, Sequence: 2, Turn: run.Turn, Author: "system", Parts: []protocol.ConversationPart{{Kind: "compaction", Data: "Summary"}},
		})
		wantState("meter_state_near")

		draft.ID = "cancelled-compaction"
		apply("delta", draft)
		wantState("meter_compacting")
		run.Status, run.FinishedMs = protocol.ConversationStatusCancelled, pointer(int64(2))
		apply("run", run)
		wantState("meter_state_near")
	})
}

func TestContextStatsPairedLatency(t *testing.T) {
	s := meterStatistics()
	latency := &s.Observations.Latency
	latency.ResponseUs = pointer(uint64(19))
	latency.FirstTokenUs = pointer(uint64(100))
	latency.PairedResponseUs = pointer(uint64(37))
	latency.WaitingUs = pointer(uint64(0))
	d := contextStats(s, "sonnet", 200_000, false)
	if d.Response != latency.ResponseUs || d.FirstToken != latency.FirstTokenUs || d.PairedResponse != latency.PairedResponseUs || d.WaitingAfterResponse != latency.WaitingUs {
		t.Fatalf("latency projection differs from observations: %+v", d)
	}
}

func TestContextStepTimingUsesSelectedAttempt(t *testing.T) {
	s := meterStatistics()
	old := &s.Observations.Recent[0].Attempts[0]
	old.FirstTokenUs, old.ElapsedUs = pointer(uint64(3)), pointer(uint64(8))
	request := &s.Observations.Recent[2]
	latest := request.Attempts[0]
	latest.FirstTokenUs, latest.ElapsedUs = pointer(uint64(500_000)), pointer(uint64(2_500_000))
	latest.Usage = &protocol.ConversationUsage{Output: 150, Reasoning: 30}
	request.Attempts = []protocol.ConversationObservationAttempt{*old, latest}
	steps := contextSteps(s.Observations.Recent)
	if steps[0].Speed != nil || steps[0].FirstTokenUs != nil || steps[1].Speed == nil || *steps[1].Speed != 60 || steps[1].FirstTokenUs == nil || *steps[1].FirstTokenUs != 500_000 {
		t.Fatalf("per-step request timing = %+v", steps)
	}
	request.Attempts[1].Usage = nil
	steps = contextSteps(s.Observations.Recent)
	if steps[1].Speed != nil || steps[1].FirstTokenUs == nil || *steps[1].FirstTokenUs != 500_000 {
		t.Fatal("first-token measurement must survive missing usage without borrowing speed from a retry")
	}
}
