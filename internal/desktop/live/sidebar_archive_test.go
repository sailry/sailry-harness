package live

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestArchiveAvailabilityUsesCurrentNodeTasks(t *testing.T) {
	for _, scenario := range []struct {
		name   string
		change func(*protocol.Snapshot)
		want   bool
	}{
		{name: "idle", want: true},
		{name: "running", change: func(s *protocol.Snapshot) {
			s.Sessions[0].Activity.Run = &protocol.ConversationRun{Status: protocol.ConversationStatusRunning}
		}},
		{name: "stopping", change: func(s *protocol.Snapshot) {
			s.Sessions[0].Activity.Run = &protocol.ConversationRun{Status: protocol.ConversationStatusStopping}
		}},
		{name: "waiting", change: func(s *protocol.Snapshot) {
			s.Sessions[0].Activity.Run = &protocol.ConversationRun{Status: protocol.ConversationStatusRunning}
			s.Sessions[0].Activity.Waiting = new(protocol.ActivityWaitingInput)
		}},
		{name: "queued run", change: func(s *protocol.Snapshot) {
			s.Sessions[0].Activity.Run = &protocol.ConversationRun{Status: protocol.ConversationStatusQueued}
		}},
		{name: "held input after outcome", change: func(s *protocol.Snapshot) {
			s.Sessions[0].Activity.Run = &protocol.ConversationRun{Status: protocol.ConversationStatusCompleted}
			s.Sessions[0].Activity.Queued = 1
		}},
		{name: "background work", change: func(s *protocol.Snapshot) { s.Sessions[0].Activity.Background = 1 }},
		{name: "delegated work", change: func(s *protocol.Snapshot) {
			child := protocol.DefaultSession()
			child.ID = protocol.SessionID{2}
			delegation := &protocol.Delegation{Session: s.Sessions[0].ID}
			child.Delegation = &delegation
			child.Activity.Run = &protocol.ConversationRun{Status: protocol.ConversationStatusRunning}
			s.Sessions = append(s.Sessions, child)
		}},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			snapshot := selectionSnapshot("Session")
			if scenario.change != nil {
				scenario.change(&snapshot)
			}
			r := &Runtime{controller: ownershipClient(t, snapshot)}
			// The row can predate the latest Node observation, including held input.
			row := &viewmodel.Conversation{Node: snapshot.Node.String(), ID: snapshot.Sessions[0].ID.String(), Status: viewmodel.Idle}
			if got := r.canArchiveConversation(row); got != scenario.want {
				t.Fatalf("archive availability = %t, want %t", got, scenario.want)
			}
		})
	}
}

func TestArchiveKeepsTabSplitSessionIdentity(t *testing.T) {
	for _, split := range []bool{false, true} {
		t.Run(map[bool]string{false: "selected tab", true: "background split pane"}[split], func(t *testing.T) {
			snapshot := selectionSnapshot("Project A conversation", "Project B conversation")
			projectA, projectB := protocol.ProjectID{1}, protocol.ProjectID{2}
			snapshot.Projects = []protocol.Project{{ID: projectA, Name: "Project A"}, {ID: projectB, Name: "Project B"}}
			snapshot.Sessions[0].Project, snapshot.Sessions[1].Project = &projectA, &projectB
			first, second := snapshot.Sessions[0].ID.String(), snapshot.Sessions[1].ID.String()
			f := selectionWindow(t, snapshot)
			r, app := f.r, f.r.App
			prefs := app.Prefs()
			targets := []panes.Target{{Kind: panes.Session, ID: first}, {Kind: panes.Session, ID: second}}
			if split {
				prefs.Panes = []*panes.Node{selectionSplit(targets...)}
			} else {
				prefs.Tabs = targets
			}
			app.SetPrefs(prefs)
			selected := first
			if split {
				selected = second
			}
			r.OpenSession(selected)
			f.window.Frame()
			app.Composer(first).Draft, app.Composer(second).Draft = "Input A", "Input B"
			shown := map[string]*composer.State{}
			conversation := app.Hooks.Conversation
			app.Hooks.Conversation = func(c *ui.Context, cv *viewmodel.Conversation, comp *composer.State) (*composer.Context, shell.ConversationActions) {
				shown[cv.ID] = comp
				return conversation(c, cv, comp)
			}
			archived := snapshot.Sessions[0]
			archived.Archived, archived.Revision = true, archived.Revision+1
			f.source.mu.Lock()
			f.source.handle = func(request protocol.Request) (link.Response, bool) {
				if request.Command.Kind == "set_session_archived" {
					return link.Response{Output: protocol.Output{Kind: "session", Data: archived}}, true
				}
				return link.Response{}, false
			}
			f.source.mu.Unlock()
			r.App.Sidebar.Actions.Archive(app.Data.Find(first))
			r.workers.Wait()
			f.window.Frame()
			f.source.mu.Lock()
			var commands []protocol.CommandSetSessionArchivedPayload
			for _, request := range f.source.requests {
				if request.Command.Kind == "set_session_archived" {
					input, _ := detail[protocol.CommandSetSessionArchivedPayload](request.Command.Data)
					commands = append(commands, input)
				}
			}
			f.source.mu.Unlock()
			if !slices.Equal(commands, []protocol.CommandSetSessionArchivedPayload{{Session: archived.ID, ExpectedRevision: 1, Archived: true}}) {
				t.Fatalf("archive used a different tab or pane: %+v", commands)
			}
			f.event(t, "session_changed", archived)
			clear(shown)
			f.window.Frame()
			if app.Sidebar.Selected != selected || app.FocusedConversation() != selected || shown[selected] != app.Composer(selected) {
				t.Fatalf("archive rebound selection or its rendered composer: sidebar %q, focused %q", app.Sidebar.Selected, app.FocusedConversation())
			}
			if app.Data.ProjectOf(first).ID != projectA.String() || app.Data.ProjectOf(second).ID != projectB.String() ||
				!app.Data.Find(first).Archived || app.Data.Find(second).Archived || app.Composer(first).Draft != "Input A" || app.Composer(second).Draft != "Input B" {
				t.Fatal("archive mixed conversation data, drafts or project ownership")
			}
			if split {
				if groups := app.Prefs().Panes; len(groups) != 1 || !slices.Equal(groups[0].Targets(), targets) || len(shown) != 2 {
					t.Fatal("archiving removed or rebound a split pane")
				}
			} else if !slices.Equal(app.Prefs().Tabs, targets) || len(shown) != 1 {
				t.Fatal("archiving removed a kept tab or rendered another conversation")
			}
		})
	}
}
