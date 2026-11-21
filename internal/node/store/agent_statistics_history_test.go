package store

import (
	"database/sql"
	"fmt"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAgentStatisticsHistoryReads(t *testing.T) {
	storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	write := func(callback func(*sql.Conn) error) {
		t.Helper()
		if err := storage.Node.Write(ctx, callback); err != nil {
			t.Fatal(err)
		}
	}
	appendResponse := func(db *sql.Conn, metered bool) error {
		message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Answer"))
		if metered {
			message.Usage = &canonical.Usage{Input: ptr(uint64(100)), Output: ptr(uint64(20))}
		}
		record := canonical.NewRecord(turn, "message", &message, now)
		record.Provenance.Author = "assistant"
		_, _, err := AppendCanonical(ctx, db, session.ID, record)
		return err
	}
	const history = 64
	write(func(db *sql.Conn) error {
		for range history {
			if err := appendResponse(db, true); err != nil {
				return err
			}
		}
		return nil
	})
	var previous *AgentStatisticsHistory
	read := func(name string, usageRows, eventRows, callReads, contextReads, fullUsage, fullEvents int) protocol.ConversationStatistics {
		t.Helper()
		var current protocol.ConversationStatistics
		if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			var next *AgentStatisticsHistory
			var err error
			current, next, err = ReadAgentStatisticsReusing(ctx, db, session.ID, previous)
			if err != nil {
				return err
			}
			baseline, cold, err := ReadAgentStatisticsReusing(ctx, db, session.ID, nil)
			if err != nil {
				return err
			}
			if !reflect.DeepEqual(current, baseline) {
				return fmt.Errorf("warm aggregate differs from a cold snapshot: got %+v, want %+v", current, baseline)
			}
			if work := cold.work; work.usageRows != fullUsage || work.eventRows != fullEvents || work.callReads != 1 || work.contextReads != 1 || work.compactionReads != 1 {
				return fmt.Errorf("unexpected baseline work: %+v", work)
			}
			want := agentStatisticsWork{usageRows: usageRows, eventRows: eventRows, callReads: callReads, contextReads: contextReads}
			if previous == nil {
				want.compactionReads = 1
			}
			if next.work != want {
				return fmt.Errorf("reused work = %+v, want %+v (cold: %+v)", next.work, want, cold.work)
			}
			previous = next
			return nil
		}); err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		return current
	}
	read("first subscription", history, history, 1, 1, history, history)
	read("second subscription", 0, 0, 0, 0, history, history)
	write(func(db *sql.Conn) error {
		if _, _, err := renameSession(ctx, db, protocol.CommandRenameSessionPayload{Session: session.ID, ExpectedRevision: session.Revision, Title: "Renamed"}); err != nil {
			return err
		}
		// Transitioning within active statuses changes neither captured usage nor phases.
		_, err := db.ExecContext(ctx, `UPDATE agent_runs SET body=json_set(body,'$.status','stopping') WHERE turn=?1`, turn.String())
		return err
	})
	read("rename and unrelated run status", 0, 0, 0, 0, history, history)
	write(func(db *sql.Conn) error { return appendResponse(db, true) })
	if result := read("appended usage", 1, 1, 0, 1, history+1, history+1); result.Responses != history+1 {
		t.Fatalf("append was not counted: %+v", result)
	}
	write(func(db *sql.Conn) error { return appendResponse(db, false) })
	if result := read("append without observation", 0, 1, 0, 1, history+1, history+2); result.ContextTokens != nil {
		t.Fatalf("unmetered response retained stale context: %+v", result)
	}
	for _, elapsed := range []uint64{10, 20} {
		attempt := protocol.DefaultConversationObservationAttempt()
		attempt.Index, attempt.ElapsedUs = 1, &elapsed
		attempt.Outcome, attempt.Transport = protocol.ConversationObservationOutcomeSucceeded, "http"
		write(func(db *sql.Conn) error {
			message := canonical.NewMessage(canonical.Assistant)
			record := canonical.NewRecord(turn, "message", &message, now)
			record.Metadata.Execution = &canonical.Execution{Observation: &protocol.ConversationObservationRequest{ID: "request", Turn: turn, Model: "fixture", Attempts: []protocol.ConversationObservationAttempt{attempt}}}
			_, _, err := AppendCanonical(ctx, db, session.ID, record)
			return err
		})
		result := read("observation snapshot", 0, 1, 1, 0, history+1, history+2+int(elapsed/10))
		if result.Observations.Calls != 1 || result.Observations.ModelUs != elapsed {
			t.Fatalf("new snapshot did not replace the previous attempt: %+v", result.Observations)
		}
	}
	read("unchanged observation", 0, 0, 0, 0, history+1, history+4)
}

func TestAgentStatisticsHistoryPricingAndBudget(t *testing.T) {
	storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	var previous *AgentStatisticsHistory
	check := func(wantCost, wantTrigger uint64, usageRows int) {
		t.Helper()
		if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			result, next, err := ReadAgentStatisticsReusing(ctx, db, session.ID, previous)
			if err != nil {
				return err
			}
			if result.Cost == nil || result.Cost.UsdMicros != wantCost || result.Cost.Responses != 2 || result.Budget == nil || result.Budget.TriggerTokens != wantTrigger || next.work.usageRows != usageRows {
				return fmt.Errorf("cost=%+v (want %d), budget=%+v (want %d), work=%+v (want %d usage rows)", result.Cost, wantCost, result.Budget, wantTrigger, next.work, usageRows)
			}
			if previous != nil && (next.work.eventRows != 0 || next.work.callReads != 0) {
				return fmt.Errorf("pricing or budget change recomputed observation history: %+v", next.work)
			}
			previous = next
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	}
	write := func(callback func(*sql.Conn) error) {
		t.Helper()
		if err := storage.Node.Write(ctx, callback); err != nil {
			t.Fatal(err)
		}
	}
	write(func(db *sql.Conn) error {
		if _, err := db.ExecContext(ctx, `INSERT INTO model_catalog(provider,id,body) VALUES('openai','fixture',?1),('deepseek','fixture',?2)`, []byte(`{"cost":{"input":2,"output":8}}`), []byte(`{"cost":{"input":1,"output":2}}`)); err != nil {
			return err
		}
		configuration, err := currentProvider(ctx, db, session.Config.Provider, nil)
		if err != nil {
			return err
		}
		for _, frozen := range []bool{false, true} {
			message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Priced answer"))
			message.Usage = &canonical.Usage{Input: ptr(uint64(100)), Output: ptr(uint64(20))}
			record := canonical.NewRecord(turn, "message", &message, now)
			record.Provenance.Author = "assistant"
			var provider *protocol.ConversationProvider
			if frozen {
				provider = configuration.Provider
				provider.BillingRateBps = ptr(uint32(5000))
				record.Metadata.Execution = &canonical.Execution{Source: &canonical.ModelSource{ID: provider.ID, Name: provider.Name, API: provider.Api, Authentication: provider.Authentication, Model: "fixture"}}
			}
			if _, _, err = AppendCanonicalWithProvider(ctx, db, session.ID, record, provider); err != nil {
				return err
			}
		}
		return nil
	})
	check(360+180, 24000, 2)
	write(func(db *sql.Conn) error {
		_, err := db.ExecContext(ctx, `UPDATE model_catalog SET body=?1 WHERE provider='openai'`, []byte(`{"cost":{"input":4,"output":16}}`))
		return err
	})
	check(720+180, 24000, 2)
	write(func(db *sql.Conn) error {
		configuration, err := currentProvider(ctx, db, session.Config.Provider, nil)
		if err != nil {
			return err
		}
		configuration.Provider.Preset = &protocol.ConversationPresetsPresetRef{ID: "deepseek", Source: protocol.ConversationPresetsSource{Kind: "builtin"}}
		configuration.Provider.Models[0].Context = 40000
		configuration.Provider.BillingRateBps = ptr(uint32(20000))
		if err = saveProviderConfiguration(ctx, db, *configuration); err != nil {
			return err
		}
		compaction := protocol.DefaultConversationCompactionSettings()
		compaction.Percent = 70
		_, _, err = saveCompaction(ctx, db, compaction)
		return err
	})
	check(140+180, 24000, 2)
	write(func(db *sql.Conn) error {
		_, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now)
		return err
	})
	check(140+180, 28000, 0)
}

func TestAgentStatisticsHistoryMembershipAndVisibility(t *testing.T) {
	storage, node, parent, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	write := func(callback func(*sql.Conn) error) {
		t.Helper()
		if err := storage.Node.Write(ctx, callback); err != nil {
			t.Fatal(err)
		}
	}
	histories := map[protocol.SessionID]*AgentStatisticsHistory{}
	read := func(session protocol.SessionID) (protocol.ConversationStatistics, agentStatisticsWork) {
		t.Helper()
		var result protocol.ConversationStatistics
		var work agentStatisticsWork
		if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			var next *AgentStatisticsHistory
			var err error
			result, next, err = ReadAgentStatisticsReusing(ctx, db, session, histories[session])
			if err != nil {
				return err
			}
			cold, err := ReadAgentStatistics(ctx, db, session)
			if err != nil {
				return err
			}
			if !reflect.DeepEqual(result, cold) {
				return fmt.Errorf("warm aggregate differs from cold snapshot: %+v, want %+v", result, cold)
			}
			histories[session], work = next, next.work
			return nil
		}); err != nil {
			t.Fatal(err)
		}
		return result, work
	}
	appendParent := func(db *sql.Conn, record canonical.Record) error {
		_, _, err := AppendCanonical(ctx, db, parent.ID, record)
		return err
	}
	write(func(db *sql.Conn) error {
		reset := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant), now)
		reset.Metadata.Execution = &canonical.Execution{ContextTokens: ptr(uint64(10))}
		return appendParent(db, reset)
	})
	if result, _ := read(parent.ID); result.ContextEstimate == nil || *result.ContextEstimate != 10 {
		t.Fatalf("initial context estimate = %+v", result)
	}
	var child *AgentInvocation
	write(func(db *sql.Conn) error {
		call, err := delegationFixtureCall(ctx, db, turn, "spawn_agent", map[string]any{"task": "Background work", "background": true}, now)
		if err != nil {
			return err
		}
		inv, _, err := DelegateAgent(ctx, db, node, turn, call, nil, true, now)
		if err != nil {
			return err
		}
		child, _, err = ClaimChild(ctx, db, inv.Run.Turn, now)
		if err == nil && child == nil {
			return fmt.Errorf("child was not claimed")
		}
		return err
	})
	if result, work := read(parent.ID); result.Observations.Children != 1 || result.Delegated.Responses != 0 || work.usageRows != 0 {
		t.Fatalf("new child membership not reflected: %+v, work %+v", result, work)
	}
	appendChild := func(db *sql.Conn) error {
		message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Child response"))
		message.Usage = &canonical.Usage{Input: ptr(uint64(30)), Output: ptr(uint64(3))}
		response := canonical.NewRecord(child.Run.Turn, "message", &message, now)
		response.Provenance.Author = "assistant"
		_, _, err := AppendCanonical(ctx, db, child.Session.ID, response)
		return err
	}
	write(appendChild)
	if result, work := read(parent.ID); result.Responses != 0 || result.Delegated.Responses != 1 || work != (agentStatisticsWork{usageRows: 1}) {
		t.Fatalf("child update changed or re-read the parent history: %+v, work %+v", result, work)
	}
	tail := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Context growth")), now)
	tail.Provenance.Author = "user"
	write(func(db *sql.Conn) error { return appendParent(db, tail) })
	visible, _ := read(parent.ID)
	setVisibility := func(db *sql.Conn, visible bool) error {
		change := canonical.NewRecord(turn, "operation", canonical.Operation{Kind: "visibility", Data: &canonical.Visibility{Message: tail.ID, UserVisible: visible, AgentVisible: visible}}, now)
		runtime := canonical.DefaultRuntime()
		runtime.UserVisible = false
		change.Metadata.Runtime = &runtime
		return appendParent(db, change)
	}
	write(func(db *sql.Conn) error { return setVisibility(db, false) })
	hidden, work := read(parent.ID)
	if hidden.ContextEstimate == nil || visible.ContextEstimate == nil || *hidden.ContextEstimate >= *visible.ContextEstimate || work.eventRows != 1 || work.callReads != 0 {
		t.Fatalf("visibility did not update the retained context estimate: before=%+v, after=%+v, work=%+v", visible, hidden, work)
	}
	var fork protocol.Session
	write(func(db *sql.Conn) error {
		if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		var err error
		fork, _, err = forkConversation(ctx, db, protocol.CommandForkConversationPayload{Session: parent.ID, Through: turn, ExpectedRevision: parent.Revision})
		return err
	})
	if result, _ := read(fork.ID); result.Delegated.Responses != 1 || !reflect.DeepEqual(result.ContextEstimate, hidden.ContextEstimate) {
		t.Fatalf("fork missed inherited usage or visibility: %+v", result)
	}
	write(func(db *sql.Conn) error {
		if err := setVisibility(db, true); err != nil {
			return err
		}
		return appendChild(db)
	})
	if result, work := read(fork.ID); result.Delegated.Responses != 2 || !reflect.DeepEqual(result.ContextEstimate, hidden.ContextEstimate) || work != (agentStatisticsWork{usageRows: 1, eventRows: 1}) {
		t.Fatalf("fork ignored updated delegated usage or adopted source visibility: %+v, work %+v", result, work)
	}
	write(func(db *sql.Conn) error {
		revision, err := historyRevision(ctx, db, parent.ID)
		if err != nil {
			return err
		}
		_, _, err = rewindConversation(ctx, db, protocol.CommandRewindConversationPayload{Session: parent.ID, ExpectedHead: turn, ExpectedRevision: revision}, false)
		return err
	})
	if result, work := read(parent.ID); result.Turns != 0 || result.Observations.Children != 0 || result.Delegated.Responses != 0 || result.ContextEstimate != nil || work.callReads != 1 {
		t.Fatalf("rewind retained old membership: %+v, work %+v", result, work)
	}
	if result, work := read(fork.ID); result.Delegated.Responses != 2 || work != (agentStatisticsWork{}) {
		t.Fatalf("rewind invalidated a separate branch: %+v, work %+v", result, work)
	}
}

func BenchmarkReadAgentStatisticsHistory(b *testing.B) {
	storage, node, session, turn, now := agentHistoryFixture(b, protocol.WorkModeCode)
	ctx := b.Context()
	text := strings.Repeat("a", 8<<10)
	const turns, responses = 20, 8
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		for i := range turns {
			for j := range responses {
				message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, text))
				message.Usage = &canonical.Usage{Input: ptr(uint64(100)), Output: ptr(uint64(20))}
				record := canonical.NewRecord(turn, "message", &message, now)
				record.Provenance.Author = "assistant"
				attempt := protocol.DefaultConversationObservationAttempt()
				attempt.Index, attempt.ElapsedUs = 1, ptr(uint64(10))
				attempt.Outcome, attempt.Transport = protocol.ConversationObservationOutcomeSucceeded, "http"
				record.Metadata.Execution = &canonical.Execution{Observation: &protocol.ConversationObservationRequest{
					ID: fmt.Sprintf("%d/%d", i, j), Turn: turn, Model: "fixture", Attempts: []protocol.ConversationObservationAttempt{attempt},
				}}
				if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
					return err
				}
			}
			if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
				return err
			}
			if i+1 < turns {
				var err error
				if turn, err = delegationFixtureParent(ctx, db, node, session, now); err != nil {
					return err
				}
			}
		}
		return nil
	}); err != nil {
		b.Fatal(err)
	}
	var committed *AgentStatisticsHistory
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		_, committed, err = ReadAgentStatisticsReusing(ctx, db, session.ID, nil)
		return err
	}); err != nil {
		b.Fatal(err)
	}
	for _, test := range []struct {
		name  string
		prior *AgentStatisticsHistory
	}{{"full_aggregation", nil}, {"reuse_unchanged", committed}} {
		b.Run(test.name, func(b *testing.B) {
			b.ReportAllocs()
			var next *AgentStatisticsHistory
			ctx := b.Context()
			for b.Loop() {
				if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
					var err error
					_, next, err = ReadAgentStatisticsReusing(ctx, db, session.ID, test.prior)
					return err
				}); err != nil {
					b.Fatal(err)
				}
			}
			if !reflect.DeepEqual(next.result, committed.result) || next.result.Responses != turns*responses {
				b.Fatal("statistics fixture lost a recorded response or changed the aggregate")
			}
			b.ReportMetric(float64(next.work.usageRows), "usage-rows/op")
			b.ReportMetric(float64(next.work.eventRows), "event-rows/op")
			b.ReportMetric(float64(next.work.callReads), "call-queries/op")
		})
	}
}
