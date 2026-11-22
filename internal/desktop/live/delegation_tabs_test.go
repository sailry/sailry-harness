package live

import (
	"context"
	"slices"
	"testing"
	"testing/synctest"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func liveChildFixture() (protocol.Snapshot, protocol.ConversationSnapshot) {
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
	return snapshot, history
}

func TestOpenLiveChildRevealsPanel(t *testing.T) {
	snapshot, history := liveChildFixture()
	node, parent, child := snapshot.Node, history.Page.Session, snapshot.Sessions[1].ID

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

type childTestTransport struct {
	conversationTestTransport
	node, chat *conversationTestSubscription
}

func (source *childTestTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	switch topic.Kind {
	case "node":
		return source.node, nil
	case "conversation":
		return source.chat, nil
	default:
		return source.conversationTestTransport.Subscribe(ctx, topic)
	}
}

func childTestRuntime(t *testing.T, snapshot protocol.Snapshot, history protocol.ConversationSnapshot) (*Runtime, *childTestTransport, *viewmodel.Conversation) {
	t.Helper()
	source := &childTestTransport{
		conversationTestTransport: conversationTestTransport{snapshot: snapshot},
		node:                      &conversationTestSubscription{updates: make(chan protocol.Update, 4), closed: make(chan struct{})},
		chat:                      &conversationTestSubscription{updates: make(chan protocol.Update, 4), closed: make(chan struct{})},
	}
	source.node.updates <- protocol.Update{Kind: "snapshot", Data: snapshot}
	source.chat.updates <- protocol.Update{Kind: "conversation_snapshot", Data: history}
	controller := client.New(source)
	t.Cleanup(func() { _ = controller.Close() })
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	cv := &viewmodel.Conversation{Node: snapshot.Node.String(), ID: history.Page.Session.String()}
	app := shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}, Unassigned: []*viewmodel.Conversation{cv}})
	app.Sidebar.Selected = cv.ID
	app.MountPanel()
	app.PanelOpen = false
	r := &Runtime{App: app, ctx: t.Context(), controller: controller, teams: newTeamState(),
		busy: map[string]bool{}, admittedSessions: map[string]protocol.Session{}}
	release, err := controller.WatchConversation(t.Context(), history.Page.Session, func() { r.conversationChanged(history.Page.Session) })
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(release)
	r.conversationWatches = map[protocol.SessionID]func(){history.Page.Session: release}
	synctest.Wait()
	if state, ok := controller.Conversation(history.Page.Session); !ok || !state.Connected || state.Err != nil {
		t.Fatalf("parent observation not ready: %+v", state)
	}
	return r, source, cv
}

func childTestFrame(r *Runtime, render func(*ui.Context)) *ui.Tester {
	return ui.NewTester(func(c *ui.Context) {
		r.mu.Lock()
		queue := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, apply := range queue {
			apply(c)
		}
		if render != nil {
			render(c)
		}
	}, 520, 400)
}

func TestLiveChildObservationsCanArriveInEitherOrder(t *testing.T) {
	for _, childFirst := range []bool{true, false} {
		name := "inventory first"
		if childFirst {
			name = "canonical child first"
		}
		t.Run(name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				inventory, history := liveChildFixture()
				child := inventory.Sessions[1]
				nodeFirst, chatFirst := inventory, history
				if childFirst {
					nodeFirst.Sessions = inventory.Sessions[:1]
				} else {
					chatFirst.Page.Children = nil
				}
				r, source, cv := childTestRuntime(t, nodeFirst, chatFirst)
				source.sessions = map[protocol.SessionID]protocol.Session{child.ID: child}
				tab := panel.Tab{Kind: panel.Subagent, ID: child.ID.String()}
				frame := childTestFrame(r, nil)
				want := []string{"read_session"}
				if !childFirst {
					r.openLiveChild(cv, protocol.SessionID{8}.String())
					synctest.Wait()
					frame.Frame()
					if len(source.requests) != 0 || r.App.PanelOpen {
						t.Fatal("unknown child ID admitted a read")
					}
					want = []string{"read_turn", "read_session"}
					source.handle = func(request protocol.Request) (link.Response, bool) {
						if request.Command.Kind != "read_turn" {
							return link.Response{}, false
						}
						query, ok := detail[protocol.CommandReadTurnPayload](request.Command.Data)
						if !ok || query.Session != history.Page.Session || query.Turn != history.Page.Runs[0].Turn ||
							query.ExpectedRevision != history.Page.Revision || query.Before != nil {
							t.Errorf("unexpected founding proof query: %+v", request)
						}
						return link.Response{Output: protocol.Output{Kind: "turn_history", Data: protocol.ConversationTurnHistory{
							Revision: history.Page.Revision, Run: history.Page.Runs[0], Entries: history.Page.Entries, Children: history.Page.Children,
						}}}, true
					}
				}
				r.openLiveChild(cv, tab.ID)
				synctest.Wait()
				frame.Frame()
				if !r.App.PanelOpen || r.App.Panel.Active != tab || !slices.EqualFunc(source.requests, want, func(request protocol.Request, kind string) bool {
					return request.Target == inventory.Node && request.Command.Kind == kind
				}) {
					t.Fatalf("validated receipt did not open the child: %v, %v", r.App.Panel.Tabs, source.requests)
				}
				if childFirst {
					if len(r.snapshot().Sessions) != 1 || r.admittedSessions[tab.ID].ID != child.ID {
						t.Fatal("fixture did not exercise a receipt ahead of inventory")
					}
					source.node.updates <- protocol.Update{Kind: "event", Data: protocol.EventEnvelope{
						Node: inventory.Node, Cursor: inventory.Cursor + 1, Event: protocol.Event{Kind: "session_changed", Data: child},
					}}
					synctest.Wait()
				} else {
					state, _ := r.controller.Conversation(history.Page.Session)
					if len(state.Snapshot.Page.Children) != 0 || len(state.Snapshot.Page.Background) != 0 {
						t.Fatal("inventory-first proof merged the child's origin into the parent observation")
					}
					source.chat.updates <- protocol.Update{Kind: "conversation_frame", Data: protocol.ConversationFrame{
						Node: history.Node, Session: history.Page.Session, Sequence: history.Sequence + 1,
						Change: protocol.ConversationChange{Kind: "child", Data: history.Page.Children[0]},
					}}
					synctest.Wait()
				}
				state, _ := r.controller.Conversation(history.Page.Session)
				targets := slices.Collect(r.liveChildren(state))
				if len(targets) != 1 || !targets[0].Listed || !targets[0].MatchesSession(child) {
					t.Fatal("both observations did not converge to the same child")
				}
			})
		})
	}
}

func offTailLiveChildFixture() (protocol.Snapshot, protocol.ConversationSnapshot, protocol.ConversationTurnHistory) {
	inventory, history := liveChildFixture()
	child := history.Page.Children[0]
	inventory.Sessions[1].Activity.Run = &child.Run
	older := protocol.DefaultConversationTurnHistory()
	older.Revision, older.Run, older.Entries, older.Children = history.Page.Revision, history.Page.Runs[0], history.Page.Entries, history.Page.Children
	older.Run.Status = protocol.ConversationStatusCompleted
	latest := protocol.TurnID{3}
	history.Page.Runs = []protocol.ConversationRun{{Session: history.Page.Session, Worktree: child.Run.Worktree, Turn: latest,
		Sequence: 3, Revision: 1, Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusCompleted}}
	history.Page.NextBefore = &latest
	history.Page.Entries, history.Page.Children = nil, nil
	history.Page.Background = []protocol.ConversationBackgroundTask{{ID: "agent:" + child.Run.Turn.String(),
		Session: history.Page.Session, Turn: older.Run.Turn, State: protocol.ConversationBackgroundStateRunning, Revision: 1, Title: "Off-tail child",
		Kind: protocol.ConversationBackgroundKind{Kind: "agent", Data: protocol.ConversationBackgroundKindAgentPayload{Session: child.Run.Session, Turn: child.Run.Turn}}}}
	return inventory, history, older
}

func TestOffTailLiveChildUsesOnlyItsParentTurn(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		inventory, history, older := offTailLiveChildFixture()
		r, source, cv := childTestRuntime(t, inventory, history)
		child := inventory.Sessions[1]
		rewound := false
		releaseProof := make(chan struct{})
		source.handle = func(request protocol.Request) (link.Response, bool) {
			if request.Command.Kind != "read_turn" {
				return link.Response{}, false
			}
			<-releaseProof
			query, ok := detail[protocol.CommandReadTurnPayload](request.Command.Data)
			if !ok || query.Session != history.Page.Session || query.Turn != older.Run.Turn || query.ExpectedRevision != history.Page.Revision || query.Before != nil {
				t.Errorf("unscoped parent read: %+v", request)
			}
			if rewound {
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "history changed"}}, true
			}
			return link.Response{Output: protocol.Output{Kind: "turn_history", Data: older}}, true
		}
		var overview *composer.Overview
		var activity *composer.Context
		frame := childTestFrame(r, func(c *ui.Context) {
			r.beginConversationFrame()
			r.conversationSeen[history.Page.Session] = true
			activity = &composer.Context{}
			r.projectChildComposer(cv, r.App.Composer(cv.ID), activity, c.Locale())
			overview = r.sessionOverview(c, cv)
			r.finishConversationFrame()
		})
		frame.Frame()
		synctest.Wait()
		unverified := len(activity.Activity.Children) != 0 || len(overview.Children) != 0
		close(releaseProof)
		if unverified {
			t.Fatal("unverified background child was presented as trusted")
		}
		synctest.Wait()
		frame.Frame()
		if len(activity.Activity.Children) != 1 || activity.Activity.Children[0].ID != child.ID.String() ||
			len(overview.Children) != 1 || overview.Children[0].Name != "Off-tail child" {
			t.Fatalf("confirmed off-tail activity missing: %+v, %+v", activity.Activity.Children, overview.Children)
		}
		overview.OpenChild(child.ID.String())
		synctest.Wait()
		frame.Frame()
		if r.App.Panel.Active != (panel.Tab{Kind: panel.Subagent, ID: child.ID.String()}) {
			t.Fatal("confirmed off-tail child did not open")
		}
		if !slices.EqualFunc(source.requests, []string{"read_turn", "read_turn", "read_session"}, func(request protocol.Request, kind string) bool {
			return request.Target == inventory.Node && request.Command.Kind == kind
		}) {
			t.Fatalf("off-tail lookup did not remain targeted: %+v", source.requests)
		}
		state, _ := r.controller.Conversation(history.Page.Session)
		if len(state.Snapshot.Page.Entries) != 0 || len(state.Snapshot.Page.Children) != 0 || state.Snapshot.Page.NextBefore == nil {
			t.Fatal("child lookup expanded the parent transcript")
		}
		// The Node rewound before this click, but its conversation observation
		// is still at the revision that the overview verified.
		rewound = true
		r.App.PanelOpen = false
		overview.OpenChild(child.ID.String())
		synctest.Wait()
		frame.Frame()
		if r.App.PanelOpen || len(source.requests) != 4 || source.requests[3].Command.Kind != "read_turn" {
			t.Fatal("stale display proof admitted a child read after rewind")
		}
	})
}

func TestLiveChildRedrawRefreshesOwningNode(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		localInventory, localHistory := liveChildFixture()
		local, _, localView := childTestRuntime(t, localInventory, localHistory)
		remoteInventory, remoteHistory := liveChildFixture()
		remoteInventory.Node, remoteHistory.Node = protocol.NodeID{2}, protocol.NodeID{2}
		remoteInventory.Sessions[1].Config.Model = "remote model"
		remote, source, remoteView := childTestRuntime(t, remoteInventory, remoteHistory)
		remote.parent, remote.App = local, local.App
		local.bindings = map[string]*Runtime{remoteView.Node: remote}

		var activity composer.Context
		var overview *composer.Overview
		owner, view := local, localView
		show := true
		frame := childTestFrame(local, func(c *ui.Context) {
			local.beginConversationFrame()
			remote.beginConversationFrame()
			if show {
				owner.conversationSeen[localHistory.Page.Session] = true
				owner.projectChildComposer(view, local.App.Composer(view.ID), &activity, c.Locale())
				overview = local.sessionOverview(c, view)
			}
			local.finishConversationFrame()
			remote.finishConversationFrame()
		})
		assertModel := func(want string) {
			t.Helper()
			if len(activity.Activity.Children) != 1 || activity.Activity.Children[0].Model != want || len(overview.Children) != 1 {
				t.Fatalf("child projection did not show model %q: %v, %v", want, activity.Activity.Children, overview.Children)
			}
		}
		frame.Frame()
		assertModel("fixture")
		owner, view = remote, remoteView
		frame.Frame()
		assertModel("remote model")
		if len(local.overviews) != 0 {
			t.Fatal("switching Node retained the previous conversation projection")
		}

		child := remoteInventory.Sessions[1]
		local.confirmedRemovals = map[protocol.NodeID]map[protocol.SessionID]bool{remoteInventory.Node: {child.ID: true}}
		frame.Frame()
		if len(activity.Activity.Children) != 0 || len(overview.Children) != 0 {
			t.Fatal("a hidden child remained visible through a cached projection")
		}
		local.confirmedRemovals = nil
		child.Revision++
		child.Config.Model = "updated remote model"
		source.node.updates <- protocol.Update{Kind: "event", Data: protocol.EventEnvelope{
			Node: remoteInventory.Node, Cursor: remoteInventory.Cursor + 1, Event: protocol.Event{Kind: "session_changed", Data: child},
		}}
		synctest.Wait()
		frame.Frame()
		assertModel(child.Config.Model)

		run := remoteHistory.Page.Children[0]
		run.Run.Status, run.Run.FinishedMs = protocol.ConversationStatusCompleted, new(int64(1))
		source.chat.updates <- protocol.Update{Kind: "conversation_frame", Data: protocol.ConversationFrame{
			Node: remoteHistory.Node, Session: remoteHistory.Page.Session, Sequence: remoteHistory.Sequence + 1,
			Change: protocol.ConversationChange{Kind: "child", Data: run},
		}}
		synctest.Wait()
		frame.Frame()
		if len(activity.Activity.Children) != 0 || len(overview.Children) != 0 {
			t.Fatal("child completion did not refresh the mounted peer conversation")
		}
		for range 3 {
			show = false
			frame.Frame()
			if len(remote.overviews) != 0 || len(remote.conversationCache) != 0 {
				t.Fatal("unmount retained the conversation projection")
			}
			show = true
			frame.Frame()
			if len(remote.overviews) != 1 || len(activity.Activity.Children) != 0 {
				t.Fatal("remount did not show the current child state")
			}
		}
	})
}

func TestOpenLiveChildRejectsUnrelatedScope(t *testing.T) {
	for _, name := range []string{"wrong Node", "wrong parent", "wrong worktree", "unregistered worktree", "wrong project", "wrong active turn", "forked child", "forged origin", "inventory-only forged origin", "wrong history revision", "rewound parent", "forged receipt"} {
		t.Run(name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				inventory, history, older := offTailLiveChildFixture()
				child := &inventory.Sessions[1]
				wantTurnReads, wantSessionReads := 0, 0
				switch name {
				case "wrong parent":
					(**child.Delegation).Session = protocol.SessionID{8}
				case "wrong worktree":
					child.Worktree = protocol.WorktreeID{8}
				case "unregistered worktree":
					inventory.Worktrees = nil
				case "wrong project":
					child.Project = new(protocol.ProjectID{8})
				case "wrong active turn":
					child.Activity.Run.Turn = protocol.TurnID{8}
				case "forked child":
					child.Fork = &protocol.ConversationFork{Session: history.Page.Session, Through: older.Run.Turn}
				case "forged origin", "inventory-only forged origin":
					if name == "inventory-only forged origin" {
						history.Page.Background = nil
					}
					(**child.Delegation).Index++
					wantTurnReads = 1
				case "wrong history revision":
					older.Revision++
					wantTurnReads = 1
				case "rewound parent":
					wantTurnReads = 1
				case "forged receipt":
					wantTurnReads, wantSessionReads = 1, 1
				}
				r, source, cv := childTestRuntime(t, inventory, history)
				source.handle = func(request protocol.Request) (link.Response, bool) {
					switch request.Command.Kind {
					case "read_turn":
						if name == "rewound parent" {
							return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "turn is not in this conversation"}}, true
						}
						return link.Response{Output: protocol.Output{Kind: "turn_history", Data: older}}, true
					case "read_session":
						other := *child
						origin := **other.Delegation
						origin.Index++
						other.Delegation = new(&origin)
						return link.Response{Output: protocol.Output{Kind: "session", Data: other}}, true
					default:
						return link.Response{}, false
					}
				}
				if name == "wrong Node" {
					cv.Node = protocol.NodeID{8}.String()
				}
				r.openLiveChild(cv, child.ID.String())
				synctest.Wait()
				childTestFrame(r, nil).Frame()
				turnReads, sessionReads := 0, 0
				for _, request := range source.requests {
					switch request.Command.Kind {
					case "read_turn":
						turnReads++
					case "read_session":
						sessionReads++
					default:
						t.Fatalf("unexpected request: %+v", request)
					}
				}
				if r.App.PanelOpen || len(r.App.Panel.Tabs) != 0 || turnReads != wantTurnReads || sessionReads != wantSessionReads {
					t.Fatalf("unrelated target admitted or over-read: open=%v read_turn=%d read_session=%d", r.App.PanelOpen, turnReads, sessionReads)
				}
			})
		})
	}
}
