package acceptance

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"testing"
	"time"

	core "github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/live"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSandboxRecordingPreservesApprovalAndRawCompletion(t *testing.T) {
	const command = "printf 'SAILRY-SANDBOX-REPLAY\\n'"
	fixture := replayFixture()
	turn := fixture.History.Page.Runs[0].Turn
	call, err := copyLiveValue[protocol.ConversationPartToolCallPayload](fixture.History.Page.Entries[1].Parts[2].Data)
	if err != nil {
		t.Fatal(err)
	}
	resolved := protocol.DefaultToolDetailResolved()
	resolved.Input.Content = &protocol.ToolDetailText{Text: command, Code: true}
	call.Summary.Resolved = &resolved
	fixture.History.Page.Entries[1].Parts[2].Data = call
	approval := protocol.DefaultConversationApproval()
	approval.ID, approval.Session, approval.Turn = protocol.ApprovalID{5}, fixture.Session, turn
	approval.Entry, approval.Index = fixture.History.Page.Entries[1].ID, 2
	approval.Source, approval.State = protocol.ConversationApprovalSourceUser, protocol.ConversationApprovalStatePending
	access := protocol.ProcessAccessUnrestricted
	approval.Access = &access
	pending := protocol.DefaultConversationSnapshot()
	pending.Node, pending.Sequence, pending.Page.Session = fixture.Node, 2, fixture.Session
	run := fixture.History.Page.Runs[0]
	run.Status, run.FinishedMs = protocol.ConversationStatusRunning, nil
	pending.Page.Runs = []protocol.ConversationRun{run}
	pending.Page.Entries = slices.Clone(fixture.History.Page.Entries[:2])
	pending.Page.Approvals = []protocol.ConversationApproval{approval}
	approved := approval
	approved.State = protocol.ConversationApprovalStateApproved
	approved.Choice = &protocol.ConversationApprovalChoice{Kind: "once"}
	fixture.History.Page.Approvals = []protocol.ConversationApproval{approved}
	fixture.Frames[1].Snapshot.Page = fixture.History.Page
	fixture.Frames = []core.RecordingFrame{fixture.Frames[0], {At: 20 * time.Millisecond, Snapshot: pending}, fixture.Frames[1]}
	detail := fixture.Tools[0].Detail
	detail.Arguments, err = json.Marshal(map[string]any{"command": command, "access": "unrestricted", "background": false, "timeout_ms": 5000})
	if err != nil {
		t.Fatal(err)
	}
	completion := protocol.DefaultProcessCompletion()
	completion.Outcome = protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}
	completion.Stdout.Text = "SAILRY-SANDBOX-REPLAY\n"
	raw, err := json.Marshal(map[string]any{"kind": "command_result", "data": completion, "sailry_command": map[string]any{"id": protocol.RequestID{6}}})
	if err != nil {
		t.Fatal(err)
	}
	result := json.RawMessage(raw)
	detail.Result = &result
	detail.Content = &protocol.ToolContent{Version: 1, Blocks: []protocol.ToolContentBlock{{Kind: "text", Data: protocol.ToolContentText{Path: "/data/stdout/text"}}}}
	encoded, err := json.Marshal(detail)
	if err != nil {
		t.Fatal(err)
	}
	directory := filepath.Join(t.TempDir(), "acceptance")
	if err := core.Own(directory, fixture.Node, fixture.Session); err != nil {
		t.Fatal(err)
	}
	recorder, err := core.NewRecorder(directory, fixture.Node, fixture.Session)
	if err != nil {
		t.Fatal(err)
	}
	for _, frame := range fixture.Frames {
		if err := recorder.Append(frame); err != nil {
			t.Fatal(err)
		}
	}
	foreign := fixture.Frames[2]
	foreign.Snapshot.Page.Session = protocol.SessionID{9}
	if err := recorder.Append(foreign); err == nil {
		t.Fatal("sandbox recording accepted a foreign session")
	}
	if err := recorder.Flush(); err != nil {
		t.Fatal(err)
	}
	reads := []string{}
	controller := client.New(&liveFixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (protocol.Output, error) {
		reads = append(reads, request.Command.Kind)
		switch request.Command.Kind {
		case "read_conversation":
			input := request.Command.Data.(*protocol.CommandReadConversationPayload)
			if input.Session != fixture.Session {
				t.Fatal("sandbox capture read a foreign session")
			}
			return protocol.Output{Kind: "conversation", Data: fixture.History}, nil
		case "read_tool_detail":
			input := request.Command.Data.(*protocol.CommandReadToolDetailPayload)
			ref := fixture.Tools[0].Reference
			if input.Session != ref.Session || input.Turn != ref.Turn || input.Entry != ref.Entry || input.Index != ref.Index || input.ExpectedRevision != ref.Revision {
				t.Fatal("sandbox capture changed its canonical tool reference")
			}
			return protocol.Output{Kind: "tool_detail", Data: protocol.ToolDetailPage{Reference: ref, TotalBytes: uint64(len(encoded)), Text: string(encoded)}}, nil
		default:
			t.Fatalf("sandbox recording attempted an execution or approval: %s", request.Command.Kind)
			return protocol.Output{}, nil
		}
	}})
	t.Cleanup(func() { _ = controller.Close() })
	if err := core.CaptureRecording(t.Context(), controller, directory, fixture.Session); err != nil {
		t.Fatal(err)
	}
	if !slices.Equal(reads, []string{"read_conversation", "read_tool_detail"}) {
		t.Fatalf("sandbox capture reads = %v", reads)
	}
	// A later frame flush must not replace the separately captured raw result.
	if err := recorder.Flush(); err != nil {
		t.Fatal(err)
	}
	recording, err := core.ReadRecording(directory, fixture.Session)
	if err != nil || recording.Truncated || len(recording.Frames) != 3 || len(recording.Tools) != 1 || recording.Tools[0].Detail == nil {
		t.Fatalf("sandbox recording incomplete: %+v, %v", recording, err)
	}
	_, _, waiting, _ := live.Conversation(client.ConversationState{Snapshot: recording.Frames[1].Snapshot, Connected: true}, "en")
	if len(waiting.Approvals) != 1 || waiting.Approvals[0].ID != approval.ID.String() || !waiting.Approvals[0].Unrestricted || waiting.Approvals[0].Remember || waiting.Approvals[0].Prefix != "" || waiting.Approvals[0].Description != command {
		t.Fatalf("pending unrestricted approval lost its exact command or once-only scope: %+v", waiting.Approvals)
	}
	_, _, waiting, _ = live.Conversation(client.ConversationState{Snapshot: recording.Frames[2].Snapshot, Connected: true}, "en")
	if len(waiting.Approvals) != 0 || len(recording.History.Page.Approvals) != 1 || recording.History.Page.Approvals[0].Choice == nil || recording.History.Page.Approvals[0].Choice.Kind != "once" {
		t.Fatal("recording did not retain the resolved one-time approval")
	}
	stored := recording.Tools[0].Detail
	if !bytes.Equal(stored.Arguments, detail.Arguments) || stored.Result == nil || !bytes.Equal(*stored.Result, raw) {
		t.Fatal("recording changed the original command arguments or raw process completion")
	}
	before, err := os.ReadFile(core.RecordingPath(directory, fixture.Session))
	if err != nil {
		t.Fatal(err)
	}
	report := RunReplay(t.Context(), directory, nil)
	if !report.OK() || len(report.Cases) != 1 {
		t.Fatalf("sandbox approval and completion replay: %+v", report)
	}
	after, err := os.ReadFile(core.RecordingPath(directory, fixture.Session))
	if err != nil || !bytes.Equal(before, after) {
		t.Fatal("sandbox replay modified its original recording")
	}
}
