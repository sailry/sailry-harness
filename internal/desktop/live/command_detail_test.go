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

func TestHistoricalCommandDisclosure(t *testing.T) {
	const title, stdout = "Original background command", "Retained output of the original command"
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
			query, _ := detail[protocol.CommandReadCommandPayload](request.Command.Data)
			if query.Session != session || query.ID != command || query.WaitMs != 0 {
				t.Errorf("opened detail read another process: %+v", query)
			}
			authentic := protocol.DefaultProcessSnapshot()
			authentic.Info.ID, authentic.Info.Session, authentic.Info.Turn = command, session, turn
			authentic.Info.Status = protocol.ProcessStatus{Kind: "finished", Data: protocol.ProcessOutcome{Kind: "exited", Data: int32(7)}}
			authentic.Stdout.Text = stdout
			return link.Response{Output: protocol.Output{Kind: "command_output", Data: authentic}}, true
		}
		return link.Response{}, false
	}
	transport.mu.Unlock()
	view := conversation.New()
	tester := ui.NewTesterWith(func(c *ui.Context) {
		defer r.pruneCommandDetails()
		defer view.FinishFrame()
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { conversation.View(c, view, cv.ID, cv.Turns, &cv.Session, referenceViewActions{}) })
	}, 900, 1240, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	tester.SetLocale("en")
	render := func() {
		r.workers.Wait()
		r.mu.Lock()
		queue := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, complete := range queue {
			complete(nil)
		}
		state := cacheTestFrame(t, r, cv, session)
		r.enrichDetails(cv, state, "en")
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
	if got := requests(); !reflect.DeepEqual(got, []string{"read_tool_detail", "read_command"}) || !tester.HasText(stdout) || !tester.HasText(i18n.In("en", "background_exit", "code", 7)) {
		t.Fatalf("opening did not render the real old result once: %v; %q", got, tester.Texts())
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
}
