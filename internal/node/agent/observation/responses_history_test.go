package observation

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestResponsesHistoryOnlyHandsOffTheSameSession(t *testing.T) {
	var history ResponsesHistory
	session, other := protocol.NewSessionID(), protocol.NewSessionID()
	firstTurn, otherTurn, nextTurn := protocol.NewTurnID(), protocol.NewTurnID(), protocol.NewTurnID()
	first, firstTrace := history.Start(t.Context(), session, firstTurn)
	otherCtx, otherTrace := history.Start(t.Context(), other, otherTurn)
	if got := recordResponsesFixture(t, firstTrace, "first", 1); got != nil {
		t.Fatal("a new session invented a predecessor")
	}
	history.Complete(first)
	firstTrace.Clear()
	if got := recordResponsesFixture(t, otherTrace, "other", 1); got != nil {
		t.Fatal("an active execution inherited another session's receipt")
	}
	history.Complete(otherCtx)
	otherTrace.Clear()

	_, next := history.Start(t.Context(), session, nextTurn)
	got := recordResponsesFixture(t, next, "next", 1)
	if got == nil || got.PreviousRequestID != "first" || got.PreviousTurn != firstTurn || got.SharedInputItems != 1 {
		t.Fatalf("handoff = %+v, want first turn of the same session", got)
	}
	if got = recordResponsesFixture(t, next, "next", 2); got == nil || got.PreviousRequestID != "next" || got.PreviousAttempt != 1 || got.PreviousTurn != (protocol.TurnID{}) {
		t.Fatalf("same-turn retry = %+v, want preceding attempt and absent turn", got)
	}
}

func TestResponsesHistoryDropsUnmeasurableOrStaleBaselines(t *testing.T) {
	for _, scenario := range []string{"no attempt", "unmeasurable attempt", "same turn reclaimed", "eviction"} {
		t.Run(scenario, func(t *testing.T) {
			var history ResponsesHistory
			session, firstTurn := protocol.NewSessionID(), protocol.NewTurnID()
			first, trace := history.Start(t.Context(), session, firstTurn)
			recordResponsesFixture(t, trace, "first", 1)
			history.Complete(first)
			trace.Clear()

			checkTurn := protocol.NewTurnID()
			switch scenario {
			case "same turn reclaimed":
				checkTurn = firstTurn
			case "eviction":
				for range maxResponsesSessions {
					other, trace := history.Start(t.Context(), protocol.NewSessionID(), protocol.NewTurnID())
					recordResponsesFixture(t, trace, "other", 1)
					history.Complete(other)
					trace.Clear()
				}
				if len(history.recent) != maxResponsesSessions {
					t.Fatalf("retained %d baselines, want %d", len(history.recent), maxResponsesSessions)
				}
			default:
				middle, trace := history.Start(t.Context(), session, protocol.NewTurnID())
				if scenario == "unmeasurable attempt" {
					trace.Record("unmeasurable", 1, nil, nil)
				}
				history.Complete(middle)
				trace.Clear()
			}
			_, trace = history.Start(t.Context(), session, checkTurn)
			if got := recordResponsesFixture(t, trace, "later", 1); got != nil {
				t.Fatalf("claimed stale predecessor: %+v", got)
			}
		})
	}
}

func recordResponsesFixture(t *testing.T, trace *ResponsesTrace, request string, attempt uint64) *protocol.ConversationResponsesBoundary {
	t.Helper()
	_, shape := trace.Prepare([]byte(`{"input":[{"role":"user","content":"fixture"}]}`), nil)
	if shape == nil {
		t.Fatal("fixture must produce a comparable shape")
	}
	return trace.Record(request, attempt, nil, shape)
}
