package live

import (
	"context"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestOpenLiveChildRevealsPanel(t *testing.T) {
	node, tree := protocol.NodeID{1}, protocol.WorktreeID{1}
	parent, child, turn := protocol.SessionID{1}, protocol.SessionID{2}, protocol.TurnID{1}
	origin := &protocol.Delegation{Session: parent, Turn: turn, Entry: "spawn"}
	config := protocol.DefaultSessionConfig()
	config.Model = "fixture"
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = node
	snapshot.Worktrees = []protocol.Worktree{{ID: tree, Main: true}}
	snapshot.Sessions = []protocol.Session{
		{ID: parent, Worktree: tree, Revision: 1, Config: config, Roles: protocol.DefaultRoleSnapshot()},
		{ID: child, Worktree: tree, Revision: 1, Config: config, Roles: protocol.DefaultRoleSnapshot(), Delegation: &origin},
	}
	history := protocol.DefaultConversationSnapshot()
	history.Node, history.Page.Session, history.Page.Revision = node, parent, 1
	history.Page.Runs = []protocol.ConversationRun{{Session: parent, Worktree: tree, Turn: turn, Sequence: 1, Revision: 1, Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusRunning}}
	call := protocol.DefaultConversationPartToolCallPayload()
	call.Name, call.Summary = "spawn_agent", &protocol.ToolDetailSummary{}
	history.Page.Entries = []protocol.ConversationEntry{{ID: origin.Entry, Sequence: 1, Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "tool_call", Data: call}}}}
	history.Page.Children = []protocol.ConversationChild{{Run: protocol.ConversationRun{Session: child, Worktree: tree, Turn: protocol.TurnID{2}, Sequence: 2, Revision: 1, Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusRunning}, Origin: *origin}}

	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	controller := client.New(&conversationTestTransport{snapshot: snapshot, conversation: history})
	defer controller.Close()
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	changed := make(chan struct{}, 1)
	release, err := controller.WatchConversation(ctx, parent, func() {
		select {
		case changed <- struct{}{}:
		default:
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	for {
		state, ok := controller.Conversation(parent)
		if ok && state.Err != nil {
			t.Fatal(state.Err)
		}
		if ok && state.Connected {
			break
		}
		select {
		case <-changed:
		case <-ctx.Done():
			t.Fatal(ctx.Err())
		}
	}
	cv := &viewmodel.Conversation{Node: node.String(), ID: parent.String()}
	app := shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})
	app.Sidebar.Selected = cv.ID
	app.MountPanel()
	runtime := &Runtime{App: app, ctx: ctx, controller: controller, teams: newTeamState(),
		busy: map[string]bool{}, admittedSessions: map[string]protocol.Session{}}
	tester := ui.NewTester(func(c *ui.Context) {
		for _, apply := range runtime.queue {
			apply(c)
		}
		runtime.queue = nil
	}, 100, 100)
	tab := panel.Tab{Kind: panel.Subagent, ID: child.String()}
	for range 2 {
		app.PanelOpen = false
		runtime.openLiveChild(cv, tab.ID)
		runtime.workers.Wait()
		tester.Frame()
		if !app.PanelOpen || app.Panel.Active != tab || len(app.Panel.Tabs) != 1 {
			t.Fatalf("opening a live child did not reveal its tab: open=%v tabs=%v", app.PanelOpen, app.Panel.Tabs)
		}
	}

	app.PanelOpen = false
	runtime.openLiveChild(cv, tab.ID)
	app.Sidebar.Selected = "another conversation"
	app.MountPanel()
	runtime.workers.Wait()
	tester.Frame()
	if app.PanelOpen || len(app.Panel.Tabs) != 0 {
		t.Fatalf("a delayed child read changed another conversation: open=%v tabs=%v", app.PanelOpen, app.Panel.Tabs)
	}
}
