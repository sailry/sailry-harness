package live

import (
	"encoding/json"
	"reflect"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestForegroundCommandRetainedPreviewBinding(t *testing.T) {
	session, turn, id := protocol.SessionID{1}, protocol.TurnID{2}, protocol.RequestID{3}
	tail := strings.Repeat("retained\n", 101) + "finished"
	result, err := json.Marshal(map[string]any{
		"kind": "command_result",
		"data": map[string]any{
			"stdout": protocol.ProcessCapture{Text: "captured head", Truncated: true},
			"stderr": protocol.ProcessCapture{Text: "error stream"},
		},
		"sailry_command": map[string]any{
			"id":     id,
			"stdout": retainedCommandTail{Text: tail, Offset: 70000, Head: 71000, Start: 71000, End: 71000, Total: 71000},
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	detail := protocol.ToolDetail{Result: new(json.RawMessage(result)), Content: &protocol.ToolContent{Version: 1, Blocks: []protocol.ToolContentBlock{{
		Kind: "text", Data: protocol.ToolContentText{Path: "/data/stdout/text", Notices: []protocol.PluginDesktopNavigation{
			{Label: "Output was truncated"}, {Label: "Some output could not be decoded"},
		}},
	}}}}
	ref, ok := toolCommand(detail, session, turn)
	if !ok || ref.session != session || ref.turn != turn || ref.id != id || ref.preview == nil || *ref.preview != tail+"\nerror stream" {
		t.Fatalf("foreground capture lost its retained command or final lines: %+v", ref)
	}
	call := conversation.Call{Name: "run_command"}
	ProjectToolDetail(&call, &protocol.ToolDetailSummary{HasDetails: true}, detail, "en")
	if call.Output == nil || !call.Output.Command || !reflect.DeepEqual(call.Output.Notices, []string{"Some output could not be decoded"}) {
		t.Fatalf("command disclosure hid a real diagnostic or kept a preview notice: %+v", call.Output)
	}
}

func TestHistoricalCommandDisclosure(t *testing.T) {
	const title, stdout = "Original background command", "Retained output of the original command"
	const noRetention = "command retention ended"
	prefix := strings.Repeat("before the preview\n", 40)
	tail := strings.Repeat("last lines\n", 99) + stdout
	session, turn, command := protocol.SessionID{1}, protocol.TurnID{1}, protocol.RequestID{1}
	snapshot := protocol.DefaultConversationSnapshot()
	snapshot.Node, snapshot.Sequence = protocol.NodeID{1}, 6
	page := &snapshot.Page
	page.Session, page.Revision = session, 1
	page.Runs = []protocol.ConversationRun{{Session: session, Turn: turn, Worktree: protocol.WorktreeID{1}, Kind: "task", Status: "completed", Sequence: 1, Revision: 1}}
	for i := 2; i <= 21; i++ {
		id := protocol.RequestID{byte(i)}
		page.Background = append(page.Background, protocol.ConversationBackgroundTask{ID: "command:" + id.String(), Session: session, Turn: turn,
			Kind:  protocol.ConversationBackgroundKind{Kind: "command", Data: protocol.ConversationBackgroundKindCommandPayload{ID: id}},
			State: "consumed", Revision: 1, Outcome: new(protocol.ConversationBackgroundOutcomeCompleted)})
	}
	call := transcriptTool("launch", "run_command")
	result := transcriptResult("launch", "run_command")
	summary := result.Data.(protocol.ConversationPartToolResultPayload).Summary
	summary.Task, summary.Title, summary.Revision = new("command:"+command.String()), new(title), strings.Repeat("a", 64)
	page.Entries = []protocol.ConversationEntry{
		{ID: "call", Turn: turn, Sequence: 1, Author: "assistant", Parts: []protocol.ConversationPart{call}},
		{ID: "result", Turn: turn, Sequence: 2, Author: "tool", Parts: []protocol.ConversationPart{result}},
		{ID: "answer", Turn: turn, Sequence: 3, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Original answer"}}},
	}
	ref := protocol.ToolDetailReference{Session: session, Turn: turn, Entry: "result", Call: new("launch"), Revision: summary.Revision}
	raw, err := json.Marshal(map[string]any{"kind": "command_started", "data": map[string]any{"id": command, "session": session, "turn": turn}})
	if err != nil {
		t.Fatal(err)
	}
	original := protocol.DefaultToolDetail()
	original.Result = new(json.RawMessage(raw))
	body, err := json.Marshal(original)
	if err != nil {
		t.Fatal(err)
	}
	r, cv, transport, _ := watchCallbackTranscript(t, snapshot)
	r.initCommands()
	t.Cleanup(func() { r.closeCommands(); r.workers.Wait() })
	retained := true
	transport.mu.Lock()
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		switch request.Command.Kind {
		case "read_tool_detail":
			query, _ := detail[protocol.CommandReadToolDetailPayload](request.Command.Data)
			if got := (protocol.ToolDetailReference{Session: query.Session, Turn: query.Turn, Entry: query.Entry, Index: query.Index, Call: query.Call, Revision: query.ExpectedRevision}); !reflect.DeepEqual(got, ref) {
				t.Errorf("opened detail came from another record: %+v", got)
			}
			return link.Response{Output: protocol.Output{Kind: "tool_detail", Data: protocol.ToolDetailPage{Reference: ref, TotalBytes: uint64(len(body)), Text: string(body)}}}, true
		case "read_command":
			if !retained {
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: noRetention}}, true
			}
			query, _ := detail[protocol.CommandReadCommandPayload](request.Command.Data)
			if query.Session != session || query.ID != command || query.WaitMs != 0 {
				t.Errorf("opened detail read another process: %+v", query)
			}
			authentic := protocol.DefaultProcessSnapshot()
			authentic.Info.ID, authentic.Info.Session, authentic.Info.Turn = command, session, turn
			authentic.Info.Status = protocol.ProcessStatus{Kind: "finished", Data: protocol.ProcessOutcome{Kind: "exited", Data: int32(7)}}
			authentic.Stdout.Text, authentic.Stdout.Truncated = "bounded capture", true
			return link.Response{Output: protocol.Output{Kind: "command_output", Data: authentic}}, true
		case "read_command_page":
			if !retained {
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: noRetention}}, true
			}
			query, _ := detail[protocol.CommandReadCommandPagePayload](request.Command.Data)
			if query.Session != session || query.ID != command || query.WaitMs != 0 {
				t.Errorf("opened detail paged another process: %+v", query)
			}
			page := protocol.DefaultProcessOutputOutputPage()
			page.Info.ID, page.Info.Session, page.Info.Turn = command, session, turn
			page.Stream, page.Offset = query.Stream, query.Offset
			if query.Stream == protocol.ProcessOutputOutputStreamStdout {
				page.TotalBytes = uint64(len(prefix) + len(tail))
				if query.Offset == 0 {
					page.Text, page.NextOffset = prefix, new(uint64(len(prefix)))
				} else if query.Offset == uint64(len(prefix)) {
					page.Text = tail
				} else {
					t.Errorf("unexpected output cursor: %+v", query)
				}
			}
			return link.Response{Output: protocol.Output{Kind: "command_output_page", Data: page}}, true
		}
		return link.Response{}, false
	}
	transport.mu.Unlock()
	view := conversation.New()
	var completions []func(*ui.Context)
	tester := ui.NewTesterWith(func(c *ui.Context) {
		defer r.pruneCommandDetails()
		defer view.FinishFrame()
		theme.Apply(c, "", theme.Light)
		for _, complete := range completions {
			complete(c)
		}
		completions = nil
		ui.Column(c).Fill().Children(func() { conversation.View(c, view, cv.ID, cv.Turns, &cv.Session, referenceViewActions{}) })
	}, 900, 1240, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	tester.SetLocale("en")
	render := func() {
		r.workers.Wait()
		r.mu.Lock()
		completions = r.queue
		r.queue = nil
		r.mu.Unlock()
		cacheTestFrame(t, r, cv, session)
		tester.Frame()
	}
	requests := func() []string {
		transport.mu.Lock()
		defer transport.mu.Unlock()
		var kinds []string
		for _, request := range transport.requests {
			if kind := request.Command.Kind; kind == "read_tool_detail" || kind == "read_command" || kind == "read_command_page" {
				kinds = append(kinds, kind)
			}
		}
		return kinds
	}
	click := func(label string) {
		t.Helper()
		if err := tester.Click(label); err != nil {
			t.Fatalf("click %q: %v; %q", label, err, tester.Texts())
		}
	}
	render()
	click(i18n.In("en", "turn_completed"))
	render()
	if got := requests(); len(got) != 0 || tester.HasText(stdout) {
		t.Fatalf("showing the old work fetched detail before its row opened: %v", got)
	}
	click(title)
	render()
	render()
	wantRequests := []string{"read_tool_detail", "read_command", "read_command_page", "read_command_page", "read_command_page"}
	lines := strings.Split(tail, "\n")
	if got := requests(); !reflect.DeepEqual(got, wantRequests) || !tester.HasText(strings.Join(lines[len(lines)-12:], "\n")) || tester.HasText(tail) || tester.HasText(prefix) || tester.HasText("Partial results") || tester.HasText(i18n.In("en", "background_exit", "code", 7)) {
		t.Fatalf("opening did not render the real old result's preview once: %v; %q", got, tester.Texts())
	}
	click(i18n.In("en", "output_show_all", "count", len(lines)))
	render()
	if got := requests(); !reflect.DeepEqual(got, wantRequests) || !tester.HasText(tail) || tester.HasText(prefix) {
		t.Fatalf("Show all did not reveal the retained result without another read: %v; %q", got, tester.Texts())
	}
	click(i18n.In("en", "output_show_less"))
	render()
	copyLabel := i18n.In("en", "tool_copy")
	copyButton, ok := tester.AccessByRole(ui.RoleButton, copyLabel)
	if !ok {
		t.Fatal("retained command has no Copy action")
	}
	tester.Move(copyButton.Bounds.X+copyButton.Bounds.W/2, copyButton.Bounds.Y+copyButton.Bounds.H/2)
	tester.Frame()
	click(copyLabel)
	render()
	wantRequests = append(wantRequests, "read_command_page", "read_command_page", "read_command_page")
	if got := requests(); !reflect.DeepEqual(got, wantRequests) || tester.Clipboard() != prefix+tail {
		t.Fatalf("Copy did not recover all retained pages: %v, %d bytes", got, len(tester.Clipboard()))
	}
	transport.mu.Lock()
	retained = false
	transport.mu.Unlock()
	click(copyLabel)
	render()
	if tester.Clipboard() != prefix+tail || !tester.HasText(strings.Join(lines[len(lines)-12:], "\n")) {
		t.Fatal("eviction replaced the clipboard or removed the existing preview")
	}
	blocks := conversation.Compact(cv.Turns[0].Reply.Blocks)
	if len(blocks) != 2 || len(blocks[0].Calls) != 1 || blocks[0].Calls[0].ID != "call/0" || blocks[1].Text != "Original answer" {
		t.Fatalf("opening moved or added transcript rows: %+v", blocks)
	}
	click(title)
	render()
	if len(r.commands.details) != 0 || tester.HasText(stdout) {
		t.Fatalf("closing retained the old command's view or output: %d reads, output visible %t", len(r.commands.details), tester.HasText(stdout))
	}
	click(title)
	render()
	render()
	// The body is the command's own output; a failed read writes nothing.
	for _, text := range tester.Texts() {
		if strings.Contains(text, noRetention) {
			t.Fatalf("reopening showed the read error as the command's output: %q", tester.Texts())
		}
	}
}
