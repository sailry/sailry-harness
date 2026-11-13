package live

import (
	"fmt"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWorkspaceHunkReads(t *testing.T) {
	for _, changeSession := range []bool{false, true} {
		for _, oldFirst := range []bool{false, true} {
			t.Run(fmt.Sprintf("session change %t old result first %t", changeSession, oldFirst), func(t *testing.T) {
				synctest.Test(t, func(t *testing.T) {
					fixture := newPluginLocationTestFixture(t)
					root := fixture.root
					root.busy, root.uncertain = map[string]bool{}, map[string]protocol.Request{}
					project := root.workspaceFor(fixture.main)
					oldSession, newSession := protocol.SessionID{1}, protocol.SessionID{1}
					if changeSession {
						newSession = protocol.SessionID{2}
					}
					oldTurn, newTurn := protocol.TurnID{1}, protocol.TurnID{2}
					const relative = "same-file.go"
					pending := map[protocol.TurnID]chan link.Response{
						oldTurn: make(chan link.Response, 1), newTurn: make(chan link.Response, 1),
					}
					reads := make(chan protocol.CommandReadTurnHunksPayload, 4)
					fixture.source.dispatch = func(request protocol.Request) (link.Admission, error) {
						query, ok := detail[protocol.CommandReadTurnHunksPayload](request.Command.Data)
						if request.Command.Kind != "read_turn_hunks" || !ok {
							return link.Admission{}, fmt.Errorf("unexpected command %s", request.Command.Kind)
						}
						reads <- query
						return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: pending[query.Turn]}, nil
					}
					// Drain only queued effects; no drawing or process-wide raster
					// workers are needed to check the resource owner.
					window := ui.NewTester(func(c *ui.Context) {
						root.mu.Lock()
						queue := root.queue
						root.queue = nil
						root.mu.Unlock()
						for _, apply := range queue {
							apply(c)
						}
					}, 1, 1)
					settle := func() { synctest.Wait(); window.Frame(); synctest.Wait() }
					project.session = oldSession
					project.reviewTurns[relative] = oldTurn
					project.readTurnHunks(relative)
					settle()
					select {
					case read := <-reads:
						if read.Session != oldSession || read.Turn != oldTurn || read.Path != relative {
							t.Fatalf("initial hunk scope = %+v", read)
						}
					default:
						t.Fatal("initial hunk read was not dispatched")
					}
					project.session = newSession
					project.reviewTurns[relative] = newTurn
					project.readTurnHunks(relative)
					project.readTurnHunks(relative)
					settle()
					select {
					case read := <-reads:
						if read.Session != newSession || read.Turn != newTurn || read.Path != relative {
							t.Fatalf("successor hunk scope = %+v", read)
						}
					default:
						t.Fatal("an older read suppressed the new review of the same file")
					}
					if len(reads) != 0 {
						t.Fatal("the same in-flight review was read twice")
					}
					respond := func(session protocol.SessionID, turn protocol.TurnID) {
						pending[turn] <- link.Response{Output: protocol.Output{Kind: "turn_hunks", Data: protocol.ConversationCheckpointHunkReview{
							Session: session, Turn: turn, Worktree: fixture.main.ID, Path: relative,
							Hunks: []protocol.ConversationCheckpointHunk{{ID: turn.String()}},
						}}}
						settle()
					}
					if oldFirst {
						respond(oldSession, oldTurn)
						if _, exists := project.hunks[relative]; exists {
							t.Fatal("an older result populated the current review while it was loading")
						}
					}
					respond(newSession, newTurn)
					if !oldFirst {
						respond(oldSession, oldTurn)
					}
					got, exists := project.hunks[relative]
					if !exists || got.Session != newSession || got.Turn != newTurn || len(got.Hunks) != 1 || got.Hunks[0].ID != newTurn.String() {
						t.Fatalf("current hunk review = %+v, exists %t", got, exists)
					}
				})
			})
		}
	}
}

func TestWorkspaceTurnReview(t *testing.T) {
	for _, scenario := range []string{"latest turn wins", "ordinary changes retires pending review", "repeated turn keeps latest path"} {
		t.Run(scenario, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				fixture := newPluginLocationTestFixture(t)
				root := fixture.root
				root.busy, root.uncertain = map[string]bool{}, map[string]protocol.Request{}
				session := protocol.Session{ID: protocol.SessionID{1}, Project: fixture.main.Project, Worktree: fixture.main.ID}
				root.admittedSessions = map[string]protocol.Session{session.ID.String(): session}
				cv := &viewmodel.Conversation{ID: session.ID.String(), Node: root.controller.Target().String()}
				root.App.Data.Unassigned = append(root.App.Data.Unassigned, cv)
				project := root.workspaceFor(fixture.main)
				project.session, project.initialized, project.release = session.ID, true, func() {}
				// Opening a tab is real; its file bytes are irrelevant to which
				// captured turn the outstanding navigation may publish.
				project.panel = panel.NewLiveProject(fixture.main.ID.String(), project.data, &panel.Actions{})
				root.workspace.selected = fixture.main.ID
				oldTurn, newTurn := protocol.TurnID{1}, protocol.TurnID{2}
				pending := map[protocol.TurnID]chan link.Response{
					oldTurn: make(chan link.Response, 1), newTurn: make(chan link.Response, 1),
				}
				reads := make(chan protocol.TurnID, 4)
				fixture.source.dispatch = func(request protocol.Request) (link.Admission, error) {
					query, ok := detail[protocol.CommandReadTurnDiffPayload](request.Command.Data)
					if request.Command.Kind != "read_turn_diff" || !ok {
						return link.Admission{}, fmt.Errorf("unexpected command %s", request.Command.Kind)
					}
					reads <- query.Turn
					return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: pending[query.Turn]}, nil
				}
				window := ui.NewTester(func(c *ui.Context) {
					root.mu.Lock()
					queue := root.queue
					root.queue = nil
					root.mu.Unlock()
					for _, apply := range queue {
						apply(c)
					}
				}, 1, 1)
				settle := func() { synctest.Wait(); window.Frame(); synctest.Wait() }
				x := &conversationActions{r: root, cv: cv}
				root.reviewWorkspaceTurn(x, oldTurn.String(), "first.go")
				settle()
				switch scenario {
				case "latest turn wins":
					root.reviewWorkspaceTurn(x, newTurn.String(), "new.go")
				case "ordinary changes retires pending review":
					root.openWorkspaceChange(x, "ordinary.go")
				case "repeated turn keeps latest path":
					root.reviewWorkspaceTurn(x, oldTurn.String(), "second.go")
				}
				settle()
				wantReads := 1
				if scenario == "latest turn wins" {
					wantReads = 2
				}
				if len(reads) != wantReads {
					t.Fatalf("turn diff reads = %d, want %d", len(reads), wantReads)
				}
				respond := func(turn protocol.TurnID, paths ...string) {
					value := protocol.ConversationCheckpointTurnDiff{Session: session.ID, Turn: turn}
					for _, path := range paths {
						value.Files = append(value.Files, protocol.GitDiff{Path: path})
					}
					pending[turn] <- link.Response{Output: protocol.Output{Kind: "turn_diff", Data: value}}
					settle()
				}
				if scenario == "latest turn wins" {
					respond(newTurn, "new.go")
				}
				respond(oldTurn, "first.go", "second.go")
				switch scenario {
				case "latest turn wins":
					if project.turnView == nil || *project.turnView != newTurn || project.panel.Git.Change() != "new.go" || len(project.data.Changes) != 1 || project.data.Changes[0].Path != "new.go" {
						t.Fatalf("old result replaced the latest turn: %v, %s, %+v", project.turnView, project.panel.Git.Change(), project.data.Changes)
					}
				case "ordinary changes retires pending review":
					if project.turnView != nil || project.panel.Git.Change() != "ordinary.go" || len(project.data.Changes) != 0 {
						t.Fatalf("old result reopened a turn after leaving it: %v, %s, %+v", project.turnView, project.panel.Git.Change(), project.data.Changes)
					}
				case "repeated turn keeps latest path":
					if project.turnView == nil || *project.turnView != oldTurn || project.panel.Git.Change() != "second.go" || len(project.data.Changes) != 2 {
						t.Fatalf("deduplicated read lost the latest file: %v, %s, %+v", project.turnView, project.panel.Git.Change(), project.data.Changes)
					}
				}
			})
		})
	}
}
