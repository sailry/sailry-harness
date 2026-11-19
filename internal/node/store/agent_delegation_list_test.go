package store

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestListSubagentsAcrossParentTurns(t *testing.T) {
	t.Parallel()
	storage, node, session, firstParent, now := delegationPermissionFixture(t, protocol.PermissionAsk, true)
	ctx := t.Context()
	var older, working, foreign, newest, continued AgentInvocation
	var parent, otherParent protocol.TurnID
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		older, err = listFixtureChild(ctx, db, node, firstParent, now, map[string]any{"task": "private child prompt", "title": "Evidence", "role": "reviewer"})
		if err != nil {
			return err
		}
		if err = claimListFixtureChild(ctx, db, older, now); err != nil {
			return err
		}
		reply := canonical.NewRecord(older.Run.Turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "private completed answer")), now)
		reply.Provenance.Author = "assistant"
		if _, _, err = AppendCanonical(ctx, db, older.Session.ID, reply); err != nil {
			return err
		}
		if _, _, err = FinishTurn(ctx, db, older.Run.Turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		if _, err = ConsumeChildBackground(ctx, db, firstParent, older.Session.ID, &older.Run.Turn); err != nil {
			return err
		}
		working, err = listFixtureChild(ctx, db, node, firstParent, now, map[string]any{"task": "untitled child prompt"})
		if err != nil {
			return err
		}
		if err = claimListFixtureChild(ctx, db, working, now); err != nil {
			return err
		}
		guidance, err := delegationFixtureCall(ctx, db, firstParent, "guide_agent", map[string]any{"session": working.Session.ID, "message": "private guidance"}, now)
		if err != nil {
			return err
		}
		if _, next, _, err := GuideChild(ctx, db, node, firstParent, guidance, guidance.Arguments, now); err != nil || next != nil {
			return fmt.Errorf("running guidance should not create another invocation: %v", err)
		}
		if _, _, err = FinishTurn(ctx, db, firstParent, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		parent, err = delegationFixtureParent(ctx, db, node, session, now)
		if err != nil {
			return err
		}
		other, _, err := createSession(ctx, db, node, storage.Profile, protocol.CommandCreateSessionPayload{Config: &session.Config})
		if err != nil {
			return err
		}
		otherParent, err = delegationFixtureParent(ctx, db, node, other.Data.(protocol.Session), now)
		if err != nil {
			return err
		}
		foreign, err = listFixtureChild(ctx, db, node, otherParent, now, map[string]any{"task": "unrelated child", "title": "Evidence"})
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	read := func(turn protocol.TurnID, options SubagentListOptions) (SubagentPage, error) {
		var page SubagentPage
		err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			page, err = ListSubagents(ctx, db, turn, options)
			return err
		})
		return page, err
	}
	first, err := read(parent, SubagentListOptions{Limit: new(uint16(1))})
	if err != nil || first.NextBefore == nil || len(first.Subagents) != 1 {
		t.Fatalf("first page = %+v, %v", first, err)
	}
	if want := (SubagentSummary{Session: working.Session.ID, Turn: working.Run.Turn, Status: protocol.ConversationStatusRunning}); first.Subagents[0] != want {
		t.Fatalf("running session was not selected over newer guidance: %+v", first.Subagents[0])
	}
	second, err := read(parent, SubagentListOptions{Before: first.NextBefore, Limit: new(uint16(1))})
	if err != nil || second.NextBefore != nil || len(second.Subagents) != 1 {
		t.Fatalf("second page = %+v, %v", second, err)
	}
	entry := second.Subagents[0]
	if entry.Session != older.Session.ID || entry.Turn != older.Run.Turn || entry.Status != protocol.ConversationStatusCompleted || entry.Title != "Evidence" || entry.Role == nil || entry.Role.Key != "reviewer" || entry.Role.Name != "Reviewer" {
		t.Fatalf("completed entry = %+v", entry)
	}
	if raw, err := encode(second); err != nil || strings.Contains(string(raw), "private") {
		t.Fatalf("list exposed non-metadata: %s, %v", raw, err)
	}
	if other, err := read(otherParent, SubagentListOptions{}); err != nil || len(other.Subagents) != 1 || other.Subagents[0].Session != foreign.Session.ID {
		t.Fatalf("list crossed parent session boundary: %+v, %v", other, err)
	}
	for _, row := range []struct {
		turn    protocol.TurnID
		options SubagentListOptions
		code    protocol.ErrorCode
	}{
		{turn: working.Run.Turn, code: protocol.ErrorCodePermissionDenied},
		{turn: firstParent, code: protocol.ErrorCodeConflict},
		{turn: parent, options: SubagentListOptions{Limit: new(uint16(51))}, code: protocol.ErrorCodeInvalidRequest},
		{turn: parent, options: SubagentListOptions{Before: new("9223372036854775808")}, code: protocol.ErrorCodeInvalidRequest},
	} {
		_, err := read(row.turn, row.options)
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != row.code {
			t.Fatalf("list through %s returned %v, want %s", row.turn, err, row.code)
		}
	}

	err = storage.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		newest, err = listFixtureChild(ctx, db, node, parent, now, map[string]any{"task": "later child"})
		if err != nil {
			return err
		}
		continued, err = listFixtureChild(ctx, db, node, parent, now, map[string]any{"session": older.Session.ID, "task": "explicit follow-up"})
		if err != nil {
			return err
		}
		return setQueuePaused(ctx, db, older.Session.ID, true)
	})
	if err != nil {
		t.Fatal(err)
	}
	second, err = read(parent, SubagentListOptions{Before: first.NextBefore})
	if err != nil || second.NextBefore != nil || len(second.Subagents) != 1 {
		t.Fatalf("older page shifted after new work: %+v, %v", second, err)
	}
	if entry := second.Subagents[0]; entry.Session != older.Session.ID || entry.Turn != continued.Run.Turn || continued.Run.Turn == older.Run.Turn || entry.Status != protocol.ConversationStatusQueued || !entry.Paused {
		t.Fatalf("reused session should retain its place and show the current turn: %+v", entry)
	}
	if fresh, err := read(parent, SubagentListOptions{}); err != nil || fresh.NextBefore != nil || len(fresh.Subagents) != 3 || fresh.Subagents[0].Session != newest.Session.ID {
		t.Fatalf("fresh page did not include each owned child once: %+v, %v", fresh, err)
	}
	// A current execution may never inherit access merely from its session's founding row.
	schemaExecute(t, storage.Node, "UPDATE agent_delegations SET body=json_set(body,'$.session',?2) WHERE turn=?1", continued.Run.Turn.String(), foreign.Session.ID.String())
	var fault *protocol.Fault
	if _, err = read(parent, SubagentListOptions{}); !errors.As(err, &fault) || fault.Code != protocol.ErrorCodePermissionDenied {
		t.Fatalf("listed a current execution owned by a different parent: %v", err)
	}
}

func TestListSubagentsStopEvidence(t *testing.T) {
	t.Parallel()
	storage, node, _, parent, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	type outcome struct {
		status protocol.ConversationStatus
		reason string
	}
	want := map[protocol.TurnID]outcome{}
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		for _, row := range []struct {
			outcome
			receipt bool
		}{
			{outcome{protocol.ConversationStatusCancelled, ""}, false},
			{outcome{protocol.ConversationStatusInterrupted, ""}, false},
			{outcome{protocol.ConversationStatusStopping, "stop_requested"}, false},
			{outcome{protocol.ConversationStatusCancelled, "stop_requested"}, true},
			{outcome{protocol.ConversationStatusInterrupted, "stop_requested"}, true},
		} {
			child, err := listFixtureChild(ctx, db, node, parent, now, map[string]any{"task": "private stop fixture"})
			if err != nil {
				return err
			}
			if err = claimListFixtureChild(ctx, db, child, now); err != nil {
				return err
			}
			if row.receipt {
				request := protocol.NewRequest(node, protocol.Command{Kind: "stop_turn", Data: &protocol.CommandStopTurnPayload{Turn: child.Run.Turn}})
				body, err := encode(request)
				if err != nil {
					return err
				}
				if _, err = admission.Admit(ctx, db, node, request, body); err != nil {
					return err
				}
				output, _, err := ExecuteAgentCommand(ctx, db, node, storage.Profile, node, request, now)
				if err != nil {
					return err
				}
				if err = admission.Complete(ctx, db, node, request.ID, link.Response{Output: output}); err != nil {
					return err
				}
			}
			if row.status == protocol.ConversationStatusStopping {
				_, err = stopAgentTurn(ctx, db, child.Run.Turn, now)
			} else {
				_, _, err = FinishTurn(ctx, db, child.Run.Turn, row.status, nil, now)
			}
			if err != nil {
				return err
			}
			want[child.Run.Turn] = row.outcome
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	check := func(storage *Store) {
		t.Helper()
		var page SubagentPage
		if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			page, err = ListSubagents(ctx, db, parent, SubagentListOptions{})
			return err
		}); err != nil || len(page.Subagents) != len(want) {
			t.Fatalf("list with stops = %+v, %v", page, err)
		}
		for _, child := range page.Subagents {
			if exact, exists := want[child.Turn]; !exists || (outcome{child.Status, child.StopReason}) != exact {
				t.Fatalf("child %s has status %q and reason %q, want %+v", child.Turn, child.Status, child.StopReason, exact)
			}
		}
	}
	check(storage)
	// Simulate the pre-marker schema with active stop receipts and reopen it
	// through the normal migration before expiring the old request ledger.
	schemaExecute(t, storage.Node, "ALTER TABLE agent_delegations DROP COLUMN stop_requested")
	if err = storage.Close(); err != nil {
		t.Fatal(err)
	}
	upgraded := reopenSchemaFixture(t, storage.Profile, node)
	if err = upgraded.Node.Write(ctx, func(db *sql.Conn) error {
		removed, err := admission.Prune(ctx, db, time.Now().Add(admission.Retention+time.Hour))
		if err == nil && removed < 2 {
			return fmt.Errorf("did not prune the stop receipts: %d", removed)
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err = upgraded.Node.Write(ctx, func(db *sql.Conn) error {
		for turn, expected := range want {
			if expected.status == protocol.ConversationStatusCancelled && expected.reason == "" {
				request := protocol.NewRequest(node, protocol.Command{Kind: "stop_turn", Data: &protocol.CommandStopTurnPayload{Turn: turn}})
				_, _, err := ExecuteAgentCommand(ctx, db, node, upgraded.Profile, node, request, now)
				return err
			}
		}
		return fmt.Errorf("missing independently cancelled turn")
	}); err != nil {
		t.Fatal(err)
	}
	check(upgraded)
}

func listFixtureChild(ctx context.Context, db *sql.Conn, node protocol.NodeID, parent protocol.TurnID, now time.Time, args map[string]any) (AgentInvocation, error) {
	call, err := delegationFixtureCall(ctx, db, parent, "spawn_agent", args, now)
	if err != nil {
		return AgentInvocation{}, err
	}
	child, _, err := DelegateAgent(ctx, db, node, parent, call, nil, true, now)
	return child, err
}

func claimListFixtureChild(ctx context.Context, db *sql.Conn, child AgentInvocation, now time.Time) error {
	inv, _, err := ClaimChild(ctx, db, child.Run.Turn, now)
	if err == nil && inv == nil {
		err = fmt.Errorf("child %s was not claimed", child.Run.Turn)
	}
	return err
}
