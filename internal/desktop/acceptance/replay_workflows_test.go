package acceptance

import (
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
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/live"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// These small recordings are handwritten regression fixtures, not recordings of
// a model or user session. They exercise the recorded wire format and the real
// projection and controls without an execution adapter.
func recordedWorkflowStart() protocol.ConversationSnapshot {
	snapshot := protocol.DefaultConversationSnapshot()
	snapshot.Node, snapshot.Sequence = protocol.NodeID{1}, 1
	snapshot.Page.Session, snapshot.Page.Revision = protocol.SessionID{2}, 1
	run := protocol.DefaultConversationRun()
	run.Session, run.Worktree, run.Turn = snapshot.Page.Session, protocol.WorktreeID{4}, protocol.TurnID{3}
	run.Sequence, run.Revision = 1, 1
	run.Kind, run.Status = protocol.ConversationRunKindTask, protocol.ConversationStatusRunning
	snapshot.Page.Runs = []protocol.ConversationRun{run}
	return snapshot
}

func recordedWorkflowEntry(snapshot *protocol.ConversationSnapshot, id, author string, parts ...protocol.ConversationPart) {
	entry := protocol.DefaultConversationEntry()
	entry.ID, entry.Turn, entry.Author = id, snapshot.Page.Runs[0].Turn, author
	entry.Sequence, entry.Parts = uint64(len(snapshot.Page.Entries)+1), parts
	snapshot.Page.Entries = append(snapshot.Page.Entries, entry)
	snapshot.Sequence, snapshot.Page.Revision = entry.Sequence, snapshot.Page.Revision+1
}

func recordedWorkflowCopy(t *testing.T, snapshot protocol.ConversationSnapshot) protocol.ConversationSnapshot {
	t.Helper()
	copied, err := copyLiveValue[protocol.ConversationSnapshot](snapshot)
	if err != nil {
		t.Fatal(err)
	}
	return copied
}

func readWorkflowRecording(t *testing.T, snapshots ...protocol.ConversationSnapshot) core.RecordingSession {
	t.Helper()
	last := snapshots[len(snapshots)-1]
	history := protocol.DefaultConversationHistory()
	history.Sequence, history.Page = last.Sequence, last.Page
	recording := core.RecordingSession{Version: 1, Node: last.Node, Session: last.Page.Session,
		Captured: time.Unix(1, 0).UTC(), History: history, Tools: []core.RecordingTool{}}
	for index, snapshot := range snapshots {
		recording.Frames = append(recording.Frames, core.RecordingFrame{At: time.Duration(index) * 20 * time.Millisecond, Snapshot: snapshot})
	}
	encoded, err := json.Marshal(recording)
	if err != nil {
		t.Fatal(err)
	}
	directory := filepath.Join(t.TempDir(), "acceptance")
	path := core.RecordingPath(directory, recording.Session)
	if err := profile.PrivateDirectory(directory); err != nil {
		t.Fatal(err)
	}
	if err := profile.PrivateDirectory(filepath.Dir(path)); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, encoded, 0o600); err != nil {
		t.Fatal(err)
	}
	restored, err := core.ReadRecording(directory, recording.Session)
	if err != nil {
		t.Fatal(err)
	}
	result := replayRecording(t.Context(), restored, core.CaseResult{ID: "workflow", Session: restored.Session.String(), Status: "failed"}, shell.Assembly{})
	if result.Status != "passed" {
		t.Fatalf("recorded workflow replay: %+v", result)
	}
	return restored
}

func workflowWindow(t *testing.T, session protocol.SessionID, language string) (*window, *viewmodel.Conversation) {
	t.Helper()
	cv := &viewmodel.Conversation{ID: session.String(), Title: "Recorded workflow"}
	w := &window{ctx: t.Context(), result: &core.CaseResult{Session: session.String()}}
	w.app = shell.NewLive(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{cv}, Workspace: &viewmodel.Workspace{}})
	w.app.Effects, w.app.NavOpen, w.app.Sidebar.Selected = false, false, cv.ID
	w.tester = ui.NewTesterWith(w.app.View, 1280, 1000, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
	w.tester.SetLocale(language)
	return w, cv
}

func workflowFrame(t *testing.T, w *window, cv *viewmodel.Conversation, frame core.RecordingFrame, language string) {
	t.Helper()
	cv.Turns, cv.Session, cv.Waiting, cv.Queue = live.Conversation(client.ConversationState{Connected: true, Snapshot: frame.Snapshot}, language)
	if err := verifyRecordedText(frame.Snapshot, cv.Turns); err != nil {
		t.Fatal(err)
	}
	w.app.Messages.List(cv.ID).ScrollToEnd()
	w.tester.Frame()
}

func openWorkflowWork(t *testing.T, w *window, language, key string) {
	t.Helper()
	if button, ok := w.tester.AccessByRole(ui.RoleButton, i18n.In(language, key)); ok && !button.Expanded {
		w.tester.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
		w.tester.Frame()
	}
}

func TestRecordedWorkflowGuidanceKeepsItsHistoryPosition(t *testing.T) {
	pending := recordedWorkflowStart()
	owner := pending.Page.Runs[0].Turn
	worktree := pending.Page.Runs[0].Worktree
	initial := protocol.Attachment{ID: protocol.AttachmentID{5}, Spec: protocol.AttachmentSpec{Worktree: worktree, Name: "initial.txt", MediaType: "text/plain"}}
	guideFile := protocol.Attachment{ID: protocol.AttachmentID{6}, Spec: protocol.AttachmentSpec{Worktree: worktree, Name: "guidance.txt", MediaType: "text/plain"}}
	guideImage := protocol.Attachment{ID: protocol.AttachmentID{7}, Spec: protocol.AttachmentSpec{Worktree: worktree, Name: "guidance.png", MediaType: "image/png"}}
	recordedWorkflowEntry(&pending, "initial-request", "user", protocol.ConversationPart{Kind: "text", Data: "Inspect the fixture"}, protocol.ConversationPart{Kind: "attachment", Data: initial})
	callID := "read-before-guidance"
	call := protocol.DefaultConversationPartToolCallPayload()
	call.ID, call.Name, call.Presentation, call.Summary = &callID, "read_file", protocol.ToolPresentationSummary, &protocol.ToolDetailSummary{}
	recordedWorkflowEntry(&pending, "before-guidance", "assistant", protocol.ConversationPart{Kind: "tool_call", Data: call},
		protocol.ConversationPart{Kind: "resource", Data: json.RawMessage(`{"type":"tool_started","id":"read-before-guidance","name":"read_file"}`)},
		protocol.ConversationPart{Kind: "text", Data: "Earlier fixture answer."})
	inserted := recordedWorkflowCopy(t, pending)
	guide := inserted.Page.Runs[0]
	guide.Turn, guide.Sequence, guide.Joined, guide.Guidance, guide.Status = protocol.TurnID{8}, 9, &owner, &owner, protocol.ConversationStatusCompleted
	inserted.Page.Runs = append(inserted.Page.Runs, guide)
	recordedWorkflowEntry(&inserted, "inserted-guidance", "user", protocol.ConversationPart{Kind: "text", Data: "Also preserve this constraint"},
		protocol.ConversationPart{Kind: "attachment", Data: guideFile}, protocol.ConversationPart{Kind: "image", Data: protocol.ConversationImage{Attachment: guideImage}})
	inserted.Page.Entries[len(inserted.Page.Entries)-1].Turn = guide.Turn
	draft := protocol.DefaultConversationDraft()
	draft.ID, draft.Turn, draft.Author, draft.Parts = "later-draft", owner, "assistant", []protocol.ConversationPart{{Kind: "text", Data: "Later fixture answer."}}
	inserted.Drafts = []protocol.ConversationDraft{draft}
	finished := recordedWorkflowCopy(t, inserted)
	finished.Drafts = nil
	result := protocol.DefaultConversationPartToolResultPayload()
	result.ID, result.Name, result.Summary = &callID, call.Name, &protocol.ToolDetailSummary{}
	recordedWorkflowEntry(&finished, "read-completed", "tool", protocol.ConversationPart{Kind: "tool_result", Data: result})
	recordedWorkflowEntry(&finished, "later-answer", "assistant", draft.Parts...)
	finished.Page.Runs[0].Status = protocol.ConversationStatusCompleted
	recording := readWorkflowRecording(t, pending, inserted, finished)
	w, cv := workflowWindow(t, recording.Session, "en")
	w.tester.SetSize(1280, 1600)
	for index, frame := range recording.Frames {
		workflowFrame(t, w, cv, frame, "en")
		if len(cv.Turns) != 1 || cv.Turns[0].ID != owner.String() {
			t.Fatalf("frame %d split guidance into a separate execution: %+v", index, cv.Turns)
		}
		turn := cv.Turns[0]
		var calls []conversation.Call
		for _, block := range turn.Reply.Blocks {
			calls = append(calls, block.Calls...)
		}
		if len(calls) != 1 || calls[0].ID != "before-guidance/0" || calls[0].Running != (index < 2) {
			t.Fatalf("frame %d duplicated or detached the earlier tool: %+v", index, calls)
		}
		if index == 0 {
			continue
		}
		if len(turn.Segments) != 2 || turn.Segments[0].From != 0 || turn.Segments[1].From != 2 ||
			len(turn.Segments[0].Inputs) != 1 || len(turn.Segments[1].Inputs) != 1 {
			t.Fatalf("frame %d lost the guidance boundary: %+v", index, turn.Segments)
		}
		first, later := turn.Segments[0].Inputs[0], turn.Segments[1].Inputs[0]
		if first.ID != owner.String() || !slices.Equal(first.Files, []int{0}) || len(first.Images) != 0 ||
			later.ID != guide.Turn.String() || !slices.Equal(later.Files, []int{1}) || !slices.Equal(later.Images, []int{0}) ||
			len(turn.Files) != 2 || turn.Files[0].ID != initial.ID.String() || turn.Files[1].ID != guideFile.ID.String() || len(turn.Images) != 1 || turn.Images[0].ID != guideImage.ID.String() {
			t.Fatalf("frame %d moved an attachment to another input: %+v", index, turn)
		}
		var bottom float32
		for _, text := range []string{"Inspect the fixture", "Earlier fixture answer.", "Also preserve this constraint", "Later fixture answer."} {
			bounds, found := w.tester.Find(text)
			if !found || bounds.Y < bottom {
				t.Fatalf("frame %d misplaced %q in the rendered transcript: %q", index, text, w.tester.Texts())
			}
			bottom = bounds.Y + bounds.H
		}
		if w.tester.HasText("Inspect the fixture\n\nAlso preserve this constraint") {
			t.Fatal("the rendered transcript merged guidance into the initial bubble")
		}
	}
}

func TestRecordedWorkflowPlanUpdatesComponentAndCapsule(t *testing.T) {
	pending := recordedWorkflowStart()
	name, callID := core.PackageToolAlias("fixture-checklist", "replace_steps"), "plan-first"
	call := protocol.DefaultConversationPartToolCallPayload()
	call.ID, call.Name, call.Presentation = &callID, name, protocol.ToolPresentationProgress
	call.Grouping, call.Summary = protocol.ToolGroupingStandalone, &protocol.ToolDetailSummary{}
	recordedWorkflowEntry(&pending, "plan-call", "assistant", protocol.ConversationPart{Kind: "tool_call", Data: call})
	active := recordedWorkflowCopy(t, pending)
	progress := protocol.ConversationProgress{Steps: []protocol.ConversationProgressStep{
		{Description: "Inspect the fixture", State: protocol.ConversationProgressStateCompleted},
		{Description: "Apply the fixture fix", State: protocol.ConversationProgressStateInProgress},
	}}
	result := protocol.DefaultConversationPartToolResultPayload()
	result.ID, result.Name, result.Summary = &callID, name, &protocol.ToolDetailSummary{Progress: &progress}
	recordedWorkflowEntry(&active, "plan-result", "tool", protocol.ConversationPart{Kind: "tool_result", Data: result})
	finished := recordedWorkflowCopy(t, active)
	nextID := "plan-final"
	call.ID = &nextID
	recordedWorkflowEntry(&finished, "next-plan-call", "assistant", protocol.ConversationPart{Kind: "tool_call", Data: call})
	completed := progress
	completed.Steps = slices.Clone(progress.Steps)
	completed.Steps[1].State = protocol.ConversationProgressStateCompleted
	result.ID, result.Summary = &nextID, &protocol.ToolDetailSummary{Progress: &completed}
	recordedWorkflowEntry(&finished, "next-plan-result", "tool", protocol.ConversationPart{Kind: "tool_result", Data: result})
	finished.Page.Runs[0].Status = protocol.ConversationStatusCompleted
	recording := readWorkflowRecording(t, pending, active, finished)
	w, cv := workflowWindow(t, recording.Session, "en")
	var steps []conversation.Step
	// The fixture adapter supplies projected steps to the production composer.
	// Runtime's RPC context binding is outside this read-only replay.
	w.app.Hooks.Conversation = func(*ui.Context, *viewmodel.Conversation, *composer.State) (*composer.Context, shell.ConversationActions) {
		return &composer.Context{Readonly: true, Docked: true, Activity: composer.Activity{Connected: true, Progress: steps}}, &liveAdmissionActions{send: func(composer.Message) { t.Fatal("replay attempted to send") }}
	}
	for index, frame := range recording.Frames {
		turns, _, _, _ := live.Conversation(client.ConversationState{Connected: true, Snapshot: frame.Snapshot}, "en")
		if len(turns) != 1 || len(turns[0].Reply.Blocks) != 1 {
			t.Fatalf("frame %d changed the plan component count: %+v", index, turns)
		}
		block := turns[0].Reply.Blocks[0]
		steps = block.Steps
		workflowFrame(t, w, cv, frame, "en")
		if index == 0 {
			if block.Kind != conversation.ToolsBlock {
				t.Fatal("a pending plan claimed validated progress")
			}
			continue
		}
		if block.Kind != conversation.ProgressBlock || len(block.Calls) != 0 || len(steps) != 2 {
			t.Fatalf("frame %d did not replace the generic tool with progress: %+v", index, block)
		}
		openWorkflowWork(t, w, "en", "turn_completed")
		if !w.tester.HasText(progress.Steps[0].Description) || !w.tester.HasText(progress.Steps[1].Description) {
			t.Fatal("the transcript did not show its recorded steps directly", w.tester.Texts())
		}
		var capsules []ui.AccessInfo
		for _, control := range w.tester.AccessAll() {
			if control.Role != ui.RoleButton || control.Offscreen || !strings.HasPrefix(control.Name, i18n.In("en", "task_progress")) {
				continue
			}
			if control.HasPopup == ui.PopupDialog {
				capsules = append(capsules, control)
			} else {
				t.Fatalf("frame %d made the transcript plan row a disclosure", index)
			}
		}
		if index == 1 {
			if len(capsules) != 1 || !w.tester.HasText(i18n.In("en", "composer_todo_count", "completed", 1, "total", 2)) {
				t.Fatal("the active recorded plan has no unique progress capsule", w.tester.Texts())
			}
			button := capsules[0]
			w.tester.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
			w.tester.Advance(time.Second)
			w.tester.Frame()
			if capsule, ok := w.tester.AccessByRole(ui.RoleButton, button.Name); !ok || !capsule.Expanded || !w.tester.HasText(progress.Steps[0].Description) || !w.tester.HasText(progress.Steps[1].Description) {
				t.Fatal("the capsule did not open its recorded steps", w.tester.Texts())
			}
		} else if len(capsules) != 0 || steps[1].State != conversation.StepCompleted {
			t.Fatal("completed recorded progress kept a live capsule")
		}
	}
}

func TestRecordedWorkflowRetryUpdatesOneRequestRow(t *testing.T) {
	var snapshots []protocol.ConversationSnapshot
	snapshot := recordedWorkflowStart()
	for attempt := 1; attempt <= 3; attempt++ {
		raw, err := json.Marshal(map[string]any{"type": "model_retry", "id": "recorded-model-request", "attempt": attempt, "limit": 9})
		if err != nil {
			t.Fatal(err)
		}
		recordedWorkflowEntry(&snapshot, "retry-"+string(rune('0'+attempt)), "assistant", protocol.ConversationPart{Kind: "resource", Data: json.RawMessage(raw)})
		snapshots = append(snapshots, recordedWorkflowCopy(t, snapshot))
	}
	finished := recordedWorkflowCopy(t, snapshot)
	finished.Page.Runs[0].Status = protocol.ConversationStatusCompleted
	recordedWorkflowEntry(&finished, "retry-answer", "assistant", protocol.ConversationPart{Kind: "text", Data: "The recorded request recovered."})
	snapshots = append(snapshots, finished)
	recording := readWorkflowRecording(t, snapshots...)
	w, cv := workflowWindow(t, recording.Session, "zh-CN")
	for index, frame := range recording.Frames {
		workflowFrame(t, w, cv, frame, "zh-CN")
		openWorkflowWork(t, w, "zh-CN", "turn_completed")
		var retries []*conversation.Retry
		for _, block := range cv.Turns[0].Reply.Blocks {
			if block.Retry != nil {
				retries = append(retries, block.Retry)
			}
		}
		attempt := min(index+1, 3)
		if len(retries) != 1 || retries[0].Request != "recorded-model-request" || retries[0].Attempt != attempt {
			t.Fatalf("frame %d duplicated or lost the request's latest retry: %+v", index, retries)
		}
		for number := 1; number <= 3; number++ {
			label := i18n.In("zh-CN", "chat_model_reconnecting", "attempt", number, "limit", 9)
			count := 0
			for _, text := range w.tester.Texts() {
				if text == label {
					count++
				}
			}
			if number == attempt && count != 1 || number != attempt && count != 0 {
				t.Fatalf("frame %d has %d copies of %q: %q", index, count, label, w.tester.Texts())
			}
		}
		if w.tester.HasText(i18n.In("zh-CN", "chat_model_retried", "attempt", attempt, "limit", 9)) {
			t.Fatal("finishing the request renamed the recorded retry")
		}
	}
}

func TestRecordedWorkflowReadToolKeepsLocalizedSummaryOnly(t *testing.T) {
	snapshot := recordedWorkflowStart()
	snapshot.Page.Runs[0].Status = protocol.ConversationStatusCompleted
	callID, title := "read-call", "Read file"
	name := core.PackageToolAlias("fixture-files", "read_file")
	resolved := protocol.DefaultToolDetailResolved()
	resolved.Label = &protocol.ToolResultLocalText{Label: title, Locales: map[string]string{"zh-CN": "读取文件"}}
	resolved.Input.Summary = &protocol.ToolDetailText{Text: "docs/example.txt", Code: true}
	resolved.Output = &protocol.ToolDetailOutput{Preview: "RECORDED-READ-BODY-MUST-STAY-HIDDEN"}
	summary := protocol.DefaultToolDetailSummary()
	summary.Title, summary.Resolved, summary.HasDetails = &title, &resolved, false
	call := protocol.DefaultConversationPartToolCallPayload()
	call.ID, call.Name, call.Summary = &callID, name, &summary
	result := protocol.DefaultConversationPartToolResultPayload()
	result.ID, result.Name, result.Summary = &callID, name, &summary
	recordedWorkflowEntry(&snapshot, "read-request", "assistant", protocol.ConversationPart{Kind: "tool_call", Data: call})
	recordedWorkflowEntry(&snapshot, "read-result", "tool", protocol.ConversationPart{Kind: "tool_result", Data: result})
	recording := readWorkflowRecording(t, snapshot)
	w, cv := workflowWindow(t, recording.Session, "zh-CN")
	workflowFrame(t, w, cv, recording.Frames[0], "zh-CN")
	openWorkflowWork(t, w, "zh-CN", "turn_completed")
	blocks := cv.Turns[0].Reply.Blocks
	if len(blocks) != 1 || len(blocks[0].Calls) != 1 || blocks[0].Calls[0].Output != nil || blocks[0].Calls[0].Load != nil {
		t.Fatal("the recorded summary-only read acquired a detail disclosure", blocks)
	}
	if !w.tester.HasText("读取文件") || !w.tester.HasText("docs/example.txt") || w.tester.HasText(title) {
		t.Fatal("the recorded tool lost its localized title or input", w.tester.Texts())
	}
	for _, control := range w.tester.AccessAll() {
		if control.Role == ui.RoleButton && strings.HasPrefix(control.Name, "读取文件") {
			t.Fatal("the summary-only read became an expandable tool row")
		}
	}
	area, found := w.tester.Find("读取文件")
	if !found {
		t.Fatal("the localized read row is missing")
	}
	w.tester.ClickAt(area.X+area.W/2, area.Y+area.H/2)
	w.tester.Frame()
	if w.tester.HasText(resolved.Output.Preview) {
		t.Fatal("clicking a summary-only tool exposed its omitted result")
	}
}

func TestRecordedWorkflowCompactionFailureStopsActivity(t *testing.T) {
	const cause = "HTTP 400: This conversation can no longer be routed to its original account. Start a new Claude Code session."
	running := recordedWorkflowStart()
	running.Page.Runs[0].Kind = protocol.ConversationRunKindCompaction
	failed := recordedWorkflowCopy(t, running)
	failed.Sequence, failed.Page.Revision = 2, 2
	failed.Page.Runs[0].Status = protocol.ConversationStatusFailed
	failed.Page.Runs[0].Error = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: cause,
		Details: &protocol.FaultDetails{Kind: "model_request", Data: protocol.FaultDetailsModelRequestPayload{Reason: protocol.ModelFailureRequest}}}
	recording := readWorkflowRecording(t, running, failed)
	w, cv := workflowWindow(t, recording.Session, "en")
	for index, frame := range recording.Frames {
		workflowFrame(t, w, cv, frame, "en")
		if index == 0 {
			if cv.Turns[0].Compaction.State != conversation.CompactionRunning || !w.tester.HasText(i18n.In("en", "chat_compacting")) {
				t.Fatal("the initial recorded compaction did not show its running state")
			}
			continue
		}
		if cv.Running() || cv.Session.Loading || cv.Session.Connection != conversation.Connected || cv.Turns[0].Compaction.State != conversation.CompactionFailed || cv.Turns[0].Compaction.Cause != cause || cv.Session.Diagnostic != nil {
			t.Fatalf("failed compaction retained activity or disconnected its Host: %+v", cv)
		}
		if blocks := cv.Turns[0].Reply.Blocks; len(blocks) != 1 || blocks[0].Kind != conversation.ErrorBlock || blocks[0].Text != cause || len(blocks[0].Fields) != 1 || blocks[0].Fields[0] != [2]string{"code", string(protocol.ErrorCodeUnavailable)} {
			t.Fatalf("failed compaction lost its stored reply detail: %+v", blocks)
		}
		if !w.tester.HasText(i18n.In("en", "chat_compact_failed")) || w.tester.HasText(i18n.In("en", "chat_compacting")) {
			t.Fatal("the terminal replay retained the compaction loading label", w.tester.Texts())
		}
		if err := w.button(i18n.In("en", "chat_compact_failed")); err != nil {
			t.Fatal(err)
		}
		if !w.tester.HasText(failed.Page.Runs[0].Error.Message) {
			t.Fatal("the recorded failure did not expose its original cause")
		}
	}
}
