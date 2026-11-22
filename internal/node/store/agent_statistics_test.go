package store

import (
	"database/sql"
	"encoding/json"
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
				if result.Turns != 1 || result.Responses != 1 || result.Usage == nil || result.Usage.Input != 100 || result.Usage.Output != 20 || result.Generation == nil || result.Generation.ElapsedUs != 6 || result.Generation.OutputTokens != 19 || result.Observations.Replies != 1 {
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
					if current.Budget == nil || current.Budget.ContextLimit != 200000 || current.Budget.TriggerTokens != 136000 || current.Budget.OutputReserve != 64000 || current.Budget.SummaryOutput != 8000 || current.ContextLimit == nil || *current.ContextLimit != 32000 {
						return errors.New("settled budget or frozen measurement used the wrong policy")
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
					if usage.Totals.Generation == nil || *usage.Totals.Generation != *result.Generation || usage.Requests.Items[0].ElapsedUs == nil || *usage.Requests.Items[0].ElapsedUs != 10 || usage.Requests.Items[0].FirstTokenUs == nil || *usage.Requests.Items[0].FirstTokenUs != 4 {
						t.Fatalf("session and report decode statistics or raw request timing differ: %+v", usage.Totals)
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

func TestLatencyTimelineUsesMatchedAttempts(t *testing.T) {
	var latency agentStatisticLatency
	latency.observe(protocol.ConversationObservationAttempt{ResponseUs: ptr(uint64(70))})
	latency.observe(protocol.ConversationObservationAttempt{FirstTokenUs: ptr(uint64(110))})
	latency.observe(protocol.ConversationObservationAttempt{ResponseUs: ptr(uint64(90)), FirstTokenUs: ptr(uint64(80))})
	if latency.pairedResponse.finish() != nil || latency.waiting.finish() != nil {
		t.Fatal("unpaired latency produced a timeline")
	}
	latency.observe(protocol.ConversationObservationAttempt{ResponseUs: ptr(uint64(10)), FirstTokenUs: ptr(uint64(30))})
	latency.observe(protocol.ConversationObservationAttempt{Transport: "http", HeadersUs: ptr(uint64(0)), FirstTokenUs: ptr(uint64(0))})
	if a, b := *latency.pairedResponse.finish(), *latency.waiting.finish(); a != 5 || b != 10 {
		t.Fatalf("paired response and waiting = %d and %d, want 5 and 10", a, b)
	}
}

func TestAgentStatisticsCacheCoverage(t *testing.T) {
	t.Parallel()
	storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		appendUsage := func(usage canonical.Usage) error {
			message := canonical.NewMessage(canonical.Assistant)
			message.Usage = &usage
			record := canonical.NewRecord(turn, "message", &message, now)
			record.Metadata.Execution = &canonical.Execution{Timing: &canonical.Timing{ElapsedUs: ptr(uint64(10))}}
			_, _, err := AppendCanonical(ctx, db, session.ID, record)
			return err
		}
		if err := appendUsage(canonical.Usage{}); err != nil {
			return err
		}
		if err := appendUsage(canonical.Usage{Input: ptr(uint64(100)), Output: ptr(uint64(20)), CacheRead: ptr(uint64(60))}); err != nil {
			return err
		}
		stats, history, err := ReadAgentStatisticsReusing(ctx, db, session.ID, nil)
		if err != nil {
			return err
		}
		if stats.Responses != 1 || stats.Usage == nil || stats.Usage.Input != 100 || stats.Observations.CacheRead == nil || *stats.Observations.CacheRead != 60 {
			return fmt.Errorf("unmetered retry affected reported cache coverage: %+v", stats)
		}
		if err := appendUsage(canonical.Usage{Input: ptr(uint64(50)), Output: ptr(uint64(10))}); err != nil {
			return err
		}
		stats, history, err = ReadAgentStatisticsReusing(ctx, db, session.ID, history)
		if err != nil {
			return err
		}
		if stats.Responses != 2 || stats.Usage == nil || stats.Usage.Input != 150 || stats.Observations.CacheRead != nil || history.work.usageRows != 1 {
			return fmt.Errorf("appended response without cache usage: statistics=%+v, work=%+v", stats, history.work)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestAgentStatisticsPreservesAnthropicCacheWritePricing(t *testing.T) {
	t.Parallel()
	storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Answer"))
		message.Usage = &canonical.Usage{Input: ptr(uint64(200)), Output: ptr(uint64(20)), CacheRead: ptr(uint64(60)), CacheWrite: ptr(uint64(40)), Provider: json.RawMessage(`{"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":40}}`)}
		record := canonical.NewRecord(turn, "message", &message, now)
		record.Metadata.Execution = &canonical.Execution{Source: &canonical.ModelSource{ID: session.Config.Provider, Name: "Fixture", API: "anthropic", Authentication: "api_key", Model: "fixture"}}
		if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, `INSERT INTO model_catalog(provider,id,body) VALUES('anthropic','fixture',?1)`, []byte(`{"cost":{"input":3,"output":15,"cache_read":0.3,"cache_write":3.75}}`)); err != nil {
			return err
		}
		result, err := ReadAgentStatistics(ctx, db, session.ID)
		if err != nil {
			return err
		}
		if result.Cost == nil || result.Cost.UsdMicros != 858 || result.Observations.CacheRead == nil || *result.Observations.CacheRead != 60 {
			return fmt.Errorf("hourly cache write statistics: cost=%+v cached=%v", result.Cost, result.Observations.CacheRead)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestDecodeGenerationRequiresMatchedTiming(t *testing.T) {
	elapsed, first := uint64(10), uint64(4)
	tokens := protocol.ConversationUsage{Output: 12, Reasoning: 5}
	for _, times := range [][2]*uint64{{nil, &first}, {&elapsed, nil}, {&first, &first}, {&first, &elapsed}} {
		if got := tokens.DecodeGeneration(times[0], times[1], false); got != nil {
			t.Fatalf("incomplete or nonpositive decode interval contributed %+v", got)
		}
	}
	for _, tc := range []struct {
		usage    protocol.ConversationUsage
		streamed bool
		want     uint64
	}{{tokens, false, 7}, {tokens, true, 12}, {protocol.ConversationUsage{Output: 12}, false, 12}} {
		got := tc.usage.DecodeGeneration(&elapsed, &first, tc.streamed)
		if got == nil || got.ElapsedUs != 6 || got.OutputTokens != tc.want || got.Responses != 1 {
			t.Fatalf("decode sample = %+v for %+v", got, tc)
		}
	}
}

func TestAgentThroughputUsesMatchedFullRequestDurations(t *testing.T) {
	t.Parallel()
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		count := ptr[uint64]
		image, vision := protocol.MediaKindImage, protocol.MediaKindVision
		requests := []struct {
			input, output, reasoning *uint64
			elapsed, first           *uint64
			media                    *protocol.MediaKind
		}{
			{count(1), count(150), count(110), count(10_000_000), count(9_800_000), nil},
			{nil, count(18), nil, count(2_000_000), nil, nil},
			{count(1), count(0), nil, count(4_000_000), count(4_000_000), nil},
			{count(1), count(1000), nil, nil, count(10), nil},
			{count(1), count(2000), nil, count(0), nil, nil},
			{count(1), nil, nil, count(100_000_000), nil, nil},
			{count(1), count(5000), nil, count(1_000_000), nil, &image},
			{count(1), count(32), nil, count(4_000_000), nil, &vision},
			{count(1), count(100), nil, count(2), count(3), nil},
		}
		for _, request := range requests {
			message := canonical.NewMessage(canonical.Assistant)
			message.Usage = &canonical.Usage{Input: request.input, Output: request.output, Reasoning: request.reasoning}
			record := canonical.NewRecord(turn, "message", &message, now)
			record.Metadata.Execution = &canonical.Execution{Media: request.media, Timing: &canonical.Timing{ElapsedUs: request.elapsed, FirstTokenUs: request.first}}
			if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
				return err
			}
		}
		stats, err := ReadAgentStatistics(ctx, db, session.ID)
		if err != nil {
			return err
		}
		query := protocol.DefaultUsageQuery()
		query.StartMs, query.EndMs, query.Dimension = now.UnixMilli(), now.Add(time.Minute).UnixMilli(), protocol.UsageDimensionModel
		report, err := readAgentUsage(ctx, db, node, query)
		if err != nil {
			return err
		}
		if len(report.Groups) != 1 || len(report.Days) != 1 || len(report.Recent) != 1 {
			return fmt.Errorf("unexpected usage buckets: groups=%d days=%d recent=%d", len(report.Groups), len(report.Days), len(report.Recent))
		}
		// All completions share the same timestamp. Each measured response contributes its own duration.
		want := protocol.UsageGeneration{OutputTokens: 200, ElapsedUs: 20_000_000, Responses: 4}
		for surface, got := range map[string]*protocol.UsageGeneration{
			"session": stats.Throughput, "report": report.Totals.Throughput, "group": report.Groups[0].Metrics.Throughput,
			"day": report.Days[0].Metrics.Throughput, "recent": report.Recent[0].Metrics.Throughput,
		} {
			if got == nil || *got != want {
				return fmt.Errorf("%s throughput = %+v, want %+v", surface, got, want)
			}
		}
		if stats.Responses != uint64(len(requests)) || stats.IncompleteTokenResponses != 2 || report.Totals.Responses != stats.Responses || report.Totals.IncompleteTokenResponses != 2 {
			return fmt.Errorf("metered responses=%d, incomplete=%d, report=%+v", stats.Responses, stats.IncompleteTokenResponses, report.Totals)
		}
		decode := protocol.UsageGeneration{OutputTokens: 40, ElapsedUs: 200_000, Responses: 1}
		if stats.Generation == nil || *stats.Generation != decode || report.Totals.Generation == nil || *report.Totals.Generation != decode {
			return fmt.Errorf("decode samples: session=%+v report=%+v, want %+v", stats.Generation, report.Totals.Generation, decode)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestDelegatedStatistics(t *testing.T) {
	t.Parallel()
	storage, node, session, parent, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := db.ExecContext(ctx, `INSERT INTO model_catalog(provider,id,body) VALUES('openai','fixture',?1)`, []byte(`{"cost":{"input":2,"output":8}}`)); err != nil {
			return err
		}
		respond := func(session protocol.SessionID, turn protocol.TurnID, provider *protocol.ConversationProvider, input, output, elapsed uint64) error {
			message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Answer"))
			message.Usage = &canonical.Usage{Input: ptr(input), Output: ptr(output)}
			record := canonical.NewRecord(turn, "message", &message, now)
			record.Provenance.Author = "assistant"
			record.Metadata.Execution = &canonical.Execution{Timing: &canonical.Timing{ElapsedUs: ptr(elapsed), FirstTokenUs: ptr(uint64(2))}}
			if provider != nil {
				record.Metadata.Execution.Source = &canonical.ModelSource{ID: provider.ID, Name: provider.Name, API: provider.Api, Authentication: provider.Authentication, Model: "fixture"}
			}
			_, _, err := AppendCanonicalWithProvider(ctx, db, session, record, provider)
			return err
		}
		if err := respond(session.ID, parent, nil, 100, 20, 10); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, `UPDATE agent_runs SET provider=json_set(provider,'$.billing_rate_bps',5000) WHERE turn=?1`, parent.String()); err != nil {
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
			inv, _, err := ClaimChild(ctx, db, child.Run.Turn, now)
			if err != nil || inv == nil || inv.Provider == nil {
				return fmt.Errorf("child was not claimed: %v", err)
			}
			for range responses {
				if err = respond(child.Session.ID, child.Run.Turn, inv.Provider, 1000, 300, 7); err != nil {
					return err
				}
			}
			children = append(children, child)
		}
		result, err := ReadAgentStatistics(ctx, db, session.ID)
		if err != nil {
			return err
		}
		if result.Turns != 1 || result.Responses != 1 || result.Usage == nil || result.Usage.Input != 100 || result.Usage.Output != 20 || result.Cost == nil || result.Cost.UsdMicros != 360 || result.Generation == nil || result.Generation.ElapsedUs != 8 {
			t.Fatalf("child usage leaked into the parent's own totals: %#v", result)
		}
		delegated := result.Delegated
		if delegated.Responses != 3 || delegated.Tokens == nil || delegated.Tokens.Input != 3000 || delegated.Tokens.Output != 900 || delegated.Cost == nil || delegated.Cost.UsdMicros != 6600 || delegated.Cost.Responses != 3 || delegated.Generation == nil || delegated.Generation.ElapsedUs != 15 {
			t.Fatalf("delegated totals = %#v", delegated)
		}
		want := protocol.UsageGeneration{OutputTokens: 900, ElapsedUs: 21, Responses: 3}
		if delegated.Throughput == nil || *delegated.Throughput != want {
			t.Fatalf("delegated throughput = %+v, want %+v", delegated.Throughput, want)
		}
		own, err := ReadAgentStatistics(ctx, db, children[0].Session.ID)
		if err != nil {
			return err
		}
		if own.Responses != 2 || own.Usage == nil || own.Usage.Input != 2000 || own.Cost == nil || own.Cost.UsdMicros != 4400 || !reflect.DeepEqual(own.Delegated, protocol.DefaultUsageMetrics()) {
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
