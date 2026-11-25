package live

import (
	"context"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type workspaceStatusTransport struct {
	resourcePageTransport
	repository *conversationTestSubscription
	statuses   map[protocol.SessionID]*conversationTestSubscription
	chats      map[protocol.SessionID]*conversationTestSubscription
}

func (t *workspaceStatusTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	switch topic.Kind {
	case "repository":
		return t.repository, nil
	case "conversation":
		if id, ok := detail[protocol.SessionID](topic.Data); ok && t.chats[id] != nil {
			return t.chats[id], nil
		}
	case "status":
		if scope, ok := detail[protocol.PluginUiStatusScope](topic.Data); ok && scope.Session != nil && t.statuses[*scope.Session] != nil {
			return t.statuses[*scope.Session], nil
		}
	}
	return t.resourcePageTransport.Subscribe(ctx, topic)
}

func TestWorkspaceStatusRefreshKeepsLiveTranscripts(t *testing.T) {
	const count = 30
	parent, child := protocol.SessionID{1}, protocol.SessionID{2}
	project, tree := protocol.ProjectID{1}, protocol.WorktreeID{1}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	snapshot.Projects = []protocol.Project{{ID: project, Name: "Viewport", Path: "/fixture"}}
	snapshot.Worktrees = []protocol.Worktree{{ID: tree, Project: &project, Path: "/fixture", Main: true}}
	snapshot.Sessions = []protocol.Session{{ID: parent, Project: &project, Worktree: tree, Revision: 1,
		Config: protocol.DefaultSessionConfig(), Roles: protocol.DefaultRoleSnapshot()}}
	makeChat := func(id protocol.SessionID, prefix string, offset int) protocol.ConversationSnapshot {
		chat := protocol.DefaultConversationSnapshot()
		chat.Node, chat.Sequence = snapshot.Node, 1
		chat.Page.Session, chat.Page.Revision = id, 1
		for i := range count {
			turn := protocol.TurnID{byte(offset + i + 1)}
			started, finished := int64(1_700_000_000_000+i*2000), int64(1_700_000_001_000+i*2000)
			run := protocol.ConversationRun{Session: id, Turn: turn, Worktree: tree, Sequence: uint64(offset + i + 1), Revision: 1,
				Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusCompleted, StartedMs: &started, FinishedMs: &finished}
			paragraphs := 2
			if i == count-1 {
				run.Status, run.FinishedMs = protocol.ConversationStatusRunning, nil
				paragraphs = 12
			}
			chat.Page.Runs = append(chat.Page.Runs, run)
			chat.Page.Entries = append(chat.Page.Entries,
				protocol.ConversationEntry{ID: fmt.Sprintf("%s-user-%d", prefix, i), Turn: turn, Sequence: uint64(i*2 + 1), Author: "user",
					Parts: []protocol.ConversationPart{{Kind: "text", Data: fmt.Sprintf("%s message %02d", prefix, i)}}},
				protocol.ConversationEntry{ID: fmt.Sprintf("%s-reply-%d", prefix, i), Turn: turn, Sequence: uint64(i*2 + 2), Author: "assistant",
					Parts: []protocol.ConversationPart{{Kind: "text", Data: strings.Repeat("A paragraph keeps its place in this conversation.\n\n", paragraphs)}}})
		}
		return chat
	}
	parentChat, childChat := makeChat(parent, "Parent", 0), makeChat(child, "Child", count)
	parentRun := parentChat.Page.Runs[count-1]
	reply := &parentChat.Page.Entries[len(parentChat.Page.Entries)-1]
	for i := range 8 {
		id := protocol.SessionID{byte(i + 2)}
		title := fmt.Sprintf("Viewport helper %d", i)
		origin := &protocol.Delegation{Session: parent, Turn: parentRun.Turn, Entry: reply.ID, Index: uint64(i + 1)}
		snapshot.Sessions = append(snapshot.Sessions, protocol.Session{ID: id, Project: &project, Worktree: tree, Revision: 1,
			Config: protocol.DefaultSessionConfig(), Roles: protocol.DefaultRoleSnapshot(), Delegation: &origin})
		run := childChat.Page.Runs[0]
		if id != child {
			run = protocol.ConversationRun{Session: id, Turn: protocol.TurnID{byte(count*2 + i + 1)}, Worktree: tree, Sequence: uint64(count*2 + i + 1), Revision: 1,
				Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusRunning, StartedMs: parentRun.StartedMs}
		}
		parentChat.Page.Children = append(parentChat.Page.Children, protocol.ConversationChild{Origin: *origin, Run: run})
		call := protocol.DefaultConversationPartToolCallPayload()
		call.ID, call.Name = &title, "spawn_agent"
		call.Summary = &protocol.ToolDetailSummary{Revision: title, Title: &title, Delegation: protocol.ToolDetailDelegation{Background: true}}
		reply.Parts = append(reply.Parts, protocol.ConversationPart{Kind: "tool_call", Data: call})
	}
	subscription := func() *conversationTestSubscription {
		return &conversationTestSubscription{updates: make(chan protocol.Update, 3), closed: make(chan struct{})}
	}
	transport := &workspaceStatusTransport{
		resourcePageTransport: resourcePageTransport{conversationTestTransport: conversationTestTransport{snapshot: snapshot}},
		repository:            subscription(),
		statuses:              map[protocol.SessionID]*conversationTestSubscription{parent: subscription()},
		chats:                 map[protocol.SessionID]*conversationTestSubscription{parent: subscription(), child: subscription()},
	}
	for _, chat := range []protocol.ConversationSnapshot{parentChat, childChat} {
		transport.chats[chat.Page.Session].updates <- protocol.Update{Kind: "conversation_snapshot", Data: chat}
	}
	pushBadge := func(label string) {
		declaration := protocol.DefaultPluginUiContribution()
		declaration.ID, declaration.Slot, declaration.Kind = "badge", protocol.PluginUiSlotStatus, protocol.PluginUiKindButton
		declaration.Label.Label, declaration.Handler = label, new("click")
		state := protocol.DefaultPluginUiState()
		state.ID, state.Visible, state.Enabled = "badge", true, true
		status := protocol.DefaultPluginUiStatusSnapshot()
		status.Node, status.Scope = snapshot.Node, protocol.PluginUiStatusScope{Worktree: &tree, Session: &parent}
		status.Entries = []protocol.PluginUiStatusEntry{{
			Package:     protocol.PluginReference{Name: "status-fixture", Digest: strings.Repeat("a", 64)},
			Declaration: declaration, State: state, Invoke: new("invoke"),
		}}
		transport.statuses[parent].updates <- protocol.Update{Kind: "status", Data: status}
	}
	pushBadge("Plugin 1")
	var additions uint64 = 9
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		var output protocol.Output
		switch request.Command.Kind {
		case "inspect_git":
			branch, head := "main", "0123456789abcdef"
			output = protocol.Output{Kind: "git_status", Data: protocol.GitStatus{Kind: protocol.RepositoryKindReady,
				Branch: &branch, Head: &head, Entries: []protocol.GitEntry{{Path: "file.go", Diff: protocol.GitLineStats{Additions: additions, Deletions: 2}}}}}
		case "list_directory":
			output = protocol.Output{Kind: "directory", Data: protocol.Directory{Entries: []protocol.FileEntry{}}}
		case "list_git_branches":
			output = protocol.Output{Kind: "git_branches", Data: protocol.DefaultGitBranches()}
		case "read_git_log":
			output = protocol.Output{Kind: "git_log", Data: protocol.DefaultGitLog()}
		}
		return link.Response{Output: output}, output.Kind != ""
	}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = controller.Close() })
	r, err := NewWithAssembly(t.Context(), controller, preferences.ForStartup(t.TempDir()), testassembly.Full())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = r.Close() })
	r.App.SetWelcome(true)
	r.App.Sidebar.Selected = parent.String()
	prefs := r.App.Prefs()
	prefs.NavOpen, prefs.PanelOpen, prefs.OverviewHidden = false, false, true
	r.App.SetPrefs(prefs)
	r.App.Effects = false
	wakeup := make(chan struct{}, 1)
	r.SetInvalidate(func() {
		select {
		case wakeup <- struct{}{}:
		default:
		}
	})
	views := func() [2]*viewmodel.Conversation {
		return [2]*viewmodel.Conversation{r.App.Data.Find(parent.String()), r.childConversations[child]}
	}
	lists := [2]*ui.ListState{r.App.Messages.List(parent.String()), r.App.Messages.List(child.String())}
	type paneFrame struct {
		view        *viewmodel.Conversation
		firstTurn   *conversation.Turn
		first, last int
		atEnd       bool
	}
	type windowFrame struct {
		panes [2]paneFrame
		tab   panel.Tab
	}
	var frames []windowFrame
	watchFrames := false
	tester := ui.NewTesterWith(func(c *ui.Context) {
		r.App.View(c)
		c.Root().DrawOver(func(*ui.Painter, ui.Rect) {
			if !watchFrames {
				return
			}
			frame := windowFrame{tab: r.App.Panel.Active}
			for i, cv := range views() {
				p := &frame.panes[i]
				p.view = cv
				if cv != nil && !cv.Session.Loading && len(cv.Turns) == count {
					p.firstTurn = &cv.Turns[0]
				}
				p.first, p.last = lists[i].Visible()
				p.atEnd = lists[i].AtEnd()
			}
			frames = append(frames, frame)
		})
	}, 1320, 700, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
	tester.SetLocale("en")
	wait := func(label string, ready func() bool) {
		t.Helper()
		deadline := time.NewTimer(5 * time.Second)
		defer deadline.Stop()
		for {
			tester.Frame()
			if ready() {
				return
			}
			select {
			case <-wakeup:
			case <-deadline.C:
				p, _ := controller.Conversation(parent)
				c, _ := controller.Conversation(child)
				t.Fatalf("%s not rendered (parent sequence %d: %v; child sequence %d: %v)", label, p.Snapshot.Sequence, p.Err, c.Snapshot.Sequence, c.Err)
			case <-t.Context().Done():
				t.Fatal(t.Context().Err())
			}
		}
	}
	has := func(label string) bool { _, ok := tester.Access(label); return ok }
	wait("initial parent and badges", func() bool {
		cv := views()[0]
		return cv != nil && len(cv.Turns) == count && has("Git +9 -2") && has("Plugin 1")
	})
	r.openLiveChild(views()[0], child.String())
	wait("child tab", func() bool { return views()[1] != nil && len(views()[1].Turns) == count })
	const anchor = 15
	lists[0].ScrollTo(anchor, ui.Start)
	tester.Frame()
	before, visible := tester.Find("Parent message 15")
	if !visible || lists[0].AtEnd() || !lists[1].AtEnd() {
		t.Fatal("fixture did not show parent history and follow the active child")
	}
	stableViews := views()
	stableTab := r.App.Panel.Active
	assertFrames := func(label string, changed int, turns [2]*conversation.Turn) {
		t.Helper()
		watchFrames = false
		if len(frames) == 0 {
			t.Fatalf("no frames recorded for %s", label)
		}
		for i, frame := range frames {
			if frame.tab != stableTab {
				t.Fatalf("%s frame %d replaced the open child tab", label, i)
			}
			for side, p := range frame.panes {
				if p.view != stableViews[side] || p.firstTurn == nil {
					t.Fatalf("%s frame %d cleared or replaced pane %d", label, i, side)
				}
				if side == 0 && (p.atEnd || p.first > anchor || p.last < anchor) || side == 1 && (!p.atEnd || p.last != count-1) {
					t.Fatalf("%s frame %d moved pane %d: rows %d–%d, at end %t", label, i, side, p.first, p.last, p.atEnd)
				}
				if side != changed && p.firstTurn != turns[side] {
					t.Errorf("%s frame %d rebuilt the unchanged transcript of pane %d", label, i, side)
					turns[side] = p.firstTurn
				}
			}
		}
		if after, ok := tester.Find("Parent message 15"); !ok || after.Y != before.Y {
			t.Fatalf("%s moved the history anchor from %.1f to %.1f (visible %t)", label, before.Y, after.Y, ok)
		}
	}
	replay := func(label string, push func(), ready func() bool, changed int) {
		t.Helper()
		turns := [2]*conversation.Turn{&stableViews[0].Turns[0], &stableViews[1].Turns[0]}
		frames, watchFrames = nil, true
		push()
		wait(label, ready)
		assertFrames(label, changed, turns)
	}
	replay("Git", func() {
		transport.mu.Lock()
		additions = 1157
		transport.mu.Unlock()
		// Bypass only the quiet-refresh floor; the Client receives the real update.
		r.workspace.projects[tree].gitLastRead = time.Time{}
		transport.repository.updates <- protocol.Update{Kind: "files_changed", Data: protocol.UpdateFilesChangedPayload{Node: snapshot.Node, Worktree: tree, References: additions}}
	}, func() bool { return has("Git +1157 -2") }, -1)
	replay("plugin status", func() { pushBadge("Plugin 2048") }, func() bool { return has("Plugin 2048") }, -1)
	childRun := childChat.Page.Runs[count-1]
	const text = "The active child reply keeps growing."
	replay("child stream", func() {
		transport.chats[child].updates <- protocol.Update{Kind: "conversation_frame", Data: protocol.ConversationFrame{
			Node: snapshot.Node, Session: child, Sequence: childChat.Sequence + 1,
			Change: protocol.ConversationChange{Kind: "delta", Data: protocol.ConversationDraft{
				ID: "live-output", Turn: childRun.Turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: text}},
			}},
		}}
	}, func() bool { return tester.HasText(text) }, 1)
}
