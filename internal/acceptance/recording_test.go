package acceptance

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func recordingFrame(id protocol.SessionID, sequence uint64, text string) RecordingFrame {
	snapshot := protocol.DefaultConversationSnapshot()
	snapshot.Node, snapshot.Page.Session, snapshot.Sequence = protocol.NodeID{1}, id, sequence
	snapshot.Drafts = []protocol.ConversationDraft{{ID: "draft", Turn: protocol.TurnID{1}, Author: "assistant", Branch: "main", Parts: []protocol.ConversationPart{{Kind: "text", Data: text}}}}
	return RecordingFrame{At: time.Duration(sequence) * time.Millisecond, Snapshot: snapshot}
}

func TestRecordingPreservesFullFramesAndCanonicalToolDetails(t *testing.T) {
	directory := testReportDirectory(t)
	session := protocol.SessionID{1}
	if err := Own(directory, protocol.NodeID{1}, session); err != nil {
		t.Fatal(err)
	}
	recorder, err := NewRecorder(directory, protocol.NodeID{1}, session)
	if err != nil {
		t.Fatal(err)
	}
	first := recordingFrame(session, 5, "streamed first")
	if err := recorder.Append(first); err != nil {
		t.Fatal(err)
	}
	second := recordingFrame(session, 5, "streamed second")
	second.At += time.Millisecond
	if err := recorder.Append(second); err != nil {
		t.Fatal(err)
	}
	// Same sequence can carry a different draft. The caller's subsequent edit
	// must not alter the copied recording that is waiting to be flushed.
	second.Snapshot.Drafts[0].Parts[0].Data = "mutated outside recording"
	if err := recorder.Flush(); err != nil {
		t.Fatal(err)
	}
	view := toolFixtureView("read_file")
	view.page.Session = session
	view.run = protocol.DefaultConversationRun()
	view.run.Session, view.run.Turn, view.run.Worktree = session, protocol.TurnID{1}, protocol.WorktreeID{1}
	view.run.Kind, view.run.Status = protocol.ConversationRunKindTask, protocol.ConversationStatusCompleted
	view.page.Runs = []protocol.ConversationRun{view.run}
	detail := protocol.DefaultToolDetail()
	detail.Arguments = json.RawMessage(`{"path":"fixture.txt"}`)
	result := json.RawMessage(`{"text":"CANONICAL-RESULT"}`)
	detail.Result = &result
	encoded, err := json.Marshal(detail)
	if err != nil {
		t.Fatal(err)
	}
	ref := view.tools()[0].reference
	controller := client.New(&fixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (link.Admission, error) {
		switch request.Command.Kind {
		case "read_conversation":
			return fixtureResponse(request, protocol.Output{Kind: "conversation", Data: protocol.ConversationHistory{Sequence: 9, Page: view.page}}), nil
		case "read_tool_detail":
			input := request.Command.Data.(*protocol.CommandReadToolDetailPayload)
			if input.Session != session || input.Turn != ref.Turn || input.ExpectedRevision != ref.Revision {
				t.Fatal("tool capture escaped its original reference")
			}
			return fixtureResponse(request, protocol.Output{Kind: "tool_detail", Data: protocol.ToolDetailPage{Reference: ref, TotalBytes: uint64(len(encoded)), Text: string(encoded)}}), nil
		default:
			t.Fatalf("recording dispatched unexpected %q", request.Command.Kind)
			return link.Admission{}, nil
		}
	}})
	t.Cleanup(func() { _ = controller.Close() })
	if err := CaptureRecording(t.Context(), controller, directory, session); err != nil {
		t.Fatal(err)
	}
	// A repeated frame flush must also preserve canonical data captured later.
	if err := recorder.Flush(); err != nil {
		t.Fatal(err)
	}
	recording, err := ReadRecording(directory, session)
	if err != nil || len(recording.Frames) != 2 || len(recording.Tools) != 1 || recording.Tools[0].Detail == nil || recording.Truncated {
		t.Fatalf("recording = %#v, %v", recording, err)
	}
	got, err := decode[string](recording.Frames[1].Snapshot.Drafts[0].Parts[0].Data)
	if err != nil || got != "streamed second" {
		t.Fatalf("same-sequence draft was lost or mutated: %q, %v", got, err)
	}
	if recording.Tools[0].Detail.Result == nil || !strings.Contains(string(*recording.Tools[0].Detail.Result), "CANONICAL-RESULT") || recording.History.Sequence != 9 {
		t.Fatal("canonical result or history was lost")
	}
	data, err := os.ReadFile(RecordingPath(directory, session))
	if err != nil || len(data) > MaxRecordingBytes || strings.Contains(string(data), `"config"`) || strings.Contains(string(data), `"credential"`) || strings.Contains(string(data), `"providers"`) {
		t.Fatal("recording exceeded its scope or bound")
	}
	if runtime.GOOS != "windows" {
		for _, path := range []string{filepath.Join(directory, "recordings"), RecordingPath(directory, session)} {
			info, err := os.Stat(path)
			if err != nil || info.Mode().Perm()&0o077 != 0 {
				t.Fatal("recording permissions are not private")
			}
		}
	}
}

func TestRecordingBoundsAreExplicit(t *testing.T) {
	directory, session := testReportDirectory(t), protocol.SessionID{1}
	if err := Own(directory, protocol.NodeID{1}, session); err != nil {
		t.Fatal(err)
	}
	recorder, err := NewRecorder(directory, protocol.NodeID{1}, session)
	if err != nil {
		t.Fatal(err)
	}
	for index := range MaxRecordingFrames + 11 {
		if err := recorder.Append(recordingFrame(session, uint64(index), "bounded stream")); err != nil {
			t.Fatal(err)
		}
	}
	if err := recorder.Append(recordingFrame(session, MaxRecordingFrames+12, strings.Repeat("x", recordingFrameBudget+1))); err != nil {
		t.Fatal(err)
	}
	if err := recorder.Flush(); err != nil {
		t.Fatal(err)
	}
	recording, err := ReadRecording(directory, session)
	if err != nil || len(recording.Frames) != MaxRecordingFrames || !recording.Truncated || recording.DroppedFrames != 12 {
		t.Fatalf("truncation = frames %d dropped %d, %v", len(recording.Frames), recording.DroppedFrames, err)
	}
	if recording.Frames[0].Snapshot.Sequence != 0 || recording.Frames[len(recording.Frames)-1].Snapshot.Sequence != MaxRecordingFrames+10 {
		t.Fatal("recording did not retain its start and final bounded frame")
	}
	if err := recorder.Append(recordingFrame(protocol.SessionID{2}, MaxRecordingFrames+13, "foreign")); err == nil {
		t.Fatal("a foreign session entered the recording")
	}
	if err := recorder.Append(recordingFrame(session, 1, "backwards")); err == nil {
		t.Fatal("frame time moved backwards")
	}
}

func TestRecordingReferencesIncludeStandaloneToolResults(t *testing.T) {
	view := toolFixtureView("read_file")
	summary := protocol.DefaultToolDetailSummary()
	summary.Revision, summary.HasDetails = strings.Repeat("b", 64), true
	callID := "fixture-call"
	matched := protocol.DefaultConversationPartToolResultPayload()
	matched.ID, matched.Name, matched.Summary = &callID, "read_file", &summary
	standalone := matched
	standalone.ID = nil
	otherID := "other-call"
	unmatched := matched
	unmatched.ID = &otherID
	for index, result := range []protocol.ConversationPartToolResultPayload{matched, standalone, unmatched} {
		entry := protocol.DefaultConversationEntry()
		entry.ID, entry.Turn, entry.Branch = []string{"matched", "standalone", "unmatched"}[index], view.run.Turn, "fixture-branch"
		entry.Parts = []protocol.ConversationPart{{Kind: "tool_result", Data: result}}
		view.page.Entries = append(view.page.Entries, entry)
	}
	refs := recordingToolReferences(view.page)
	if len(refs) != 3 || refs[0].Entry != "matched" || refs[1].Entry != "standalone" || refs[1].Call != nil || refs[2].Entry != "unmatched" {
		t.Fatalf("recording omitted a visible result or lost final pairing: %#v", refs)
	}
	if len(view.tools()) != 1 {
		t.Fatal("recording changed the assertion's strict call pairing")
	}
}

func TestToolDetailReadFailurePreservesSessionAndPartialRecording(t *testing.T) {
	directory := testReportDirectory(t)
	view := toolFixtureView("read_file")
	view.run = protocol.DefaultConversationRun()
	view.run.Worktree, view.run.Session, view.run.Turn = protocol.WorktreeID{1}, view.page.Session, protocol.TurnID{1}
	view.run.Kind, view.run.Status = protocol.ConversationRunKindTask, protocol.ConversationStatusCompleted
	view.page.Runs = []protocol.ConversationRun{view.run}
	if err := Own(directory, protocol.NodeID{1}, view.page.Session); err != nil {
		t.Fatal(err)
	}
	removals := 0
	controller := client.New(&fixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (link.Admission, error) {
		switch request.Command.Kind {
		case "read_session":
			return fixtureResponse(request, protocol.Output{Kind: "session", Data: lifecycleSession(view.page.Session)}), nil
		case "read_conversation":
			return fixtureResponse(request, protocol.Output{Kind: "conversation", Data: protocol.ConversationHistory{Page: view.page}}), nil
		case "stop_commands", "list_commands":
			return fixtureResponse(request, protocol.Output{Kind: "commands", Data: []protocol.ProcessInfo{}}), nil
		case "read_tool_detail":
			return fixtureFault(request, protocol.ErrorCodeUnavailable), nil
		case "remove_session":
			removals++
		}
		t.Fatalf("unexpected operation %q", request.Command.Kind)
		return link.Admission{}, nil
	}})
	t.Cleanup(func() { _ = controller.Close() })
	report, err := Cleanup(t.Context(), controller, directory, nil)
	if err == nil || report.OK() || removals != 0 {
		t.Fatal("failed detail capture allowed session removal")
	}
	recording, err := ReadRecording(directory, view.page.Session)
	if err != nil || !recording.Truncated || len(recording.Tools) != 1 || recording.Tools[0].Error == "" {
		t.Fatalf("partial evidence was lost: %#v, %v", recording, err)
	}
}
