package engine

import (
	"context"
	"encoding/json"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/tokens"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestContextTokensAfterRollingCompaction(t *testing.T) {
	t.Parallel()
	turn := protocol.NewTurnID()
	old := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Earlier task")), manualNow())
	tail := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Recent evidence")), manualNow())
	stale := uint64(90000)
	tail.Message().Usage = &canonical.Usage{Total: &stale}
	summary := canonical.NewRecord(turn, "compaction", &canonical.Compaction{Boundary: old.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Current handoff"))}, manualNow())
	history := []canonical.Record{old, tail, summary}
	want := tokens.Text("Current handoff") + tokens.Text("Recent evidence") + 14
	if got := contextTokens(history); got != want {
		t.Fatalf("context = %d, want summary plus retained tail %d", got, want)
	}
	measured := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "New answer")), manualNow())
	total := uint64(1234)
	measured.Message().Usage = &canonical.Usage{Total: &total}
	growth := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Continue")), manualNow())
	history = append(history, measured, growth)
	want = total + tokens.Text("Continue") + 7
	if got := contextTokens(history); got != want {
		t.Fatalf("context = %d, want fresh measurement plus pending input %d", got, want)
	}
}

// A reply whose usage sums several requests counts the window its last
// request occupied, not the sum.
func TestContextTokensOfAReplyThatTookSeveralRequests(t *testing.T) {
	t.Parallel()
	turn := protocol.NewTurnID()
	reply := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Answer")), manualNow())
	input, output, total, window := uint64(22000), uint64(700), uint64(22700), uint64(12300)
	reply.Message().Usage = &canonical.Usage{Input: &input, Output: &output, Total: &total}
	reply.Metadata.Execution = &canonical.Execution{ContextTokens: &window}
	if got := contextTokens([]canonical.Record{reply}); got != window {
		t.Fatalf("context = %d, want the last request's %d", got, window)
	}
	single := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Answer")), manualNow())
	single.Message().Usage = &canonical.Usage{Input: &input, Output: &output, Total: &total}
	single.Metadata.Execution = &canonical.Execution{ContextTokens: &total}
	if got := contextTokens([]canonical.Record{single}); got != total {
		t.Fatalf("single request context = %d, want %d", got, total)
	}
}

func TestContextTokensIgnoresInterruptedSteerAndEmptyUsage(t *testing.T) {
	t.Parallel()
	turn := protocol.NewTurnID()
	binding := model.Binding{
		Provider: protocol.ConversationProvider{
			ID:             protocol.NewProviderID(),
			Name:           "Fixture",
			Api:            protocol.ConversationModelApiChatCompletions,
			Authentication: protocol.AuthenticationApiKey,
		},
		Config: protocol.SessionConfig{Model: "fixture-model"},
	}
	scope, mainPurpose, compactPurpose := model.Scope(binding), canonical.PurposeMain, canonical.PurposeCompaction

	// Seed a large history whose un-folded token estimate far exceeds the
	// last measured context usage (reproducing the 201.8k vs 276.7k jump).
	bulky := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, strings.Repeat("history evidence token ", 15000))), manualNow())
	measuredTotal, cacheRead := uint64(201842), uint64(199040)
	measured := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Checked")), manualNow())
	measured.Message().Usage = &canonical.Usage{Total: &measuredTotal, CacheRead: &cacheRead}
	measured.Metadata.Execution = &canonical.Execution{Source: &scope, Purpose: &mainPurpose}
	steer := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Continue with the small fix")), manualNow())
	steer.Metadata.Runtime = &canonical.Runtime{UserVisible: true, AgentVisible: true, Steer: true}

	baseline := contextTokens([]canonical.Record{bulky, measured, steer})
	wantBaseline := measuredTotal + pendingTokens([]canonical.Record{steer})
	if baseline != wantBaseline {
		t.Fatalf("baseline contextTokens = %d, want %d", baseline, wantBaseline)
	}

	// Case A: legacy persisted hidden cancelled observation with empty "usage":{}
	hiddenCancelled := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant), manualNow())
	hiddenCancelled.Message().Usage = &canonical.Usage{}
	hiddenCancelled.Metadata.Runtime = &canonical.Runtime{UserVisible: false, AgentVisible: false}
	obsReq := protocol.DefaultConversationObservationRequest()
	obsReq.ID = protocol.NewRequestID().String()
	obsReq.Turn = turn
	obsReq.Model = binding.Config.Model
	obsAtt := protocol.DefaultConversationObservationAttempt()
	obsAtt.Index = 1
	obsAtt.Outcome = protocol.ConversationObservationOutcomeCancelled
	obsReq.Attempts = []protocol.ConversationObservationAttempt{obsAtt}
	hiddenCancelled.Metadata.Execution = &canonical.Execution{
		Source:      &scope,
		Purpose:     &mainPurpose,
		Observation: &obsReq,
	}
	rawHidden, err := json.Marshal(hiddenCancelled)
	if err != nil {
		t.Fatal(err)
	}
	var reloadedHidden canonical.Record
	if err := json.Unmarshal(rawHidden, &reloadedHidden); err != nil {
		t.Fatal(err)
	}
	if got := contextTokens([]canonical.Record{bulky, measured, reloadedHidden, steer}); got != wantBaseline {
		t.Fatalf("hidden cancelled empty usage jumped contextTokens to %d, want %d", got, wantBaseline)
	}
	if !servedFromCache(binding, []canonical.Record{bulky, measured, reloadedHidden, steer}) {
		t.Fatal("hidden cancelled attempt broke servedFromCache")
	}

	// Case B: visible interrupted assistant reply with empty usage or partial Input only
	interruptedFinish := canonical.Interrupted
	partialInput := uint64(201850)
	for _, usage := range []*canonical.Usage{{}, {Input: &partialInput}} {
		interrupted := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Partial")), manualNow())
		interrupted.Message().Finish = &interruptedFinish
		interrupted.Message().Usage = usage
		interrupted.Metadata.Runtime = &canonical.Runtime{UserVisible: true, AgentVisible: true}
		interrupted.Metadata.Execution = &canonical.Execution{Source: &scope, Purpose: &mainPurpose}
		want := measuredTotal + pendingTokens([]canonical.Record{interrupted, steer})
		if got := contextTokens([]canonical.Record{bulky, measured, interrupted, steer}); got != want {
			t.Fatalf("interrupted reply (usage=%+v) jumped contextTokens to %d, want %d", usage, got, want)
		}
		if !servedFromCache(binding, []canonical.Record{bulky, measured, interrupted, steer}) {
			t.Fatalf("interrupted reply (usage=%+v) broke servedFromCache", usage)
		}
	}

	// Case C: failed compaction observation record should not discard main context baseline
	failedCompaction := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant), manualNow())
	compactionTotal := uint64(1000)
	failedCompaction.Message().Usage = &canonical.Usage{Total: &compactionTotal}
	failedCompaction.Metadata.Runtime = &canonical.Runtime{UserVisible: false, AgentVisible: false}
	failedCompaction.Metadata.Execution = &canonical.Execution{Source: &scope, Purpose: &compactPurpose}
	if got := contextTokens([]canonical.Record{bulky, measured, failedCompaction, steer}); got != wantBaseline {
		t.Fatalf("failed compaction usage jumped contextTokens to %d, want %d", got, wantBaseline)
	}
}

func TestSteeringInterruptionDoesNotTriggerPrematureCompaction(t *testing.T) {
	t.Parallel()
	for _, streamPartial := range []bool{false, true} {
		name := "before_first_token"
		if streamPartial {
			name = "after_partial_text"
		}
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			started := make(chan struct{})
			inputTok, outputTok, totalTok, cacheTok := int64(9800), int64(200), int64(10000), int64(9000)
			measuredFinish := finish()
			measuredFinish.ProviderMetadata = fantasy.ProviderMetadata{
				model.UsageMetadataKey: &model.UsageMetadata{
					Reported:  true,
					Input:     &inputTok,
					Output:    &outputTok,
					Total:     &totalTok,
					CacheRead: &cacheTok,
				},
			}
			m := &scriptedModel{scripts: []script{
				response(text("Previous answer"), measuredFinish),
				func(ctx context.Context, _ fantasy.Call, yield func(fantasy.StreamPart) bool) {
					if streamPartial {
						if !yield(text("Working on it...")) {
							return
						}
					}
					close(started)
					<-ctx.Done()
				},
				response(text("Steered reply"), finish()),
			}}
			f := newFixture(t, m)
			f.start()
			// Turn 1 seeds a bulky user prompt whose raw token estimate exceeds
			// the 32,000-token window's compaction trigger, while the provider's
			// measured total usage is only 10,000 tokens (well below trigger).
			turn1 := f.submit(strings.Repeat("historical content block ", 10000))
			f.waitRun(turn1, protocol.ConversationStatusCompleted)

			turn2 := f.submit("Continue")
			select {
			case <-started:
			case <-f.ctx.Done():
				t.Fatal("in-flight model request did not start")
			}
			f.command("guide_turn", &protocol.CommandGuideTurnPayload{
				Recipient: turn2, ExpectedRevision: f.session.Revision, Message: protocol.ConversationInput{Text: "Just fix the typo"},
			})
			f.waitRun(turn2, protocol.ConversationStatusCompleted)
			for _, rec := range f.state().Records {
				if rec.Kind == "compaction" {
					t.Fatal("steering interruption triggered premature compaction")
				}
				if msg := rec.Message(); msg != nil && msg.Finish != nil && *msg.Finish == canonical.Interrupted && msg.Usage != nil {
					t.Fatalf("interrupted reply persisted empty usage: %+v", msg.Usage)
				}
			}
			if m.count() != 3 {
				t.Fatalf("model calls = %d, want 3 (turn1 + interrupted + steered, no compaction summary)", m.count())
			}
		})
	}
}
