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

	"github.com/egoist/mygo/ui"
	core "github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/client"
	profile "github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/live"
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
		if err := profile.PrivateDirectory(directory); err != nil {
			t.Fatal(err)
		}
		if err := profile.PrivateDirectory(filepath.Dir(path)); err != nil {
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

func replayStreamingFixture() core.RecordingSession {
	recording := replayFixture()
	run := recording.Frames[0].Snapshot.Page.Runs[0]
	user := recording.Frames[0].Snapshot.Page.Entries[0]
	user.Parts = []protocol.ConversationPart{{Kind: "text", Data: "Reply with Replay sample. Then extend it with a final sentence."}}
	recording.Frames, recording.Tools = nil, nil
	for index, text := range []string{"Replay sample.", "Replay sample. This is the final sentence."} {
		snapshot := protocol.DefaultConversationSnapshot()
		snapshot.Node, snapshot.Sequence, snapshot.Page.Session = recording.Node, 1, recording.Session
		snapshot.Page.Runs = []protocol.ConversationRun{run}
		snapshot.Page.Entries = []protocol.ConversationEntry{user}
		draft := protocol.DefaultConversationDraft()
		draft.ID, draft.Turn, draft.Author = "streaming-draft", run.Turn, "assistant"
		draft.Parts = []protocol.ConversationPart{{Kind: "text", Data: text}}
		snapshot.Drafts = []protocol.ConversationDraft{draft}
		recording.Frames = append(recording.Frames, core.RecordingFrame{At: time.Duration(index) * 40 * time.Millisecond, Snapshot: snapshot})
	}
	answer := protocol.DefaultConversationEntry()
	answer.ID, answer.Sequence, answer.Turn, answer.Author = "streaming-answer", 2, run.Turn, "assistant"
	answer.Parts = slices.Clone(recording.Frames[1].Snapshot.Drafts[0].Parts)
	run.Status = protocol.ConversationStatusCompleted
	recording.History = protocol.DefaultConversationHistory()
	recording.History.Sequence, recording.History.Page.Session = 2, recording.Session
	recording.History.Page.Runs = []protocol.ConversationRun{run}
	recording.History.Page.Entries = []protocol.ConversationEntry{user, answer}
	terminal := protocol.DefaultConversationSnapshot()
	terminal.Node, terminal.Sequence, terminal.Page = recording.Node, 2, recording.History.Page
	recording.Frames = append(recording.Frames, core.RecordingFrame{At: 80 * time.Millisecond, Snapshot: terminal})
	return recording
}

func TestRecordedReplayKeepsSameSequenceDraftGrowth(t *testing.T) {
	recording := replayStreamingFixture()
	result := replayRecording(t.Context(), recording, core.CaseResult{ID: "replay", Session: recording.Session.String(), Status: "failed"})
	if result.Status != "passed" || !slices.ContainsFunc(result.Evidence, func(e core.Evidence) bool {
		return e.Kind == "rendered_text" && strings.Contains(e.Value, "checked 4 plain reply tails")
	}) {
		t.Fatalf("the two draft frames and terminal answer were not rendered: %+v", result)
	}
	probe := &recordedReplyProbe{}
	parent := &window{ctx: t.Context(), result: &core.CaseResult{Session: recording.Session.String()}}
	for index, frame := range recording.Frames {
		turns, _, _, _ := live.Conversation(client.ConversationState{Snapshot: frame.Snapshot, Connected: true}, "en")
		if err := verifyRecordedText(frame.Snapshot, turns); err != nil {
			t.Fatal(err)
		}
		if checked, err := probe.check(parent, turns, 40*time.Millisecond); !checked || err != nil {
			t.Fatalf("frame %d did not render its recorded reply: %v", index, err)
		}
		want := "Replay sample. This is the final sentence."
		if index == 0 {
			want = "Replay sample."
		}
		if got := answerText(probe.cv.Turns[0].Reply); got != want || !recordedTextVisible(probe.w.tester, want) || probe.cv.Turns[0].User != "" {
			t.Fatalf("frame %d retained stale or duplicated text, or a masking prompt: %q", index, got)
		}
	}
	// The same words outside the transcript cannot stand in for a missing
	// reply. In particular, a user prompt must not make this check pass.
	want := "Replay sample. This is the final sentence."
	probe.w.tester = ui.NewTester(func(c *ui.Context) {
		ui.Column(c).Fill().Children(func() {
			ui.Text(c, want)
			ui.Column(c).Height(100).Role(ui.RoleLog).Children(func() { ui.Text(c, "Unrelated conversation") })
		})
	}, 600, 400)
	probe.w.tester.Frame()
	if recordedTextVisible(probe.w.tester, want) {
		t.Fatal("text outside the transcript masked the missing assistant reply")
	}
}

func TestRecordedTextRequiresItsTurnAndRole(t *testing.T) {
	for _, change := range []string{"unchanged", "missing_reply", "swapped_replies", "duplicated_reply", "wrong_user", "missing_turn"} {
		t.Run(change, func(t *testing.T) {
			snapshot := replayStreamingFixture().Frames[2].Snapshot
			first := snapshot.Page.Runs[0]
			second := first
			second.Turn, second.Sequence = protocol.TurnID{8}, 2
			snapshot.Page.Runs = append(snapshot.Page.Runs, second)
			// Both replies also occur in the user's input. A whole-transcript
			// substring search accepts their loss and the wrong turn's answer.
			snapshot.Page.Entries[0].Parts = []protocol.ConversationPart{{Kind: "text", Data: "First reply. Second reply."}}
			snapshot.Page.Entries[1].Parts = []protocol.ConversationPart{{Kind: "text", Data: "First reply."}}
			other := protocol.DefaultConversationEntry()
			other.ID, other.Sequence, other.Turn, other.Author = "second-reply", 3, second.Turn, "assistant"
			other.Parts = []protocol.ConversationPart{{Kind: "text", Data: "Second reply."}}
			snapshot.Page.Entries = append(snapshot.Page.Entries, other)
			turns, _, _, _ := live.Conversation(client.ConversationState{Snapshot: snapshot, Connected: true}, "en")
			switch change {
			case "missing_reply":
				turns[0].Reply.Blocks = nil
			case "swapped_replies":
				turns[0].Reply, turns[1].Reply = turns[1].Reply, turns[0].Reply
			case "duplicated_reply":
				turns[0].Reply.Blocks = append(turns[0].Reply.Blocks, turns[0].Reply.Blocks[0])
			case "wrong_user":
				turns[1].User, turns[0].User = turns[0].User, ""
			case "missing_turn":
				turns = turns[:1]
			}
			if err := verifyRecordedText(snapshot, turns); (err == nil) != (change == "unchanged") {
				t.Fatalf("%s text validation: %v", change, err)
			}
		})
	}
}

func TestRecordedTextRetainsJoinedUsersAndHidesContinuationPrompts(t *testing.T) {
	snapshot := replayStreamingFixture().Frames[2].Snapshot
	root := snapshot.Page.Runs[0]
	joined := root
	joined.Turn, joined.Sequence, joined.Joined = protocol.TurnID{8}, 2, &root.Turn
	continued := root
	continued.Turn, continued.Sequence = protocol.TurnID{9}, 3
	continued.Continuation = &protocol.ConversationContinuation{Package: protocol.PluginReference{Name: "fixture"}}
	queued := root
	queued.Turn, queued.Sequence, queued.Status = protocol.TurnID{10}, 4, protocol.ConversationStatusQueued
	snapshot.Page.Runs = append(snapshot.Page.Runs, joined, continued, queued)
	snapshot.Page.Queue.Items = []protocol.ConversationPending{{Turn: queued.Turn, Kind: protocol.ConversationRunKindTask, Revision: 1, ConfigRevision: 1}}
	for index, entry := range []struct {
		turn   protocol.TurnID
		author string
		text   string
	}{
		{joined.Turn, "user", "Joined user guidance"},
		{joined.Turn, "assistant", "Joined assistant answer"},
		{continued.Turn, "user", "Internal continuation prompt"},
		{continued.Turn, "assistant", "Public continuation answer"},
	} {
		value := protocol.DefaultConversationEntry()
		value.ID, value.Sequence, value.Turn, value.Author = "extra-"+string(rune('a'+index)), uint64(3+index), entry.turn, entry.author
		value.Parts = []protocol.ConversationPart{{Kind: "text", Data: entry.text}}
		snapshot.Page.Entries = append(snapshot.Page.Entries, value)
	}
	turns, _, _, _ := live.Conversation(client.ConversationState{Snapshot: snapshot, Connected: true}, "en")
	if err := verifyRecordedText(snapshot, turns); err != nil {
		t.Fatal(err)
	}
	if len(turns) != 2 || !strings.HasSuffix(turns[0].User, "\n\nJoined user guidance") || turns[1].User != "" || answerText(turns[1].Reply) != "Public continuation answer" {
		t.Fatalf("joined guidance or continuation visibility changed: %+v", turns)
	}
	turns[1].User = "Internal continuation prompt"
	if err := verifyRecordedText(snapshot, turns); err == nil {
		t.Fatal("an internal continuation prompt leaked into the replay")
	}
}

func TestRecordedReplyProbeDoesNotRequireOffscreenHistory(t *testing.T) {
	long := conversation.Turn{ID: "historical", User: strings.Repeat("Old user text. ", 800), Reply: conversation.Reply{Status: conversation.Completed,
		Blocks: []conversation.Block{{Kind: conversation.TextBlock, Text: strings.Repeat("Old answer.\n\n", 300)}}}}
	current := conversation.Turn{ID: "current", Reply: conversation.Reply{Status: conversation.Running,
		Blocks: []conversation.Block{{Kind: conversation.TextBlock, Text: strings.Repeat("A long current paragraph. ", 400) + "\n\nVisible current tail."}}}}
	probe := &recordedReplyProbe{}
	parent := &window{ctx: t.Context(), result: &core.CaseResult{Session: protocol.SessionID{2}.String()}}
	if checked, err := probe.check(parent, []conversation.Turn{long, current}, 40*time.Millisecond); !checked || err != nil {
		t.Fatalf("offscreen history failed the current reply check: %v", err)
	}
	if !recordedTextVisible(probe.w.tester, "Visible current tail.") || len(probe.cv.Turns) != 1 || probe.cv.Turns[0].ID != current.ID {
		t.Fatal("the probe did not focus the current recorded reply")
	}
}
