package live

import (
	"context"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	git "github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	files "github.com/sailry/sailry-harness/modules/files/desktop"
	market "github.com/sailry/sailry-harness/modules/market/desktop"
)

type splitCheckpointTransport struct {
	conversationTestTransport
	streams map[protocol.SessionID]*conversationTestSubscription
}

func (s *splitCheckpointTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind == "conversation" {
		return s.streams[topic.Data.(protocol.SessionID)], nil
	}
	if topic.Kind == "workspace" {
		return &conversationTestSubscription{updates: make(chan protocol.Update), closed: make(chan struct{})}, nil
	}
	return s.conversationTestTransport.Subscribe(ctx, topic)
}

func newSplitCheckpointTransport(node protocol.NodeID, tree protocol.Worktree, sessions ...protocol.Session) *splitCheckpointTransport {
	s := &splitCheckpointTransport{streams: map[protocol.SessionID]*conversationTestSubscription{}}
	s.snapshot = protocol.DefaultSnapshot()
	s.snapshot.Node, s.snapshot.Worktrees, s.snapshot.Sessions = node, []protocol.Worktree{tree}, sessions
	for _, session := range sessions {
		stream := &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})}
		history := protocol.DefaultConversationSnapshot()
		history.Node, history.Page.Session, history.Sequence, history.Page.Revision = node, session.ID, 1, 1
		run := splitCheckpointRun(session)
		if session.ID == (protocol.SessionID{3}) {
			run.Status = protocol.ConversationStatusCompleted
			run.FinishedMs = new(int64(42))
		}
		history.Page.Runs = []protocol.ConversationRun{run}
		history.Page.Entries = []protocol.ConversationEntry{{ID: "reply", Sequence: 1, Turn: run.Turn, Author: "assistant",
			Parts: []protocol.ConversationPart{{Kind: "text", Data: session.Activity.Title + " reply"}}}}
		stream.updates <- protocol.Update{Kind: "conversation_snapshot", Data: history}
		s.streams[session.ID] = stream
	}
	s.handle = func(request protocol.Request) (link.Response, bool) {
		var output protocol.Output
		switch query := request.Command.Data.(type) {
		case protocol.CommandListFileCheckpointsPayload:
			output = protocol.Output{Kind: "file_checkpoints", Data: protocol.ConversationCheckpointPage{Session: query.Session, Turn: query.Turn,
				Files: []protocol.ConversationCheckpointFile{{ID: protocol.CheckpointID{1}, Worktree: tree.ID, Path: splitCheckpointPath(query.Session)}}}}
		case protocol.CommandReadTurnDiffPayload:
			output = protocol.Output{Kind: "turn_diff", Data: protocol.ConversationCheckpointTurnDiff{Session: query.Session, Turn: query.Turn,
				Files: []protocol.GitDiff{{Path: splitCheckpointPath(query.Session), Additions: 3, Deletions: 1}}}}
		case protocol.CommandListArtifactsPayload:
			output = protocol.Output{Kind: "artifacts", Data: protocol.DefaultArtifactPage()}
		case protocol.CommandInspectGitPayload:
			output = protocol.Output{Kind: "git_status", Data: protocol.GitStatus{Kind: protocol.RepositoryKindDirectory}}
		case protocol.CommandListDirectoryPayload:
			output = protocol.Output{Kind: "directory", Data: protocol.Directory{}}
		default:
			return link.Response{}, false
		}
		return link.Response{Output: output}, true
	}
	return s
}

func splitCheckpointRun(session protocol.Session) protocol.ConversationRun {
	return protocol.ConversationRun{Session: session.ID, Worktree: session.Worktree, Turn: protocol.TurnID{1},
		Kind: protocol.ConversationRunKindTask, Sequence: 1, Revision: 1, Status: protocol.ConversationStatusRunning}
}

func splitCheckpointPath(session protocol.SessionID) string {
	if session == (protocol.SessionID{1}) {
		return "focused.go"
	}
	return "sibling.go"
}

func TestVisibleSplitShowsFinishedTurnDiffWithoutFocus(t *testing.T) {
	for _, test := range []struct {
		name          string
		separateHosts bool
		omitResources bool
	}{
		{"default/shared-worktree", false, false},
		{"default/separate-hosts", true, false},
		{"without-files-and-git/shared-worktree", false, true},
		{"without-files-and-git/separate-hosts", true, true},
	} {
		t.Run(test.name, func(t *testing.T) {
			separateHosts := test.separateHosts
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			defer cancel()
			tree := protocol.Worktree{ID: protocol.WorktreeID{1}, Path: "/workspace", Main: true}
			sessions := selectionSnapshot("Focused", "Sibling", "Hidden").Sessions
			for i := range sessions {
				sessions[i].Worktree = tree.ID
			}
			first, second, hidden := sessions[0], sessions[1], sessions[2]
			local := newSplitCheckpointTransport(protocol.NodeID{1}, tree, sessions...)
			other := local
			if separateHosts {
				local = newSplitCheckpointTransport(protocol.NodeID{1}, tree, first, hidden)
				other = newSplitCheckpointTransport(protocol.NodeID{2}, tree, second)
			}
			start := func(source *splitCheckpointTransport) *client.Client {
				t.Helper()
				controller := client.New(source)
				t.Cleanup(func() { _ = controller.Close() })
				if err := controller.Start(ctx); err != nil {
					t.Fatal(err)
				}
				return controller
			}
			prefs := preferences.Open(t.TempDir() + "/preferences.json")
			prefs.Data.WelcomeSeen = new(true)
			assembly := shell.Assemble(market.Register)
			if !test.omitResources {
				assembly = shell.Assemble(files.Register, git.Register, market.Register)
			}
			root, err := NewWithAssembly(ctx, start(local), prefs, assembly)
			if err != nil {
				t.Fatal(err)
			}
			defer root.Close()
			owner := root
			if separateHosts {
				owner = root.newBinding(start(other))
				root.bindings = map[string]*Runtime{other.Target().String(): owner}
				root.project()
			}
			app := root.App
			for _, page := range []shell.Page{shell.FilesPage, shell.GitPage} {
				if app.NativePageEnabled(page) != !test.omitResources {
					t.Fatalf("unexpected availability of %q", page)
				}
			}
			layout := app.Prefs()
			layout.Panes = []*panes.Node{selectionSplit(panes.Target{Kind: panes.Session, ID: first.ID.String()}, panes.Target{Kind: panes.Session, ID: second.ID.String()})}
			app.SetPrefs(layout)
			root.OpenSession(first.ID.String())
			window := ui.NewTesterWith(app.View, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
			window.SetLocale("en")
			wakes := make(chan struct{}, 1)
			root.SetInvalidate(func() {
				select {
				case wakes <- struct{}{}:
				default:
				}
			})
			until := func(label string, ready func() bool) {
				t.Helper()
				for {
					window.Frame()
					if ready() {
						return
					}
					select {
					case <-wakes:
					case <-ctx.Done():
						t.Fatalf("waiting for %s (focus %s): %q", label, app.FocusedConversation(), window.Texts())
					}
				}
			}
			until("both running transcripts", func() bool { return window.HasText("Focused reply") && window.HasText("Sibling reply") })
			if app.FocusedConversation() != first.ID.String() || !root.conversationSeen[first.ID] || !owner.conversationSeen[second.ID] || root.conversationSeen[hidden.ID] {
				t.Fatal("fixture did not mount the focused pane and its visible sibling")
			}
			for _, session := range []protocol.Session{first, second} {
				node, want := local.Target(), root
				if session.ID == second.ID {
					node, want = other.Target(), owner
				}
				key := pluginScopeKey{node: node, surface: "composer", session: session.ID.String(), worktree: session.Worktree.String()}
				if registry := root.pluginViews.registries[key]; registry == nil || registry.owner != want {
					t.Fatalf("missing composer plugin context for %s on %s", session.ID, node)
				}
			}
			focusedPanel := root.selectedWorkspace()
			path := splitCheckpointPath(second.ID)
			if _, seen := window.AccessByRole(ui.RoleButton, path); seen {
				t.Fatal("diff card is visible while the turn is running")
			}
			run := splitCheckpointRun(second)
			run.Status = protocol.ConversationStatusCompleted
			run.FinishedMs = new(int64(42))
			other.streams[second.ID].updates <- protocol.Update{Kind: "conversation_frame", Data: protocol.ConversationFrame{
				Node: other.Target(), Session: second.ID, Sequence: 2, Change: protocol.ConversationChange{Kind: "run", Data: run},
			}}
			until("the sibling's completed run", func() bool {
				state, ok := owner.controller.Conversation(second.ID)
				return ok && state.Connected && len(state.Snapshot.Page.Runs) == 1 && state.Snapshot.Page.Runs[0].Status == protocol.ConversationStatusCompleted
			})
			until("the sibling's diff card", func() bool {
				entry, shown := window.AccessByRole(ui.RoleButton, path)
				return shown && entry.Bounds.W > 0 && entry.Bounds.H > 0
			})
			if app.FocusedConversation() != first.ID.String() || focusedPanel == nil || app.Panel.Data != focusedPanel.data || len(focusedPanel.data.Checkpoints) != 0 {
				t.Fatal("finishing the sibling's turn changed the focused workspace panel")
			}
			if focused := app.Data.Find(first.ID.String()); len(focused.Turns) != 1 || focused.Turns[0].Reply.Changes != nil {
				t.Fatal("the sibling's diff leaked into the focused turn with the same ID")
			}
			assertReads := func(source *splitCheckpointTransport, want int) {
				t.Helper()
				source.mu.Lock()
				defer source.mu.Unlock()
				list, diff := 0, 0
				for _, request := range source.requests {
					switch query := request.Command.Data.(type) {
					case protocol.CommandListFileCheckpointsPayload:
						if query.Session != second.ID || query.Turn != run.Turn {
							t.Fatalf("checkpoint list targeted the wrong session or turn: %+v", query)
						}
						list++
					case protocol.CommandReadTurnDiffPayload:
						if query.Session != second.ID || query.Turn != run.Turn {
							t.Fatalf("turn diff targeted the wrong session or turn: %+v", query)
						}
						diff++
					}
				}
				if list != want || diff != want {
					t.Fatalf("Host %s sent %d checkpoint lists and %d diff reads, want %d of each", source.Target(), list, diff, want)
				}
			}
			assertReads(other, 1)
			if separateHosts {
				assertReads(local, 0)
			}
			root.OpenSession(second.ID.String())
			until("the sibling's focused checkpoint history", func() bool {
				return app.FocusedConversation() == second.ID.String() && len(app.Panel.Data.Checkpoints) == 1
			})
			if got := app.Panel.Data.Checkpoints[0]; got.ID != run.Turn.String() || got.Added != 3 || got.Removed != 1 {
				t.Fatalf("focused checkpoint history = %+v", got)
			}
			if separateHosts && (root.selectedWorkspace() != nil || owner.selectedWorkspace() == nil || app.Panel.Data != owner.selectedWorkspace().data) {
				t.Fatal("switching Hosts left the details panel on the former Host")
			}
			assertReads(other, 1)
			layout.Panes = nil
			app.SetPrefs(layout)
			root.OpenSession(first.ID.String())
			until("the focused pane without its sibling", func() bool {
				return app.FocusedConversation() == first.ID.String() && !owner.conversationSeen[second.ID] && !window.HasText("Sibling reply")
			})
			if owner.workspace.projects[tree.ID].checkpointSessions[second.ID] != nil || app.Panel.Data != focusedPanel.data || len(app.Panel.Data.Checkpoints) != 0 {
				t.Fatal("an unmounted sibling retained its checkpoint history in the window")
			}
		})
	}
}
