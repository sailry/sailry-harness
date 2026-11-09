package store

import (
	"database/sql"
	"errors"
	"math"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAgentStatistics(t *testing.T) {
	t.Parallel()
	for _, scenario := range []string{"membership_and_pricing", "unmetered_resets_context", "explicit_reset_estimates_growth", "summary_resets_context", "captured_budget", "observation_snapshots"} {
		t.Run(scenario, func(t *testing.T) {
			t.Parallel()
			storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
			ctx := t.Context()
			err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				empty, err := ReadAgentStatistics(ctx, db, session.ID)
				if err != nil {
					return err
				}
				if empty.Turns != 0 || empty.Responses != 0 || empty.Usage != nil || empty.ContextTokens != nil || empty.Observations.CacheRead != nil {
					return errors.New("empty history invented recorded usage")
				}
				message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Answer"))
				message.Usage = &canonical.Usage{Input: ptr(uint64(100)), Output: ptr(uint64(20)), CacheRead: ptr(uint64(2)), Reasoning: ptr(uint64(1))}
				record := canonical.NewRecord(turn, "message", &message, now)
				record.Provenance.Author = "assistant"
				record.Metadata.Execution = &canonical.Execution{Timing: &canonical.Timing{ElapsedUs: ptr(uint64(10)), FirstTokenUs: ptr(uint64(4))}}
				appendRecord := func(record canonical.Record) error {
					_, _, err := AppendCanonical(ctx, db, session.ID, record)
					return err
				}
				if err = appendRecord(record); err != nil {
					return err
				}
				switch scenario {
				case "membership_and_pricing":
					if err = appendRecord(record); err != nil {
						return err
					}
					if _, err = db.ExecContext(ctx, `INSERT INTO model_catalog(provider,id,body) VALUES('openai','fixture',?1)`, []byte(`{"cost":{"input":2,"output":8,"cache_read":0.5}}`)); err != nil {
						return err
					}
				case "unmetered_resets_context":
					unmetered := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Unmetered"))), now)
					if err = appendRecord(unmetered); err != nil {
						return err
					}
				case "explicit_reset_estimates_growth":
					reset := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant)), now)
					reset.Metadata.Execution = &canonical.Execution{ContextTokens: ptr(uint64(60))}
					if err = appendRecord(reset); err != nil {
						return err
					}
					growth := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Unmetered growth"))), now)
					if err = appendRecord(growth); err != nil {
						return err
					}
				case "summary_resets_context":
					summary := canonical.NewRecord(turn, "compaction", &canonical.Compaction{Boundary: record.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Handoff")), Start: now, End: now}, now)
					if err = appendRecord(summary); err != nil {
						return err
					}
					growth := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.User, canonical.Text(0, "New evidence"))), now)
					if err = appendRecord(growth); err != nil {
						return err
					}
				case "captured_budget":
					provider, err := currentProvider(ctx, db, session.Config.Provider, nil)
					if err != nil {
						return err
					}
					provider.Provider.Models[0].Context = 200000
					provider.Provider.Models[0].Output = 64000
					if err = saveProviderConfiguration(ctx, db, *provider); err != nil {
						return err
					}
				case "observation_snapshots":
					attempt := protocol.ConversationObservationAttempt{Index: 1, Transport: "http", ResponseUs: ptr(uint64(2)), FirstTokenUs: ptr(uint64(4)), Outcome: protocol.ConversationObservationOutcomeCancelled, Categories: []protocol.ConversationObservationCategory{{Kind: "user", Items: ptr(uint64(1)), Tokens: ptr(uint64(3))}}, CompleteContext: true, CacheReported: true, ReasoningReported: true, Usage: &protocol.ConversationUsage{Input: 100, Output: 20, Reasoning: 1}}
					initial := attempt
					failed := attempt
					failed.ElapsedUs, failed.Outcome = ptr(uint64(10)), protocol.ConversationObservationOutcomeFailed
					succeeded := failed
					succeeded.Index, succeeded.ElapsedUs, succeeded.Outcome, succeeded.ReasoningStreamed = 2, ptr(uint64(20)), protocol.ConversationObservationOutcomeSucceeded, true
					for _, attempt := range []protocol.ConversationObservationAttempt{initial, failed, initial, succeeded, failed} {
						event := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant)), now)
						event.Metadata.Execution = &canonical.Execution{Observation: &protocol.ConversationObservationRequest{ID: "captured-request", Turn: turn, Model: "fixture", TimestampMs: now.UnixMilli(), Attempts: []protocol.ConversationObservationAttempt{attempt}}}
						if err = appendRecord(event); err != nil {
							return err
						}
					}
				}
				result, err := ReadAgentStatistics(ctx, db, session.ID)
				if err != nil {
					return err
				}
				if result.Turns != 1 || result.Responses != 1 || result.Usage == nil || result.Usage.Input != 100 || result.Usage.Output != 20 || result.Generation == nil || result.Generation.ElapsedUs != 10 || result.Observations.Replies != 1 {
					t.Fatalf("recorded totals = %#v", result)
				}
				switch scenario {
				case "unmetered_resets_context":
					if result.ContextTokens != nil || result.ContextLimit != nil || result.ContextModel != nil || result.ContextTimestampMs != nil || result.FirstTokenUs != nil || result.ContextEstimate != nil {
						t.Fatal("unmetered response reused a stale context observation")
					}
				case "explicit_reset_estimates_growth":
					if result.ContextTokens != nil || result.ContextEstimate == nil || *result.ContextEstimate <= 60 || result.FirstTokenUs != nil {
						t.Fatal("unmetered growth lost the explicit context reset")
					}
				case "summary_resets_context":
					if result.ContextTokens != nil || result.ContextEstimate == nil || *result.ContextEstimate == 0 || result.LastCompaction == nil || result.LastCompaction.Outcome.Kind != "succeeded" || result.LastCompaction.Turn != turn {
						t.Fatal("summary did not reset the canonical context window")
					}
				case "observation_snapshots":
					summary := result.Observations
					if summary.Calls != 1 || summary.ModelUs != 30 || summary.OutputUs != 22 || summary.OutputTokens != 39 || summary.OutputSamples != 2 || len(summary.Recent) != 1 || len(summary.Recent[0].Attempts) != 1 || summary.Recent[0].Attempts[0].Index != 2 || summary.Recent[0].TurnIndex == nil || *summary.Recent[0].TurnIndex != 1 || summary.Latency.ElapsedUs == nil || *summary.Latency.ElapsedUs != 15 || len(summary.Composition) != 1 || *summary.Composition[0].Items != 2 {
						t.Fatalf("attempt aggregates = %#v", summary)
					}
				default:
					if result.ContextTokens == nil || *result.ContextTokens != 120 || result.ContextLimit == nil || *result.ContextLimit != 32000 || result.FirstTokenUs == nil || *result.FirstTokenUs != 4 {
						t.Fatal("captured context observation was not retained")
					}
				}
				if scenario == "captured_budget" {
					if result.Budget == nil || result.Budget.ContextLimit != 32000 || result.Budget.SummaryOutput != 8000 {
						t.Fatal("active policy followed an edited provider")
					}
					if _, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
					current, err := ReadAgentStatistics(ctx, db, session.ID)
					if err != nil {
						return err
					}
					if current.Budget == nil || current.Budget.ContextLimit != 200000 || current.Budget.TriggerTokens != 160000 || current.Budget.OutputReserve != 64000 || current.Budget.SummaryOutput != 8000 || current.ContextLimit == nil || *current.ContextLimit != 32000 {
						t.Fatal("settled policy or frozen measurement used the wrong provider revision")
					}
				}
				if scenario == "membership_and_pricing" {
					if result.Cost == nil || result.Cost.UsdMicros != 357 || result.Cost.Responses != 1 || result.Observations.CacheRead == nil || *result.Observations.CacheRead != 2 {
						t.Fatalf("session pricing = %#v", result)
					}
					if _, _, err = FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
						return err
					}
					fork, _, err := forkConversation(ctx, db, protocol.CommandForkConversationPayload{Session: session.ID, Through: turn, ExpectedRevision: session.Revision})
					if err != nil {
						return err
					}
					inherited, err := ReadAgentStatistics(ctx, db, fork.ID)
					if err != nil {
						return err
					}
					if !reflect.DeepEqual(inherited, result) {
						t.Fatal("fork changed inherited usage, captured timing or pricing")
					}
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
	t.Run("latency_quality", func(t *testing.T) {
		var mean agentStatisticMean
		mean.observe(ptr(uint64(math.MaxUint64)))
		mean.observe(ptr(uint64(math.MaxUint64)))
		if value := mean.finish(); value == nil || *value != math.MaxUint64 {
			t.Fatal("latency mean overflowed")
		}
		var latency agentStatisticLatency
		latency.observe(protocol.ConversationObservationAttempt{Transport: "http", HeadersUs: ptr(uint64(999)), ResponseUs: ptr(uint64(3)), FirstTokenUs: ptr(uint64(12)), ElapsedUs: ptr(uint64(10))})
		if *latency.response.finish() != 3 || *latency.first.finish() != 12 || *latency.waiting.finish() != 9 || *latency.elapsed.finish() != 10 {
			t.Fatal("SDK completion incorrectly bounded first-token delivery")
		}
	})
}
