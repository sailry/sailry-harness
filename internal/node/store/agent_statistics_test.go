package store

import (
	"database/sql"
	"errors"
	"fmt"
	"math"
	"reflect"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAgentStatistics(t *testing.T) {
	t.Parallel()
	for _, scenario := range []string{"membership_and_pricing", "unmetered_resets_context", "explicit_reset_estimates_growth", "summary_resets_context", "captured_budget", "observation_snapshots"} {
		t.Run(scenario, func(t *testing.T) {
			t.Parallel()
			storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
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
					status := protocol.DefaultConversationCatalogStatus()
					status.Revision, status.UpdatedAtMs = 1, ptr(uint64(1234))
					body, err := encode(status)
					if err != nil {
						return err
					}
					if _, err = db.ExecContext(ctx, "INSERT INTO model_catalog_status(id,body) VALUES(1,?1)", body); err != nil {
						return err
					}
					query := protocol.DefaultUsageQuery()
					query.StartMs, query.EndMs, query.Dimension = now.Add(-time.Hour).UnixMilli(), now.Add(time.Hour).UnixMilli(), protocol.UsageDimensionModel
					output, err := ReadAgentCommand(ctx, db, node, protocol.Command{Kind: "read_usage", Data: &query})
					if err != nil {
						return fmt.Errorf("read_usage: %w", err)
					}
					usage, ok := output.Data.(protocol.UsageReport)
					if !ok || output.Kind != "usage" || usage.Node != node || usage.Totals.Responses != 1 || usage.Totals.Tokens == nil || usage.Totals.Tokens.Input != 100 || usage.Totals.Tokens.Output != 20 ||
						usage.Totals.Cost == nil || usage.Totals.Cost.UsdMicros != 357 || len(usage.Groups) != 1 || len(usage.Requests.Items) != 1 || usage.PricingUpdatedAtMs == nil || *usage.PricingUpdatedAtMs != 1234 {
						t.Fatalf("usage report = %#v", output)
					}
					// Grouping by project labels the group, not the request's provider.
					query.Dimension = protocol.UsageDimensionProject
					if output, err = ReadAgentCommand(ctx, db, node, protocol.Command{Kind: "read_usage", Data: &query}); err != nil {
						return fmt.Errorf("read_usage by project: %w", err)
					}
					if usage = output.Data.(protocol.UsageReport); len(usage.Requests.Items) != 1 || usage.Requests.Items[0].ProviderName != "Fixture" {
						return fmt.Errorf("project report requests = %#v", usage.Requests.Items)
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
	t.Run("vendor_pricing", func(t *testing.T) {
		t.Parallel()
		storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
		ctx := t.Context()
		builtin := func(id string) *protocol.ConversationPresetsPresetRef {
			return &protocol.ConversationPresetsPresetRef{Source: protocol.ConversationPresetsSource{Kind: "builtin"}, ID: id}
		}
		price := func(input, output float64) string {
			return fmt.Sprintf(`{"cost":{"input":%g,"output":%g,"cache_read":0.5}}`, input, output)
		}
		err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Answer"))
			message.Usage = &canonical.Usage{Input: ptr(uint64(100)), Output: ptr(uint64(20)), CacheRead: ptr(uint64(2))}
			record := canonical.NewRecord(turn, "message", &message, now)
			record.Provenance.Author = "assistant"
			if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
				return err
			}
			// The fixture channel speaks Chat Completions; 98 input, 20 output and 2 cached tokens.
			for _, test := range []struct {
				name     string
				saved    *protocol.ConversationPresetsPresetRef
				captured *protocol.ConversationPresetsPresetRef
				catalog  map[string]string
				micros   uint64
			}{
				{name: "preset vendor before the protocol's", saved: builtin("deepseek"), catalog: map[string]string{"deepseek": price(1, 2), "openai": price(2, 8)}, micros: 139},
				{name: "captured preset before the saved one", saved: builtin("xai"), captured: builtin("deepseek"), catalog: map[string]string{"deepseek": price(1, 2), "xai": price(2, 8)}, micros: 139},
				{name: "preset under its catalog name", saved: builtin("qwen"), catalog: map[string]string{"alibaba": price(3, 6), "openai": price(2, 8)}, micros: 415},
				{name: "preset brand", saved: builtin("compatible-chat"), catalog: map[string]string{"openai": price(2, 8), "reseller": price(9, 9)}, micros: 357},
				{name: "protocol vendor without a preset", catalog: map[string]string{"openai": price(2, 8), "reseller": price(9, 9)}, micros: 357},
				{name: "foreign preset is not a vendor", saved: &protocol.ConversationPresetsPresetRef{Source: protocol.ConversationPresetsSource{Kind: "partner"}, ID: "deepseek"}, catalog: map[string]string{"deepseek": price(1, 2), "openai": price(2, 8)}, micros: 357},
				{name: "one price elsewhere", saved: builtin("deepseek"), catalog: map[string]string{"first": price(1, 2), "second": price(1, 2), "unpriced": `{"limit":{"context":1}}`}, micros: 139},
				{name: "disputed price elsewhere", saved: builtin("deepseek"), catalog: map[string]string{"first": price(1, 2), "second": price(2, 4)}},
				{name: "unknown model", saved: builtin("deepseek"), catalog: map[string]string{}},
			} {
				provider, err := currentProvider(ctx, db, session.Config.Provider, nil)
				if err != nil {
					return err
				}
				provider.Provider.Preset = test.saved
				if err = saveProviderConfiguration(ctx, db, *provider); err != nil {
					return err
				}
				capture := "null"
				if test.captured != nil {
					body, err := encode(test.captured)
					if err != nil {
						return err
					}
					capture = string(body)
				}
				if _, err = db.ExecContext(ctx, "UPDATE agent_runs SET provider=json_set(json(provider),'$.preset',json(?1)) WHERE turn=?2", capture, turn.String()); err != nil {
					return err
				}
				if _, err = db.ExecContext(ctx, "DELETE FROM model_catalog"); err != nil {
					return err
				}
				for supplier, body := range test.catalog {
					if _, err = db.ExecContext(ctx, "INSERT INTO model_catalog(provider,id,body) VALUES(?1,'fixture',?2)", supplier, []byte(body)); err != nil {
						return err
					}
				}
				statistics, err := ReadAgentStatistics(ctx, db, session.ID)
				if err != nil {
					return err
				}
				query := protocol.DefaultUsageQuery()
				query.StartMs, query.EndMs, query.Dimension = now.Add(-time.Hour).UnixMilli(), now.Add(time.Hour).UnixMilli(), protocol.UsageDimensionModel
				output, err := ReadAgentCommand(ctx, db, node, protocol.Command{Kind: "read_usage", Data: &query})
				if err != nil {
					return err
				}
				for surface, cost := range map[string]*protocol.UsageCost{"session": statistics.Cost, "report": output.Data.(protocol.UsageReport).Totals.Cost} {
					if test.micros == 0 && cost != nil || test.micros != 0 && (cost == nil || cost.UsdMicros != test.micros) {
						t.Errorf("%s: %s cost = %#v, want %d", test.name, surface, cost, test.micros)
					}
				}
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	})
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

func TestDelegatedStatistics(t *testing.T) {
	t.Parallel()
	storage, node, session, parent, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := db.ExecContext(ctx, `INSERT INTO model_catalog(provider,id,body) VALUES('openai','fixture',?1)`, []byte(`{"cost":{"input":2,"output":8}}`)); err != nil {
			return err
		}
		respond := func(session protocol.SessionID, turn protocol.TurnID, input, output, elapsed uint64) error {
			message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Answer"))
			message.Usage = &canonical.Usage{Input: ptr(input), Output: ptr(output)}
			record := canonical.NewRecord(turn, "message", &message, now)
			record.Provenance.Author = "assistant"
			record.Metadata.Execution = &canonical.Execution{Timing: &canonical.Timing{ElapsedUs: ptr(elapsed)}}
			_, _, err := AppendCanonical(ctx, db, session, record)
			return err
		}
		if err := respond(session.ID, parent, 100, 20, 10); err != nil {
			return err
		}
		var children []AgentInvocation
		for index, responses := range []int{2, 1} {
			call, err := delegationFixtureCall(ctx, db, parent, "spawn_agent", map[string]any{"task": fmt.Sprintf("Child %d", index), "background": true}, now)
			if err != nil {
				return err
			}
			child, _, err := DelegateAgent(ctx, db, node, parent, call, nil, true, now)
			if err != nil {
				return err
			}
			if inv, _, err := ClaimChild(ctx, db, child.Run.Turn, now); err != nil || inv == nil {
				return fmt.Errorf("child was not claimed: %v", err)
			}
			for range responses {
				if err = respond(child.Session.ID, child.Run.Turn, 1000, 300, 7); err != nil {
					return err
				}
			}
			children = append(children, child)
		}
		result, err := ReadAgentStatistics(ctx, db, session.ID)
		if err != nil {
			return err
		}
		if result.Turns != 1 || result.Responses != 1 || result.Usage == nil || result.Usage.Input != 100 || result.Usage.Output != 20 || result.Cost == nil || result.Cost.UsdMicros != 360 || result.Generation == nil || result.Generation.ElapsedUs != 10 {
			t.Fatalf("child usage leaked into the parent's own totals: %#v", result)
		}
		delegated := result.Delegated
		if delegated.Responses != 3 || delegated.Tokens == nil || delegated.Tokens.Input != 3000 || delegated.Tokens.Output != 900 || delegated.Cost == nil || delegated.Cost.UsdMicros != 13200 || delegated.Cost.Responses != 3 || delegated.Generation == nil || delegated.Generation.ElapsedUs != 21 {
			t.Fatalf("delegated totals = %#v", delegated)
		}
		own, err := ReadAgentStatistics(ctx, db, children[0].Session.ID)
		if err != nil {
			return err
		}
		if own.Responses != 2 || own.Usage == nil || own.Usage.Input != 2000 || !reflect.DeepEqual(own.Delegated, protocol.DefaultUsageMetrics()) {
			t.Fatalf("child statistics = %#v", own)
		}
		// A fork inherits the parent turn and therefore the children it delegated.
		if _, _, err = FinishTurn(ctx, db, parent, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		fork, _, err := forkConversation(ctx, db, protocol.CommandForkConversationPayload{Session: session.ID, Through: parent, ExpectedRevision: session.Revision})
		if err != nil {
			return err
		}
		inherited, err := ReadAgentStatistics(ctx, db, fork.ID)
		if err != nil {
			return err
		}
		if !reflect.DeepEqual(inherited.Delegated, delegated) {
			t.Fatalf("fork delegated totals = %#v", inherited.Delegated)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
