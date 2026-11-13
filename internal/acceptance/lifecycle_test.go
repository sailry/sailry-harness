package acceptance

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func testReportDirectory(t *testing.T) string {
	t.Helper()
	return filepath.Join(t.TempDir(), "acceptance")
}

func lifecycleSession(id protocol.SessionID) protocol.Session {
	session := protocol.DefaultSession()
	session.ID, session.Worktree, session.Revision = id, protocol.WorktreeID{1}, 23
	session.Config.Provider, session.Config.Model = protocol.ProviderID{1}, "fixture"
	session.Config.Mode, session.Config.Permission = protocol.WorkModeCode, protocol.PermissionProject
	return session
}

func fixtureFault(request protocol.Request, code protocol.ErrorCode) link.Admission {
	done := make(chan link.Response, 1)
	done <- link.Response{Fault: &protocol.Fault{Code: code, Message: "fixture failure"}}
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: done}
}

func TestOwnershipMergesAndRejectsOtherNode(t *testing.T) {
	directory := filepath.Join(testReportDirectory(t), "report")
	node := protocol.NodeID{1}
	first, second := protocol.SessionID{1}, protocol.SessionID{2}
	for _, id := range []protocol.SessionID{first, second, first} {
		if err := Own(directory, node, id); err != nil {
			t.Fatal(err)
		}
	}
	before, err := os.ReadFile(filepath.Join(directory, "ownership.json"))
	if err != nil {
		t.Fatal(err)
	}
	if err := Own(directory, protocol.NodeID{2}, protocol.SessionID{3}); err == nil {
		t.Fatal("another Node overwrote acceptance ownership")
	}
	after, err := os.ReadFile(filepath.Join(directory, "ownership.json"))
	if err != nil || string(before) != string(after) {
		t.Fatal("rejected ownership changed the ledger")
	}
	ledger, err := loadOwnership(directory, node)
	if err != nil || len(ledger.Sessions) != 2 || !ledger.owns(first) || !ledger.owns(second) {
		t.Fatalf("merged ownership = %#v, %v", ledger, err)
	}
	if runtime.GOOS != "windows" {
		for _, path := range []string{directory, filepath.Join(directory, "ownership.json")} {
			info, err := os.Stat(path)
			if err != nil || info.Mode().Perm()&0o077 != 0 {
				t.Fatalf("ownership is not private: %s", path)
			}
		}
	}
}

func TestArchiveRejectsUnownedAndOtherNodeBeforeDispatch(t *testing.T) {
	directory := testReportDirectory(t)
	if err := Own(directory, protocol.NodeID{2}, protocol.SessionID{1}); err != nil {
		t.Fatal(err)
	}
	controller := client.New(&fixtureTransport{dispatch: func(context.Context, protocol.Request) (link.Admission, error) {
		t.Fatal("a different Node dispatched cleanup")
		return link.Admission{}, nil
	}})
	t.Cleanup(func() { _ = controller.Close() })
	if err := Archive(t.Context(), controller, directory, []protocol.SessionID{{1}}); err == nil {
		t.Fatal("another Node was accepted")
	}
	other := testReportDirectory(t)
	if err := Own(other, controller.Target(), protocol.SessionID{1}); err != nil {
		t.Fatal(err)
	}
	if err := Archive(t.Context(), controller, other, []protocol.SessionID{{2}}); err == nil {
		t.Fatal("an unowned session was accepted")
	}
}

func TestCleanupUsesExplicitReportIDsAndCurrentRevision(t *testing.T) {
	directory := testReportDirectory(t)
	ids := []protocol.SessionID{{1}, {2}, {3}, {4}}
	if err := Own(directory, protocol.NodeID{1}, ids[0]); err != nil {
		t.Fatal(err)
	}
	report := Report{Version: 1, Suite: "backend", Cases: []CaseResult{{Session: ids[1].String(), OwnedSessions: []string{ids[2].String()}, Evidence: []Evidence{{Kind: "fork", Value: ids[3].String() + " preserves two turns"}, {Kind: "title", Value: "Acceptance user session"}}}}}
	report.Cases = append(report.Cases, CaseResult{Group: "ui", Session: "sailry-go/theme"}, CaseResult{Group: "replay", Session: (protocol.SessionID{99}).String()})
	if _, err := WriteReport(directory, report); err != nil {
		t.Fatal(err)
	}
	removed := map[protocol.SessionID]int{}
	reads := 0
	transport := &fixtureTransport{dispatch: func(ctx context.Context, request protocol.Request) (link.Admission, error) {
		if ctx.Err() != nil {
			t.Fatal("cleanup inherited caller cancellation")
		}
		var target struct {
			Session protocol.SessionID `json:"session"`
		}
		data, _ := json.Marshal(request.Command.Data)
		if json.Unmarshal(data, &target) != nil || !slices.Contains(ids, target.Session) {
			t.Fatal("cleanup escaped explicit ownership")
		}
		var output protocol.Output
		switch request.Command.Kind {
		case "read_session":
			reads++
			if target.Session == ids[3] {
				return fixtureFault(request, protocol.ErrorCodeNotFound), nil
			}
			output = protocol.Output{Kind: "session", Data: lifecycleSession(target.Session)}
		case "read_conversation":
			history := protocol.DefaultConversationHistory()
			history.Page.Session = target.Session
			output = protocol.Output{Kind: "conversation", Data: history}
		case "stop_commands", "list_commands":
			output = protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}
		case "remove_session":
			input := request.Command.Data.(*protocol.CommandRemoveSessionPayload)
			if input.ExpectedRevision != 23 {
				t.Fatal("removal lost the current revision")
			}
			if _, err := ReadRecording(directory, target.Session); err != nil {
				t.Fatalf("removal preceded its recording: %v", err)
			}
			removed[target.Session]++
			output = protocol.Output{Kind: "sessions_removed", Data: []protocol.SessionID{target.Session}}
		default:
			t.Fatalf("unexpected cleanup request %q", request.Command.Kind)
		}
		return fixtureResponse(request, output), nil
	}}
	controller := client.New(transport)
	t.Cleanup(func() { _ = controller.Close() })
	canceled, cancel := context.WithCancel(t.Context())
	cancel()
	result, err := Cleanup(canceled, controller, directory, nil)
	if err != nil || !result.OK() || len(result.Cases) != 4 || len(removed) != 3 {
		t.Fatalf("cleanup = %#v, %v", result, err)
	}
	before := reads
	result, err = Cleanup(t.Context(), controller, directory, nil)
	if err != nil || !result.OK() || reads != before {
		t.Fatalf("cleanup was not idempotent: reads %d -> %d, %v", before, reads, err)
	}
	ledger, err := loadOwnership(directory, controller.Target())
	if err != nil || slices.ContainsFunc(ledger.Sessions, func(value ownershipSession) bool { return !value.Removed }) {
		t.Fatalf("removal tombstones missing: %#v, %v", ledger, err)
	}
}

func TestCleanupNoopPasses(t *testing.T) {
	controller := client.New(&fixtureTransport{dispatch: func(context.Context, protocol.Request) (link.Admission, error) {
		t.Fatal("empty cleanup dispatched a request")
		return link.Admission{}, nil
	}})
	t.Cleanup(func() { _ = controller.Close() })
	report, err := Cleanup(t.Context(), controller, testReportDirectory(t), nil)
	if err != nil || !report.OK() {
		t.Fatalf("empty cleanup = %#v, %v", report, err)
	}
}

func TestUnknownCreationIsRecoveredWithoutReplayingOrSavingConfig(t *testing.T) {
	directory := testReportDirectory(t)
	session := lifecycleSession(protocol.SessionID{1})
	session.Config.Features["private_fixture"] = "DO-NOT-RECORD-CONFIG"
	creates, inspections, removals := 0, 0, 0
	var original protocol.RequestID
	transport := &fixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (link.Admission, error) {
		switch request.Command.Kind {
		case "create_session":
			creates++
			original = request.ID
			done := make(chan link.Response)
			close(done)
			return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: true}, Completion: done}, nil
		case "inspect_request":
			inspections++
			input := request.Command.Data.(*protocol.CommandInspectRequestPayload)
			if input.ID != original || len(input.Digest) != 64 {
				t.Fatal("recovery changed request identity")
			}
			outcome := protocol.RequestOutcome{Kind: "admitted"}
			if inspections > 1 {
				output := protocol.Output{Kind: "session", Data: session}
				outcome = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &output}}
			}
			return fixtureResponse(request, protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: original, Outcome: outcome}}), nil
		case "read_session":
			return fixtureResponse(request, protocol.Output{Kind: "session", Data: session}), nil
		case "read_conversation":
			history := protocol.DefaultConversationHistory()
			history.Page.Session = session.ID
			return fixtureResponse(request, protocol.Output{Kind: "conversation", Data: history}), nil
		case "stop_commands", "list_commands":
			return fixtureResponse(request, protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}), nil
		case "remove_session":
			removals++
			return fixtureResponse(request, protocol.Output{Kind: "sessions_removed", Data: []protocol.SessionID{session.ID}}), nil
		default:
			t.Fatalf("unexpected command %q", request.Command.Kind)
			return link.Admission{}, nil
		}
	}}
	controller := client.New(transport)
	t.Cleanup(func() { _ = controller.Close() })
	_, err := Execute(t.Context(), controller, directory, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Config: &session.Config}})
	if fault, ok := nodeFault(err); !ok || fault.Code != protocol.ErrorCodeOutcomeUnknown {
		t.Fatalf("unknown admission = %v", err)
	}
	data, err := os.ReadFile(filepath.Join(directory, "ownership.json"))
	if err != nil || strings.Contains(string(data), "DO-NOT-RECORD-CONFIG") || !strings.Contains(string(data), original.String()) {
		t.Fatal("request ledger lost identity or retained configuration")
	}
	report, err := Cleanup(t.Context(), controller, directory, nil)
	if err != nil || !report.OK() || creates != 1 || inspections != 2 || removals != 1 {
		t.Fatalf("recovered cleanup: create=%d inspect=%d remove=%d, %v", creates, inspections, removals, err)
	}
}

func TestUnknownStopIsRetainedAndNeverReplayed(t *testing.T) {
	directory := testReportDirectory(t)
	session := lifecycleSession(protocol.SessionID{1})
	if err := Own(directory, protocol.NodeID{1}, session.ID); err != nil {
		t.Fatal(err)
	}
	stops, removes := 0, 0
	var original protocol.RequestID
	controller := client.New(&fixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (link.Admission, error) {
		switch request.Command.Kind {
		case "read_session":
			return fixtureResponse(request, protocol.Output{Kind: "session", Data: session}), nil
		case "stop_commands":
			stops++
			original = request.ID
			done := make(chan link.Response)
			close(done)
			return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: true}, Completion: done}, nil
		case "inspect_request":
			input := request.Command.Data.(*protocol.CommandInspectRequestPayload)
			if input.ID != original {
				t.Fatal("another request was inspected")
			}
			return fixtureResponse(request, protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: original, Outcome: protocol.RequestOutcome{Kind: "admitted"}}}), nil
		case "remove_session":
			removes++
		}
		return link.Admission{}, errors.New("unexpected operation")
	}})
	t.Cleanup(func() { _ = controller.Close() })
	for range 2 {
		report, err := Cleanup(t.Context(), controller, directory, nil)
		if err == nil || report.OK() {
			t.Fatal("an unresolved stop was reported as clean")
		}
	}
	if stops != 1 || removes != 0 {
		t.Fatalf("uncertain mutation replayed: stops=%d removes=%d", stops, removes)
	}
	ledger, err := loadOwnership(directory, controller.Target())
	if err != nil || len(ledger.Requests) != 1 || ledger.Requests[0].ID != original || ledger.Sessions[0].Removed {
		t.Fatal("unknown result was not retained")
	}
}

func TestCreationVariantsRecordEveryBackup(t *testing.T) {
	main, backup := lifecycleSession(protocol.SessionID{1}), lifecycleSession(protocol.SessionID{2})
	rewound := protocol.ConversationRewind{Session: main.ID, Backup: &backup}
	turn := protocol.DefaultQueuedTurn()
	turn.ID, turn.Session, turn.Request = protocol.TurnID{1}, main.ID, protocol.RequestID{1}
	turn.Config, turn.Kind, turn.Roles.Execution = main.Config, protocol.ConversationRunKindTask, protocol.ConversationExecutionModel
	for _, test := range []struct {
		kind   string
		output protocol.Output
	}{
		{"create_session", protocol.Output{Kind: "session", Data: main}},
		{"fork_conversation", protocol.Output{Kind: "session", Data: backup}},
		{"rewind_conversation", protocol.Output{Kind: "rewound", Data: rewound}},
		{"replace_turn", protocol.Output{Kind: "turn_replaced", Data: protocol.OutputTurnReplacedPayload{Turn: turn, History: &rewound}}},
	} {
		ids, err := createdSessions(test.kind, test.output)
		if err != nil || len(ids) != 1 || ids[0] == (protocol.SessionID{}) {
			t.Fatalf("%s backup = %v, %v", test.kind, ids, err)
		}
	}
}

func TestCleanupPreservesConflictsAndContinuesOtherOwnedSessions(t *testing.T) {
	directory := testReportDirectory(t)
	broken, good := protocol.SessionID{1}, protocol.SessionID{2}
	for _, id := range []protocol.SessionID{broken, good} {
		if err := Own(directory, protocol.NodeID{1}, id); err != nil {
			t.Fatal(err)
		}
	}
	removed := []protocol.SessionID{}
	controller := client.New(&fixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (link.Admission, error) {
		var input struct {
			Session protocol.SessionID `json:"session"`
		}
		data, _ := json.Marshal(request.Command.Data)
		_ = json.Unmarshal(data, &input)
		switch request.Command.Kind {
		case "read_session":
			return fixtureResponse(request, protocol.Output{Kind: "session", Data: lifecycleSession(input.Session)}), nil
		case "read_conversation":
			history := protocol.DefaultConversationHistory()
			history.Page.Session = input.Session
			return fixtureResponse(request, protocol.Output{Kind: "conversation", Data: history}), nil
		case "stop_commands", "list_commands":
			return fixtureResponse(request, protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}), nil
		case "remove_session":
			if input.Session == broken {
				return fixtureFault(request, protocol.ErrorCodeRevisionConflict), nil
			}
			removed = append(removed, input.Session)
			return fixtureResponse(request, protocol.Output{Kind: "sessions_removed", Data: []protocol.SessionID{input.Session}}), nil
		default:
			t.Fatalf("unexpected cleanup request %q", request.Command.Kind)
			return link.Admission{}, nil
		}
	}})
	t.Cleanup(func() { _ = controller.Close() })
	report, err := Cleanup(t.Context(), controller, directory, nil)
	if err == nil || report.OK() || len(removed) != 1 || removed[0] != good {
		t.Fatalf("one failure stopped other cleanup: %v, %#v", err, report)
	}
	ledger, err := loadOwnership(directory, controller.Target())
	if err != nil || len(ledger.Requests) != 0 || ledger.Sessions[0].Removed || !ledger.Sessions[1].Removed {
		t.Fatalf("conflict ownership was lost: %#v, %v", ledger, err)
	}
	if _, err := ReadRecording(directory, broken); err != nil {
		t.Fatalf("conflicted session lost its evidence: %v", err)
	}
}

func TestCleanupRecordsOwnedDescendantsBeforeCascadingRemoval(t *testing.T) {
	directory := testReportDirectory(t)
	parent, child := protocol.SessionID{1}, protocol.SessionID{2}
	for _, id := range []protocol.SessionID{parent, child} {
		if err := Own(directory, protocol.NodeID{1}, id); err != nil {
			t.Fatal(err)
		}
	}
	removals := 0
	controller := client.New(&fixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (link.Admission, error) {
		var input struct {
			Session protocol.SessionID `json:"session"`
		}
		data, _ := json.Marshal(request.Command.Data)
		_ = json.Unmarshal(data, &input)
		switch request.Command.Kind {
		case "read_session":
			return fixtureResponse(request, protocol.Output{Kind: "session", Data: lifecycleSession(input.Session)}), nil
		case "read_conversation":
			history := protocol.DefaultConversationHistory()
			history.Page.Session = input.Session
			if input.Session == parent {
				run := protocol.DefaultConversationRun()
				run.Worktree, run.Session, run.Turn = protocol.WorktreeID{1}, child, protocol.TurnID{2}
				run.Kind, run.Status = protocol.ConversationRunKindTask, protocol.ConversationStatusCompleted
				history.Page.Children = []protocol.ConversationChild{{Run: run, Origin: protocol.Delegation{Session: parent, Turn: protocol.TurnID{1}}}}
			}
			return fixtureResponse(request, protocol.Output{Kind: "conversation", Data: history}), nil
		case "stop_commands", "list_commands":
			return fixtureResponse(request, protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}), nil
		case "remove_session":
			removals++
			if input.Session != parent {
				t.Fatal("a cascading removal was repeated for its child")
			}
			for _, id := range []protocol.SessionID{parent, child} {
				if _, err := ReadRecording(directory, id); err != nil {
					t.Fatalf("cascading removal lost descendant evidence: %v", err)
				}
			}
			return fixtureResponse(request, protocol.Output{Kind: "sessions_removed", Data: []protocol.SessionID{parent, child}}), nil
		default:
			t.Fatalf("unexpected operation %q", request.Command.Kind)
			return link.Admission{}, nil
		}
	}})
	t.Cleanup(func() { _ = controller.Close() })
	report, err := Cleanup(t.Context(), controller, directory, nil)
	if err != nil || !report.OK() || removals != 1 {
		t.Fatalf("cascading cleanup = %#v, %v, removals=%d", report, err, removals)
	}
}
