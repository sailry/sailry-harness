package live

import (
	"bytes"
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type callbackTestTransport struct {
	conversationTestTransport
	history  *conversationTestSubscription
	commands chan string
}

func (transport *callbackTestTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind == "conversation" {
		return transport.history, nil
	}
	return transport.conversationTestTransport.Subscribe(ctx, topic)
}

func watchCallbackTranscript(t *testing.T, snapshot protocol.ConversationSnapshot) (*Runtime, *viewmodel.Conversation, *callbackTestTransport, func(string, any)) {
	t.Helper()
	session := protocol.DefaultSession()
	session.ID, session.Revision = snapshot.Page.Session, 1
	node := protocol.DefaultSnapshot()
	node.Node, node.Sessions = snapshot.Node, []protocol.Session{session}
	transport := &callbackTestTransport{conversationTestTransport: conversationTestTransport{snapshot: node},
		history: &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})}, commands: make(chan string, 2)}
	transport.history.updates <- protocol.Update{Kind: "conversation_snapshot", Data: snapshot}
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		switch request.Command.Kind {
		case "stop_turn", "guide_queued_turn":
			transport.commands <- request.Command.Kind
			return link.Response{Output: protocol.Output{Kind: "accepted"}}, true
		}
		return link.Response{}, false
	}
	controller := client.New(transport)
	t.Cleanup(func() { _ = controller.Close() })
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	cv := &viewmodel.Conversation{Node: node.Node.String(), ID: session.ID.String()}
	r := &Runtime{controller: controller, ctx: t.Context(), busy: map[string]bool{}, uncertain: map[string]protocol.Request{},
		App:                 shell.NewLive(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{cv}, Workspace: &viewmodel.Workspace{}}),
		conversationWatches: map[protocol.SessionID]func(){}}
	wake := make(chan struct{}, 1)
	r.SetInvalidate(func() {
		select {
		case wake <- struct{}{}:
		default:
		}
	})
	r.startConversationFeed(session.ID, "en")
	feed := r.conversationFeeds[session.ID]
	t.Cleanup(func() {
		r.conversationWatches[session.ID]()
		r.observers.Wait()
		r.workers.Wait()
	})
	await := func(sequence uint64) {
		t.Helper()
		ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
		defer cancel()
		for {
			r.takeConversationFeed(session.ID, feed)
			got := r.conversationCache[session.ID].state
			if got.Connected && got.Snapshot.Sequence == sequence {
				return
			}
			select {
			case <-wake:
			case <-ctx.Done():
				t.Fatalf("Desktop did not publish fixture frame %d (last %d, connected %t): %v", sequence, got.Snapshot.Sequence, got.Connected, got.Err)
			}
		}
	}
	await(snapshot.Sequence)
	sequence := snapshot.Sequence
	frame := func(kind string, value any) {
		t.Helper()
		sequence++
		transport.history.updates <- protocol.Update{Kind: "conversation_frame", Data: protocol.ConversationFrame{
			Node: snapshot.Node, Session: session.ID, Sequence: sequence, Change: protocol.ConversationChange{Kind: kind, Data: value},
		}}
		await(sequence)
	}
	return r, cv, transport, frame
}

func TestCallbackTurnsRetainRepliesStopAndUserQueue(t *testing.T) {
	session, user, guidance, auto, queued := protocol.SessionID{1}, protocol.TurnID{1}, protocol.TurnID{2}, protocol.TurnID{3}, protocol.TurnID{4}
	snapshot := protocol.DefaultConversationSnapshot()
	snapshot.Node, snapshot.Sequence = protocol.NodeID{1}, 5
	page := &snapshot.Page
	page.Session, page.Revision = session, 1
	legacy := func(id byte, title string) []protocol.ConversationBackgroundNotice {
		child := protocol.TurnID{id}
		return []protocol.ConversationBackgroundNotice{{ID: "agent:" + child.String(), Title: title, Outcome: protocol.ConversationBackgroundOutcomeFailed,
			Kind:    protocol.ConversationBackgroundKind{Kind: "agent", Data: protocol.ConversationBackgroundKindAgentPayload{Session: protocol.SessionID{id}, Turn: child}},
			Summary: new("Private completion evidence")}}
	}
	automatic := protocol.ConversationRun{Turn: auto, Session: session, Sequence: 3, Revision: 1, Kind: "task", Status: "running",
		Notice: true, Notices: legacy(12, "Legacy automatic callback"), StartedMs: new(int64(3000))}
	page.Runs = []protocol.ConversationRun{
		{Turn: user, Session: session, Sequence: 1, Revision: 1, Kind: "task", Status: "completed", Notices: legacy(11, "Legacy normal callback"), FinishedMs: new(int64(2000))},
		{Turn: guidance, Session: session, Sequence: 2, Revision: 1, Kind: "task", Status: "continued", Joined: &user},
		automatic,
		{Turn: queued, Session: session, Sequence: 4, Revision: 1, Kind: "task", Status: "queued"},
	}
	part := func(text string) protocol.ConversationPart {
		return protocol.ConversationPart{Kind: "text", Data: text}
	}
	page.Entries = []protocol.ConversationEntry{
		{ID: "user", Turn: user, Sequence: 1, Author: "user", Parts: []protocol.ConversationPart{part("Initial user request")}},
		{ID: "first", Turn: user, Sequence: 2, Author: "assistant", Parts: []protocol.ConversationPart{part("Initial assistant response")}},
		{ID: "guidance", Turn: guidance, Sequence: 3, Author: "user", Parts: []protocol.ConversationPart{part("Actual user guidance")}},
		{ID: "second", Turn: user, Sequence: 4, Author: "assistant", Parts: []protocol.ConversationPart{part("Later assistant response")}},
		{ID: "legacy-input", Turn: auto, Sequence: 5, Author: "user", Parts: []protocol.ConversationPart{part("Legacy model-only input")}},
	}
	// A run update can arrive before the queue drops the same automatic turn.
	page.Queue = protocol.ConversationQueue{Revision: 3, Items: []protocol.ConversationPending{
		{Turn: auto, Kind: "task", Revision: 1, ConfigRevision: 1, Notice: true, Ready: true, Preview: "Legacy automatic callback"},
		{Turn: queued, Kind: "task", Revision: 2, ConfigRevision: 1, Ready: true, Preview: "Real queued message"},
	}}
	r, cv, transport, frame := watchCallbackTranscript(t, snapshot)
	comp, view := composer.New(cv.ID), conversation.New()
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		actions := &conversationActions{r: r, c: c, cv: cv, comp: comp}
		comp.Running = cv.Running()
		ui.Column(c).Fill().Children(func() {
			ui.Column(c).Grow(1).MinHeight(0).Children(func() { conversation.View(c, view, cv.ID, cv.Turns, &cv.Session, actions) })
			composer.View(c, comp, &composer.Context{Queue: cv.Queue, Activity: composer.Activity{Connected: true}}, actions)
		})
	}, 950, 820)
	tester.SetLocale("en")
	tester.SetPreferences(ui.Preferences{ReduceMotion: true, ReduceTransparency: true})
	show := func() {
		cacheTestFrame(t, r, cv, session)
		tester.Frame()
		tester.Frame()
		for _, hidden := range []string{"Legacy normal callback", "Legacy automatic callback", "Legacy model-only input", "Private completion evidence", i18n.In("en", "background_notice_title")} {
			if _, visible := tester.Access(hidden); visible || tester.HasText(hidden) {
				t.Fatalf("callback evidence became a transcript or queue row: %q", tester.Texts())
			}
		}
	}
	press := func(key, command string) {
		t.Helper()
		button, ok := tester.AccessByRole(ui.RoleButton, i18n.In("en", key))
		if !ok || button.Disabled {
			t.Fatalf("%s is not available: %q", key, tester.Texts())
		}
		tester.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
		tester.Frame()
		select {
		case got := <-transport.commands:
			if got != command {
				t.Fatalf("%s dispatched %s, want %s", key, got, command)
			}
		case <-time.After(5 * time.Second):
			t.Fatalf("%s did not dispatch %s", key, command)
		}
	}
	show()
	if len(cv.Turns) != 2 || len(cv.Turns[0].Segments) != 2 || cv.Turns[1].User != "" || len(cv.Turns[1].Reply.Blocks) != 0 || !cv.Running() {
		t.Fatalf("a notice changed actual input or invented work: %+v", cv.Turns)
	}
	for _, text := range []string{"Initial user request", "Initial assistant response", "Actual user guidance", "Later assistant response", i18n.In("en", "turn_awaiting_response")} {
		if !tester.HasText(text) {
			t.Fatalf("%q was hidden: %q", text, tester.Texts())
		}
	}
	if len(cv.Queue) != 1 || cv.Queue[0].ID != queued.String() || cv.Queue[0].Notice {
		t.Fatalf("wrong queue during automatic follow-up: %+v", cv.Queue)
	}
	unfoldQueue(t, tester)
	if !tester.HasText("Real queued message") {
		t.Fatalf("queued user text was hidden: %q", tester.Texts())
	}
	press("queue_send_now", "guide_queued_turn")

	frame("queue", protocol.ConversationQueue{Revision: 4, Items: page.Queue.Items[1:]})
	answer := part("Automatic assistant response")
	frame("delta", protocol.ConversationDraft{ID: "automatic-reply", Turn: auto, Author: "assistant", Parts: []protocol.ConversationPart{answer}})
	show()
	if !tester.HasText(answer.Data.(string)) || cv.Turns[1].Reply.Phase != conversation.Generating {
		t.Fatalf("the automatic response did not stream: %q", tester.Texts())
	}
	press("chat_stop", "stop_turn")
	transport.mu.Lock()
	requests := append([]protocol.Request(nil), transport.requests...)
	transport.mu.Unlock()
	if len(requests) != 2 || requests[0].Command.Kind != "guide_queued_turn" || requests[1].Command.Kind != "stop_turn" {
		t.Fatalf("unexpected composer requests: %+v", requests)
	}
	guide := requests[0].Command.Data.(protocol.CommandGuideQueuedTurnPayload)
	stop := requests[1].Command.Data.(protocol.CommandStopTurnPayload)
	if guide.Turn != queued || guide.Recipient != auto || guide.ExpectedRevision != 2 || stop.Turn != auto {
		t.Fatalf("the controls did not target the actual inputs and active run: guide=%+v stop=%+v", guide, stop)
	}
	frame("entry", protocol.ConversationEntry{ID: "automatic-reply", Turn: auto, Sequence: 6, Author: "assistant", Parts: []protocol.ConversationPart{answer}})
	automatic.Status, automatic.FinishedMs = "cancelled", new(int64(5000))
	frame("run", automatic)
	show()
	if cv.Running() || !tester.HasText(i18n.In("en", "turn_cancelled")) || !tester.HasText(answer.Data.(string)) || !tester.HasText("Real queued message") {
		t.Fatalf("stopping the automatic run lost its response, status or the queue: %q", tester.Texts())
	}
}

func TestCallbackActivityStaysAtOriginalRows(t *testing.T) {
	session, turn, worktree, command := protocol.SessionID{1}, protocol.TurnID{1}, protocol.WorktreeID{1}, protocol.RequestID{1}
	snapshot := protocol.DefaultConversationSnapshot()
	snapshot.Node, snapshot.Sequence = protocol.NodeID{1}, 8
	page := &snapshot.Page
	page.Session, page.Revision = session, 1
	page.Runs = []protocol.ConversationRun{{Session: session, Turn: turn, Worktree: worktree, Kind: "task", Status: "completed", Sequence: 1, Revision: 1}}
	task := protocol.ConversationBackgroundTask{ID: "command:" + command.String(), Title: "Worker process", Session: session, Turn: turn,
		Kind:      protocol.ConversationBackgroundKind{Kind: "command", Data: protocol.ConversationBackgroundKindCommandPayload{ID: command}},
		StartedMs: 1000, Revision: 1, State: protocol.ConversationBackgroundStateRunning}
	page.Background = []protocol.ConversationBackgroundTask{task}
	const commandLabel, nextLabel = "Start background worker", "Check worker files"
	const commandInput, commandReceipt, stderr = "run-worker --fixture", "Worker launch acknowledged", "Worker could not write the fixture output"
	start := transcriptTool("start", "run_command")
	finished := transcriptResult("start", "run_command")
	launch := finished.Data.(protocol.ConversationPartToolResultPayload).Summary
	launch.Task, launch.Title, launch.Path = &task.ID, new(commandLabel), new(commandInput)
	launch.HasDetails = true
	launch.Resolved = &protocol.ToolDetailResolved{Output: &protocol.ToolDetailOutput{Preview: commandReceipt, Paths: []string{}}}
	next := transcriptTool("files", "run_command")
	nextResult := transcriptResult("files", "run_command")
	files := nextResult.Data.(protocol.ConversationPartToolResultPayload).Summary
	files.Title, files.HasDetails = new(nextLabel), true
	files.Resolved = &protocol.ToolDetailResolved{Output: &protocol.ToolDetailOutput{Paths: []string{"log.txt", "trace.txt"}}}
	page.Entries = []protocol.ConversationEntry{
		{ID: "commands", Turn: turn, Sequence: 1, Author: "assistant", Parts: []protocol.ConversationPart{start, next}},
		{ID: "receipts", Turn: turn, Sequence: 2, Author: "tool", Parts: []protocol.ConversationPart{nextResult, finished}},
		{ID: "delegations", Turn: turn, Sequence: 3, Author: "assistant"},
		{ID: "answer", Turn: turn, Sequence: 4, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "The work was launched"}}},
	}
	for i, title := range []string{"First worker", "Second worker"} {
		part := transcriptTool(title, "spawn_agent")
		call := part.Data.(protocol.ConversationPartToolCallPayload)
		call.Presentation = protocol.ToolPresentationHidden
		call.Summary.Title, call.Summary.Delegation.Background = &title, true
		part.Data = call
		page.Entries[2].Parts = append(page.Entries[2].Parts, part)
		page.Children = append(page.Children, protocol.ConversationChild{
			Origin: protocol.Delegation{Session: session, Turn: turn, Entry: "delegations", Index: uint64(i)},
			Run: protocol.ConversationRun{Session: protocol.SessionID{byte(i + 2)}, Turn: protocol.TurnID{byte(i + 2)}, Worktree: worktree,
				Sequence: uint64(i + 2), Revision: 1, Kind: "task", Status: "running"},
		})
	}
	r, cv, _, frame := watchCallbackTranscript(t, snapshot)
	tester := transcriptActivityTester(&cv.Turns)
	tester.SetLocale("en")
	show := func() client.ConversationState {
		state := cacheTestFrame(t, r, cv, session)
		tester.Frame()
		tester.Frame()
		return state
	}
	click := func(label string) {
		t.Helper()
		if err := tester.Click(label); err != nil {
			t.Fatalf("%q cannot be opened: %v; %q", label, err, tester.Texts())
		}
		tester.Frame()
	}
	show()
	click(i18n.In("en", "turn_completed"))
	// The command and the call after it fold into one group, shown by the
	// latter; its own row is the lower one.
	click(nextLabel)
	var nextRow ui.AccessInfo
	for _, a := range tester.AccessAll() {
		if a.Role == ui.RoleButton && strings.HasPrefix(a.Name, nextLabel) && a.Bounds.Y > nextRow.Bounds.Y {
			nextRow = a
		}
	}
	tester.ClickAt(nextRow.Bounds.X+nextRow.Bounds.W/2, nextRow.Bounds.Y+nextRow.Bounds.H/2)
	tester.Frame()
	click(commandLabel)
	if !tester.HasText(commandInput) || !tester.HasText(commandReceipt) || tester.HasText(i18n.In("en", "background_running")) || !tester.HasText("log.txt") {
		t.Fatalf("the grouped command cannot be inspected: %q", tester.Texts())
	}
	click(i18n.In("en", "subagent_count", "completed", 0, "total", 2))

	task.State, task.Outcome, task.ExitCode = protocol.ConversationBackgroundStatePending, new(protocol.ConversationBackgroundOutcomeFailed), new(int32(7))
	task.Summary, task.FinishedMs, task.Revision = new(stderr), new(int64(2000)), 2
	frame("background", task)
	first, second := page.Children[0], page.Children[1]
	const reason = "Worker rejected the requested configuration"
	first.Run.Status, first.Run.Error = "failed", &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: reason}
	second.Run.Status = "completed"
	frame("child", first)
	frame("child", second)
	state := show()
	exit := i18n.In("en", "background_exit", "code", 7)
	waiting := i18n.In("en", "background_result_waiting")
	if state.Snapshot.Page.Revision != snapshot.Page.Revision || tester.HasText(exit) || tester.HasText(waiting) || !tester.HasText(stderr) || !tester.HasText(commandReceipt) || tester.HasText(i18n.In("en", "background_running")) {
		t.Fatalf("activity did not update the open command in place: %q", tester.Texts())
	}
	button, ok := tester.AccessByRole(ui.RoleButton, "First worker")
	if !ok || button.Description != i18n.In("en", "turn_failed")+"\n"+reason || !tester.HasText(i18n.In("en", "subagent_count", "completed", 2, "total", 2)) {
		t.Fatalf("the open child group lost the failure and status update: %+v, %q", button, tester.Texts())
	}
	task.State, task.Revision = protocol.ConversationBackgroundStateConsumed, 3
	frame("background", task)
	state = show()
	if tester.HasText(exit) || tester.HasText(waiting) || !tester.HasText(stderr) {
		t.Fatalf("delivery did not preserve the command's terminal details: %q", tester.Texts())
	}
	count := 0
	for _, text := range tester.Texts() {
		if text == stderr {
			count++
		}
	}
	if count != 1 {
		t.Fatalf("%q appeared %d times: %q", stderr, count, tester.Texts())
	}
	blocks := conversation.Compact(cv.Turns[0].Reply.Blocks)
	if len(blocks) != 3 || blocks[0].Kind != conversation.ToolsBlock || len(blocks[0].Calls) != 2 ||
		blocks[0].Calls[0].ID != "commands/0" || blocks[0].Calls[0].Background == nil || blocks[0].Calls[1].Background != nil ||
		blocks[1].Kind != conversation.ChildrenBlock || len(blocks[1].Children) != 2 || blocks[1].Children[0].Turn != first.Run.Turn.String() || blocks[1].Children[0].Error != reason || blocks[2].Text != "The work was launched" {
		t.Fatalf("activity added or moved a transcript row: %+v", blocks)
	}
	encoded, err := json.Marshal(state.Snapshot)
	if err != nil {
		t.Fatal(err)
	}
	var replay protocol.ConversationSnapshot
	if err := protocol.DecodeAs(protocol.PeerData, encoded, &replay); err != nil {
		t.Fatal(err)
	}
	turns := r.prepareConversation(session, client.ConversationState{Connected: true, Snapshot: replay}, "en").view.Turns
	if len(turns) != 1 {
		t.Fatalf("reopened history has %d turns, want one", len(turns))
	}
	reopened := conversation.Compact(turns[0].Reply.Blocks)
	liveRows, liveErr := json.Marshal(blocks)
	replayedRows, replayErr := json.Marshal(reopened)
	if liveErr != nil || replayErr != nil || !bytes.Equal(liveRows, replayedRows) {
		t.Fatalf("reopened history changed the command or child rows: %v, %v\nlive: %s\nreplay: %s", liveErr, replayErr, liveRows, replayedRows)
	}
	for i, call := range blocks[0].Calls {
		got := reopened[0].Calls[i]
		if (call.Load == nil) != (got.Load == nil) || (call.BackgroundDetail == nil) != (got.BackgroundDetail == nil) {
			t.Fatalf("reopened history changed the disclosure actions of %q", call.ID)
		}
	}
}
