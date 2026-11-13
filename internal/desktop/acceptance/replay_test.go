package acceptance

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	core "github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// This fixture is deliberately handwritten. It exercises the recorded data
// contract and the actual projection, disclosures and copy controls without a
// provider, Client, Node, subscription or tool executor.
func replayFixture() core.RecordingSession {
	node, session, turn := protocol.NodeID{1}, protocol.SessionID{2}, protocol.TurnID{3}
	started, finished := int64(1), int64(2)
	run := protocol.DefaultConversationRun()
	run.Session, run.Turn, run.Sequence = session, turn, 1
	run.Kind, run.Status, run.Worktree = protocol.ConversationRunKindTask, protocol.ConversationStatusRunning, protocol.WorktreeID{4}
	run.StartedMs = &started
	user := protocol.DefaultConversationEntry()
	user.ID, user.Sequence, user.Turn, user.Author = "recorded-user", 1, turn, "user"
	user.Parts = []protocol.ConversationPart{{Kind: "text", Data: "Inspect the recorded result"}}
	first := protocol.DefaultConversationSnapshot()
	first.Node, first.Sequence, first.Page.Session = node, 1, session
	first.Page.Runs, first.Page.Entries = []protocol.ConversationRun{run}, []protocol.ConversationEntry{user}
	draft := protocol.DefaultConversationDraft()
	draft.ID, draft.Turn, draft.Author = "recorded-draft", turn, "assistant"
	draft.Parts = []protocol.ConversationPart{{Kind: "thinking", Data: "Recorded private thinking"}, {Kind: "text", Data: "Before the tool. "}}
	first.Drafts = []protocol.ConversationDraft{draft}
	name := "recorded-tool"
	summary := protocol.DefaultToolDetailSummary()
	summary.Revision, summary.HasDetails = "result-1", true
	title := "Recorded command"
	summary.Title = &title
	call := protocol.DefaultConversationPartToolCallPayload()
	call.ID, call.Name, call.Summary, call.Presentation = &name, "run_command", &summary, protocol.ToolPresentationContent
	assistant := protocol.DefaultConversationEntry()
	assistant.ID, assistant.Sequence, assistant.Turn, assistant.Author = "recorded-call", 2, turn, "assistant"
	assistant.Parts = []protocol.ConversationPart{{Kind: "thinking", Data: "Recorded private thinking"}, {Kind: "text", Data: "Before the tool. "}, {Kind: "tool_call", Data: call}}
	result := protocol.DefaultConversationPartToolResultPayload()
	result.ID, result.Name, result.Summary = &name, call.Name, &summary
	tool := protocol.DefaultConversationEntry()
	tool.ID, tool.Sequence, tool.Turn, tool.Author = "recorded-result", 3, turn, "tool"
	tool.Parts = []protocol.ConversationPart{{Kind: "tool_result", Data: result}}
	answer := protocol.DefaultConversationEntry()
	answer.ID, answer.Sequence, answer.Turn, answer.Author = "recorded-answer", 4, turn, "assistant"
	answer.Parts = []protocol.ConversationPart{{Kind: "text", Data: "Final recorded answer."}}
	run.Status, run.FinishedMs = protocol.ConversationStatusCompleted, &finished
	history := protocol.DefaultConversationHistory()
	history.Sequence, history.Page.Session = 4, session
	history.Page.Runs, history.Page.Entries = []protocol.ConversationRun{run}, []protocol.ConversationEntry{user, assistant, tool, answer}
	final := protocol.DefaultConversationSnapshot()
	final.Node, final.Sequence, final.Page = node, history.Sequence, history.Page
	raw := json.RawMessage(`{"stdout":"Recorded original output","exit":7}`)
	detail := protocol.DefaultToolDetail()
	detail.Arguments, detail.Result = json.RawMessage(`{"command":"fixture"}`), &raw
	return core.RecordingSession{Version: 1, Node: node, Session: session, Captured: time.Unix(1, 0).UTC(), History: history,
		Frames: []core.RecordingFrame{{At: 0, Snapshot: first}, {At: 40 * time.Millisecond, Snapshot: final}},
		Tools:  []core.RecordingTool{{Reference: protocol.ToolDetailReference{Session: session, Turn: turn, Entry: tool.ID, Index: 0, Call: &name, Revision: summary.Revision}, Detail: &detail}}}
}

func TestRecordedReplay(t *testing.T) {
	t.Run("snapshot stream and original result", func(t *testing.T) {
		recording := replayFixture()
		directory := filepath.Join(t.TempDir(), "acceptance")
		path := core.RecordingPath(directory, recording.Session)
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			t.Fatal(err)
		}
		original, err := json.Marshal(recording)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, original, 0o600); err != nil {
			t.Fatal(err)
		}
		var emitted []core.CaseResult
		report := RunReplay(t.Context(), directory, func(result core.CaseResult) { emitted = append(emitted, result) })
		if !report.OK() || len(report.Cases) != 1 || len(emitted) != 1 {
			t.Fatalf("replay: %+v", report)
		}
		result := report.Cases[0]
		if result.Session != recording.Session.String() || !slices.Equal(result.Turns, []string{recording.History.Page.Runs[0].Turn.String()}) {
			t.Fatalf("replay lost recorded identities: %+v", result)
		}
		if !slices.ContainsFunc(result.Evidence, func(e core.Evidence) bool { return e.Kind == "replay_scope" && e.Value == "recorded-snapshots" }) {
			t.Fatalf("replay did not cover recorded snapshots: %+v", result.Evidence)
		}
		after, err := os.ReadFile(path)
		if err != nil || !bytes.Equal(original, after) || !bytes.Contains(after, []byte("Recorded private thinking")) {
			t.Fatalf("replay changed or removed original recording data: %v", err)
		}
	})
	t.Run("terminal-only and truncated scopes", func(t *testing.T) {
		for _, scope := range []string{"terminal-only", "truncated-snapshots"} {
			recording := replayFixture()
			if scope == "terminal-only" {
				recording.Frames = nil
			} else {
				recording.Truncated, recording.DroppedFrames = true, 2
			}
			result := replayRecording(t.Context(), recording, core.CaseResult{ID: "replay", Session: recording.Session.String(), Status: "failed"})
			if result.Status != "passed" || !slices.ContainsFunc(result.Evidence, func(e core.Evidence) bool { return e.Kind == "replay_scope" && e.Value == scope }) ||
				!strings.Contains(result.Detail, map[string]string{"terminal-only": "no streaming snapshots", "truncated-snapshots": "truncated"}[scope]) {
				t.Fatalf("%s claimed incorrect coverage: %+v", scope, result)
			}
		}
	})
	t.Run("reject wrong identities and reversed frames", func(t *testing.T) {
		for _, corrupt := range []func(*core.RecordingSession){
			func(r *core.RecordingSession) { r.Frames[0].Snapshot.Node = protocol.NodeID{9} },
			func(r *core.RecordingSession) { r.Frames[0].Snapshot.Page.Session = protocol.SessionID{9} },
			func(r *core.RecordingSession) { r.Frames[1].At = -time.Millisecond },
			func(r *core.RecordingSession) { r.Frames[0].Snapshot.Sequence = 10 },
			func(r *core.RecordingSession) { r.History.Page.Entries[1].Sequence = 1 },
		} {
			recording := replayFixture()
			corrupt(&recording)
			if err := validateRecording(recording); err == nil {
				t.Fatal("accepted a damaged recording")
			}
		}
	})
}
