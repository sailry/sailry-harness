package live

import (
	"encoding/json"
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func transcriptActivityTester(turns *[]conversation.Turn) *ui.Tester {
	view := conversation.New()
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { conversation.View(c, view, "parent", *turns, &conversation.Session{}, referenceViewActions{}) })
	}, 900, 1240)
	tester.SetLocale("zh-CN")
	tester.SetPreferences(ui.Preferences{ReduceMotion: true, ReduceTransparency: true})
	liveTesterScale(tester, "SAILRY_SNAPSHOTS")
	return tester
}

func transcriptActivityShot(t *testing.T, tester *ui.Tester, name string) {
	t.Helper()
	dir := os.Getenv("SAILRY_SNAPSHOTS")
	if dir == "" {
		return
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	f, err := os.Create(filepath.Join(dir, name+".png"))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if err := png.Encode(f, tester.Image()); err != nil {
		t.Fatal(err)
	}
}

func transcriptTool(id, name string) protocol.ConversationPart {
	call := protocol.DefaultConversationPartToolCallPayload()
	summary := protocol.DefaultToolDetailSummary()
	summary.Revision = id + "-call"
	call.ID, call.Name, call.Summary = &id, name, &summary
	return protocol.ConversationPart{Kind: "tool_call", Data: call}
}

func transcriptResult(id, name string) protocol.ConversationPart {
	result := protocol.DefaultConversationPartToolResultPayload()
	summary := protocol.DefaultToolDetailSummary()
	summary.Revision = id + "-result"
	result.ID, result.Name, result.Summary = &id, name, &summary
	return protocol.ConversationPart{Kind: "tool_result", Data: result}
}

func TestTranscriptToolBatchAwaitsModel(t *testing.T) {
	session, turn := protocol.SessionID{1}, protocol.TurnID{1}
	state := client.ConversationState{Connected: true, Snapshot: protocol.DefaultConversationSnapshot()}
	page := &state.Snapshot.Page
	page.Session, page.Revision = session, 1
	page.Runs = []protocol.ConversationRun{{Session: session, Turn: turn, Sequence: 1, Kind: "task", Status: "running"}}
	entry := func(id, author string, parts ...protocol.ConversationPart) {
		page.Entries = append(page.Entries, protocol.ConversationEntry{ID: id, Turn: turn, Author: author, Sequence: uint64(len(page.Entries) + 1), Parts: parts})
	}
	check := func(stage string, phase conversation.Phase, running, awaiting []string) []conversation.Block {
		t.Helper()
		turns, _, _, _, _ := projectTranscript(state, "en")
		if len(turns) != 1 || turns[0].Reply.Phase != phase {
			t.Fatalf("%s: want phase %v, got %+v", stage, phase, turns)
		}
		blocks := turns[0].Reply.Blocks
		var gotRunning, gotAwaiting []string
		for _, block := range blocks {
			for _, call := range block.Calls {
				if call.Running {
					gotRunning = append(gotRunning, call.ID)
				}
				if call.AwaitingModel {
					gotAwaiting = append(gotAwaiting, call.ID)
				}
			}
		}
		if !slices.Equal(gotRunning, running) || !slices.Equal(gotAwaiting, awaiting) {
			t.Fatalf("%s: running=%v, awaiting=%v; want running=%v, awaiting=%v", stage, gotRunning, gotAwaiting, running, awaiting)
		}
		return blocks
	}
	entry("old-call", "assistant", transcriptTool("old", "read_fixture"))
	entry("old-result", "tool", transcriptResult("old", "read_fixture"))
	check("old batch", conversation.AwaitingResponse, nil, []string{"old-call/0"})

	entry("calls", "assistant", transcriptTool("read", "read_fixture"), transcriptTool("search", "search_fixture"), transcriptTool("plan", "update_plan"))
	check("new tool calls without text", conversation.AwaitingResponse, nil, nil)
	for _, tool := range []struct{ id, name string }{{"read", "read_fixture"}, {"search", "search_fixture"}} {
		activity := json.RawMessage(fmt.Sprintf(`{"type":"tool_started","id":%q,"name":%q}`, tool.id, tool.name))
		state.Snapshot.Drafts = append(state.Snapshot.Drafts, protocol.ConversationDraft{ID: "start-" + tool.id, Turn: turn, Author: "sailry", Parts: []protocol.ConversationPart{{Kind: "resource", Data: activity}}})
	}
	batch := []string{"calls/0", "calls/1"}
	check("parallel workers", conversation.ExecutingTool, batch, nil)
	state.Snapshot.Drafts = state.Snapshot.Drafts[1:]
	check("first worker exited", conversation.ExecutingTool, batch[1:], nil)
	state.Snapshot.Drafts = nil
	check("all workers exited before the batch entry", conversation.AwaitingResponse, nil, nil)

	entry("unmatched", "tool", transcriptResult("read", "read_fixture"))
	page.Entries[len(page.Entries)-1].Branch = "other"
	check("result without a call on its branch", conversation.AwaitingResponse, nil, nil)

	progress := transcriptResult("plan", "update_plan")
	progress.Data.(protocol.ConversationPartToolResultPayload).Summary.Progress = &protocol.ConversationProgress{
		Steps: []protocol.ConversationProgressStep{{Description: "Inspect", State: "completed"}},
	}
	entry("results", "tool", transcriptResult("read", "read_fixture"), transcriptResult("search", "search_fixture"), progress)
	blocks := check("ordinary results in a mixed batch", conversation.AwaitingResponse, nil, batch)
	if len(blocks) != 5 || blocks[3].Kind != conversation.ProgressBlock || len(blocks[3].Calls) != 0 {
		t.Fatalf("validated progress did not replace the pending plan call: %+v", blocks)
	}

	state.Connected = false
	check("disconnected", conversation.AwaitingResponse, nil, nil)
	state.Connected = true
	page.Runs[0].Status = "stopping"
	check("stopping", conversation.AwaitingResponse, nil, nil)
	page.Runs[0].Status = "completed"
	check("completed", conversation.PhaseNone, nil, nil)
	page.Runs[0].Status = "running"

	state.Snapshot.Drafts = []protocol.ConversationDraft{{ID: "answer", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "thinking", Data: ""}}}}
	check("empty assistant draft", conversation.AwaitingResponse, nil, batch)
	thought := protocol.ConversationPart{Kind: "thinking", Data: "private reasoning"}
	state.Snapshot.Drafts[0].Parts[0] = thought
	check("first assistant thinking", conversation.Thinking, nil, nil)
	state.Snapshot.Drafts = nil
	entry("answer", "assistant", thought)
	check("committed assistant thinking", conversation.AwaitingResponse, nil, nil)
}

func TestTranscriptDelegationBatchesReplayInPlace(t *testing.T) {
	parent, turn := protocol.SessionID{1}, protocol.TurnID{1}
	s := protocol.DefaultConversationSnapshot()
	s.Node, s.Page.Session, s.Page.Revision = protocol.NodeID{1}, parent, 1
	run := protocol.ConversationRun{Session: parent, Worktree: protocol.WorktreeID{1}, Turn: turn, Kind: "task", Status: "running", Sequence: 1, Revision: 1}
	s.Page.Runs = []protocol.ConversationRun{run}
	p := client.NewConversationProjection(s.Node, parent, 1)
	apply := func(u protocol.Update) {
		t.Helper()
		if got, err := p.Apply(1, u); err != nil || got != client.Applied {
			t.Fatalf("%s: %v, %v", u.Kind, got, err)
		}
	}
	apply(protocol.Update{Kind: "conversation_snapshot", Data: s})
	seq := s.Sequence
	frame := func(kind string, data any) {
		t.Helper()
		seq++
		apply(protocol.Update{Kind: "conversation_frame", Data: protocol.ConversationFrame{Node: s.Node, Session: parent, Sequence: seq, Change: protocol.ConversationChange{Kind: kind, Data: data}}})
	}
	entry := func(id, author string, parts ...protocol.ConversationPart) {
		t.Helper()
		frame("entry", protocol.ConversationEntry{ID: id, Author: author, Turn: turn, Sequence: seq + 1, Parts: parts})
	}
	batch := func(id string, size, start int) []protocol.ConversationChild {
		t.Helper()
		var calls, receipts []protocol.ConversationPart
		var children []protocol.ConversationChild
		for i := 0; i < size; i++ {
			session, childTurn := protocol.SessionID{byte(i + 2)}, protocol.TurnID{byte(start + i)}
			key := fmt.Sprintf("%s-%d", id, i)
			part := transcriptTool(key, "spawn_agent")
			call := part.Data.(protocol.ConversationPartToolCallPayload)
			call.Summary.Title, call.Summary.Delegation.Background = &key, true
			if id == "reuse" {
				call.Summary.Delegation.Session = &session
			}
			call.Grouping = protocol.ToolGroupingStandalone
			part.Data = call
			calls = append(calls, part)
			receipts = append(receipts, transcriptResult(key, call.Name))
			children = append(children, protocol.ConversationChild{
				Origin: protocol.Delegation{Session: parent, Turn: turn, Entry: id, Index: uint64(i)},
				Run:    protocol.ConversationRun{Session: session, Worktree: run.Worktree, Turn: childTurn, Sequence: uint64(start + i), Revision: 1, Kind: "task", Status: "running"},
			})
		}
		entry(id, "assistant", calls...)
		for _, child := range children {
			frame("child", child)
		}
		slices.Reverse(receipts)
		entry(id+"-receipts", "tool", receipts...)
		return children
	}
	var turns []conversation.Turn
	tester := transcriptActivityTester(&turns)
	show := func() {
		t.Helper()
		turns, _, _, _ = Conversation(client.ConversationState{Connected: true, Snapshot: *p.Snapshot()}, "zh-CN")
		tester.Frame()
		tester.Frame()
	}
	order := func(labels ...string) {
		t.Helper()
		var bottom float32
		for _, label := range labels {
			r, ok := tester.Find(label)
			if !ok || r.Y < bottom {
				t.Fatalf("missing or misplaced %q in %q", label, tester.Texts())
			}
			bottom = r.Y + r.H
		}
	}
	count := func(done, total int) string {
		return i18n.In("zh-CN", "subagent_count", "completed", done, "total", total)
	}
	thinking := i18n.In("zh-CN", "turn_awaiting_response")
	first := batch("first", 10, 2)
	show()
	order(count(0, 10), thinking)
	for i := len(first) - 1; i >= 0; i-- {
		first[i].Run.Status = "completed"
		frame("child", first[i])
	}
	text := protocol.ConversationPart{Kind: "text", Data: "Inspecting the parent files"}
	frame("delta", protocol.ConversationDraft{ID: "message", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{text}})
	show()
	order(count(10, 10), text.Data.(string))
	if tester.HasText(i18n.In("zh-CN", "turn_generating")) || tester.HasText(thinking) {
		t.Fatal("streamed output also showed a phase row")
	}
	entry("message", "assistant", text)
	entry("tools", "assistant", transcriptTool("r", "read_fixture"), transcriptTool("s", "search_fixture"))
	entry("results", "tool", transcriptResult("s", "search_fixture"), transcriptResult("r", "read_fixture"))
	show()
	// The returned batch folds into one group whose header names its tools.
	order(count(10, 10), text.Data.(string), "read_fixture · search_fixture", thinking)
	transcriptActivityShot(t, tester, "transcript-child-batch-idle")
	second := batch("reuse", 2, 12)
	entry("after", "assistant", transcriptTool("last", "check_fixture"))
	entry("last-result", "tool", transcriptResult("last", "check_fixture"))
	second[1].Run.Status = "completed"
	frame("child", second[1])
	show()
	toolGroup := i18n.In("zh-CN", "meter_tool_calls")
	order(count(10, 10), text.Data.(string), toolGroup, count(1, 2), "check_fixture", thinking)
	if _, shown := tester.Find("search_fixture"); shown {
		t.Fatal("settled tools did not collapse into their group")
	}
	if err := tester.Click(toolGroup); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	order(count(10, 10), text.Data.(string), "search_fixture", count(1, 2), "check_fixture", thinking)
	transcriptActivityShot(t, tester, "transcript-child-reuse")
	if err := tester.Click(count(1, 2)); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("reuse-0") || tester.HasText("first-0") {
		t.Fatalf("toggling the reused group also opened its original group: %q", tester.Texts())
	}
	second[0].Run.Status = "completed"
	frame("child", second[0])
	show()
	order(count(10, 10), count(2, 2), "reuse-0", "check_fixture", thinking)
	run.Status = "completed"
	frame("run", run)
	show()
	blocks := conversation.Compact(turns[0].Reply.Blocks)
	var groups [][]conversation.Child
	var tools [][]string
	for _, b := range blocks {
		switch b.Kind {
		case conversation.ChildrenBlock:
			groups = append(groups, b.Children)
		case conversation.ToolsBlock:
			var ids []string
			for _, c := range b.Calls {
				if c.Name != "spawn_agent" {
					ids = append(ids, c.ID)
				}
			}
			if len(ids) > 0 {
				tools = append(tools, ids)
			}
		}
	}
	if len(groups) != 2 || len(groups[0]) != 10 || len(groups[1]) != 2 || !reflect.DeepEqual(tools, [][]string{{"tools/0", "tools/1"}, {"after/0"}}) {
		t.Fatalf("launch or tool boundaries changed: groups=%+v tools=%v", groups, tools)
	}
	for i, children := range [][]protocol.ConversationChild{first, second} {
		for j, child := range children {
			if got := groups[i][j]; got.ID != child.Run.Session.String() || got.Turn != child.Run.Turn.String() || got.Status != conversation.ChildCompleted {
				t.Fatalf("group %d row %d followed the session instead of its execution: %+v", i, j, got)
			}
		}
	}
	encoded, err := json.Marshal(p.Snapshot())
	if err != nil {
		t.Fatal(err)
	}
	var replay protocol.ConversationSnapshot
	if err := protocol.DecodeAs(protocol.PeerData, encoded, &replay); err != nil {
		t.Fatal(err)
	}
	p = client.NewConversationProjection(s.Node, parent, 1)
	apply(protocol.Update{Kind: "conversation_snapshot", Data: replay})
	show()
	if !reflect.DeepEqual(blocks, conversation.Compact(turns[0].Reply.Blocks)) || tester.HasText(thinking) {
		t.Fatal("terminal history replay differs from the live transcript")
	}
}

func TestTranscriptParentIdlePhaseTransitions(t *testing.T) {
	id, turn := protocol.SessionID{1}, protocol.TurnID{1}
	state := client.ConversationState{Connected: true, Snapshot: protocol.DefaultConversationSnapshot()}
	page := &state.Snapshot.Page
	state.Snapshot.Node = protocol.NodeID{1}
	page.Session, page.Revision = id, 1
	page.Runs = []protocol.ConversationRun{{Turn: turn, Session: id, Worktree: protocol.WorktreeID{1}, Sequence: 1, Revision: 1, Kind: "task", Status: "running"}}
	var turns []conversation.Turn
	tester := transcriptActivityTester(&turns)
	check := func(stage, want string) {
		t.Helper()
		turns, _, _, _ = Conversation(state, "zh-CN")
		tester.Frame()
		tester.Frame()
		for _, key := range []string{"turn_awaiting_response", "turn_generating", "turn_waiting_controller"} {
			label := i18n.In("zh-CN", key)
			n := 0
			for _, text := range tester.Texts() {
				if text == label {
					n++
				}
			}
			if n > 1 || (n == 1) != (key == want) {
				t.Fatalf("%s: phase %q appears %d times, want %q: %q", stage, label, n, want, tester.Texts())
			}
		}
	}
	entry := func(id, author string, parts ...protocol.ConversationPart) {
		page.Entries = append(page.Entries, protocol.ConversationEntry{ID: id, Turn: turn, Sequence: uint64(len(page.Entries) + 1), Author: author, Parts: parts})
	}
	const idle = "turn_awaiting_response"
	entry("prompt", "user", protocol.ConversationPart{Kind: "text", Data: "Inspect the repository"})
	check("admitted", idle)
	part := transcriptTool("inspect", "inspect_fixture")
	entry("request", "assistant", part)
	page.Approvals = []protocol.ConversationApproval{{ID: protocol.ApprovalID{1}, Session: id, Turn: turn, Entry: "request", State: "pending", Source: "user"}}
	check("approval", "")
	if call := turns[0].Reply.Blocks[0].Calls[0]; !call.Pending || call.AwaitingModel {
		t.Fatalf("pending approval was projected as model wait: %+v", call)
	}
	page.Approvals[0].State = "approved"
	entry("start", "sailry", protocol.ConversationPart{Kind: "resource", Data: json.RawMessage(`{"type":"tool_started","id":"inspect","name":"inspect_fixture"}`)})
	check("tool running", "")
	if call := turns[0].Reply.Blocks[0].Calls[0]; !call.Running || call.AwaitingModel {
		t.Fatalf("started tool did not report only execution: %+v", call)
	}
	entry("done", "tool", transcriptResult("inspect", "inspect_fixture"))
	check("after tool", idle)
	if call := turns[0].Reply.Blocks[0].Calls[0]; call.Running || !call.AwaitingModel {
		t.Fatalf("returned tool did not report only model wait: %+v", call)
	}
	for range 3 {
		tester.Advance(500 * time.Millisecond)
		check("no new frame", idle)
	}
	transcriptActivityShot(t, tester, "transcript-after-tool")
	text := protocol.ConversationPart{Kind: "text", Data: "Partial reply"}
	state.Snapshot.Drafts = []protocol.ConversationDraft{{ID: "partial", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{text}}}
	check("streaming", "")
	if !tester.HasText("Partial reply") {
		t.Fatal("the current stream was hidden")
	}
	state.Snapshot.Drafts[0].Parts = append(state.Snapshot.Drafts[0].Parts, protocol.ConversationPart{Kind: "thinking", Data: "private"})
	check("reasoning after earlier output", "")
	if tester.HasText("private") {
		t.Fatal("private reasoning entered the transcript")
	}
	state.Snapshot.Drafts = nil
	entry("partial", "assistant", text)
	check("after committed text", "")
	part = transcriptTool("ask", "ask_user")
	ask := part.Data.(protocol.ConversationPartToolCallPayload)
	spec := &protocol.ConversationQuestionSpec{Prompt: "Which file?", Input: protocol.ConversationQuestionInput{Kind: "choice", Data: protocol.ConversationQuestionInputChoicePayload{Options: []string{"Main"}}}}
	ask.Summary.Question = &spec
	part.Data = ask
	entry("question", "assistant", part)
	page.Questions = []protocol.ConversationQuestion{{ID: protocol.QuestionID{1}, Session: id, Turn: turn, Entry: "question", State: protocol.ConversationQuestionState{Kind: "pending"}}}
	check("question", "")
	for _, status := range []string{"cancelled", "declined"} {
		page.Questions[0].State = protocol.ConversationQuestionState{Kind: status}
		check(status+" question", "")
	}
	page.Questions[0].State = protocol.ConversationQuestionState{Kind: "answered", Data: protocol.ConversationQuestionAnswer{Kind: "choices", Data: protocol.ConversationQuestionAnswerChoicesPayload{Selected: []uint64{0}}}}
	entry("answered", "tool", transcriptResult("ask", "ask_user"))
	check("question answered", idle)
	page.Runs[0].Waiting = &protocol.ConversationRunWaiting{
		Kind: "controller_login", Attempt: protocol.RequestID{1}, Provider: protocol.ProviderID{1},
		Credential: protocol.CredentialRef{Node: state.Snapshot.Node, ID: protocol.CredentialID{1}},
	}
	check("sign-in", "turn_waiting_controller")
	page.Runs[0].Waiting = nil
	state.Connected = false
	check("disconnected", "")
	state.Connected = true
	check("reconnected", idle)
	for _, status := range []protocol.ConversationStatus{"stopping", "cancelled", "interrupted", "failed", "completed"} {
		page.Runs[0].Status = status
		check(string(status), "")
	}
}

func TestHiddenDelegationDiagnosticKeepsChildBoundary(t *testing.T) {
	turn, session := protocol.TurnID{1}, protocol.SessionID{1}
	page := protocol.DefaultConversationPage()
	page.Session, page.Revision = session, 1
	worktree := protocol.WorktreeID{1}
	page.Runs = []protocol.ConversationRun{{Turn: turn, Session: session, Worktree: worktree, Sequence: 1, Revision: 1, Status: "running", Kind: "task"}}
	for i, id := range []string{"first", "again"} {
		if i != 0 {
			page.Entries = append(page.Entries, protocol.ConversationEntry{ID: "text", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Between launches"}}})
		}
		page.Entries = append(page.Entries, protocol.ConversationEntry{ID: id, Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{ID: &id, Name: "delegate", Presentation: protocol.ToolPresentationHidden, Summary: &protocol.ToolDetailSummary{}}}}})
		page.Children = append(page.Children, protocol.ConversationChild{Origin: protocol.Delegation{Session: session, Turn: turn, Entry: id}, Run: protocol.ConversationRun{Session: protocol.SessionID{2}, Turn: protocol.TurnID{byte(i + 2)}, Worktree: worktree, Sequence: uint64(i + 2), Revision: 1, Kind: "task", Status: "completed"}})
	}
	page.Entries = append(page.Entries, protocol.ConversationEntry{ID: "late-error", Turn: turn, Author: "tool", Parts: []protocol.ConversationPart{{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{ID: new("first"), Name: "delegate", Summary: &protocol.ToolDetailSummary{Failure: &protocol.ToolFailure{Origin: protocol.ToolFailureOriginTool}}}}}})
	for i := range page.Entries {
		page.Entries[i].Sequence = uint64(i + 1)
	}
	turns, _, _, _ := Conversation(client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Node: protocol.NodeID{1}, Page: page}})
	blocks := conversation.Compact(turns[0].Reply.Blocks)
	if len(blocks) != 4 || blocks[0].Kind != conversation.ToolsBlock || len(blocks[0].Calls) != 1 || blocks[0].Calls[0].ID != "first/0" || !blocks[0].Calls[0].Warning || blocks[1].Kind != conversation.ChildrenBlock || blocks[2].Text != "Between launches" || blocks[3].Kind != conversation.ChildrenBlock || len(blocks[1].Children) != 1 || len(blocks[3].Children) != 1 {
		t.Fatalf("a late diagnostic moved past text or the reused child: %+v", blocks)
	}
	if blocks[0].Calls[0].AwaitingModel {
		t.Fatal("late delegation diagnostic was projected as model wait")
	}
}
