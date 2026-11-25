package live

import (
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"
	harness "github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The worktree fork, create and remove entries follow the window's assembly:
// Harness neither shows them nor sends their requests; Code keeps them.
func TestWorktreeEntriesFollowAssembly(t *testing.T) {
	en := func(key string) string { return i18n.In("en", key) }
	worktreeCommands := []string{"read_conversation", "inspect_git", "list_worktrees", "create_managed_worktree", "fork_conversation_at", "move_conversation"}
	for _, test := range []struct {
		name     string
		assembly func() shell.Assembly
		code     bool
	}{
		{"Harness", harness.Harness, false},
		{"Code", testassembly.Full, true},
	} {
		t.Run(test.name, func(t *testing.T) {
			project := protocol.DefaultProject()
			project.ID, project.Name, project.Path = protocol.ProjectID{1}, "Worktree assembly", "/fixture"
			tree := protocol.Worktree{ID: protocol.WorktreeID{1}, Project: &project.ID, Path: project.Path, Main: true}
			session := protocol.Session{ID: protocol.SessionID{2}, Revision: 1, Worktree: tree.ID, Project: &project.ID, Config: protocol.DefaultSessionConfig(), Roles: protocol.DefaultRoleSnapshot()}
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node, snapshot.Projects, snapshot.Worktrees, snapshot.Sessions = protocol.NodeID{1}, []protocol.Project{project}, []protocol.Worktree{tree}, []protocol.Session{session}
			controller, source := optionalDesktopController(t, snapshot)
			root, err := NewWithAssembly(t.Context(), controller, preferences.Open(filepath.Join(t.TempDir(), "preferences.json")), test.assembly())
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = root.Close() })
			root.workers.Wait()
			sent := func() int {
				n := 0
				for _, kind := range worktreeCommands {
					n += len(source.sent(kind))
				}
				return n
			}
			before := sent()
			if root.worktreesAssembled() != test.code || (root.App.Sidebar.Actions.ForkWorktree != nil) != test.code {
				t.Fatalf("worktree actions bound = %v, fork %v; want %v", root.worktreesAssembled(), root.App.Sidebar.Actions.ForkWorktree != nil, test.code)
			}
			cv := root.App.Data.Find(session.ID.String())
			listed := root.App.Data.Projects[0]
			if cv == nil || listed == nil {
				t.Fatal("fixture session is not listed")
			}

			// The project page's More menu.
			more := ui.NewTester(func(view *ui.Context) {
				kit.MenuButton(view.Key("more"), "project-more", func(b ui.Element) {
					kit.IconLook(view, b, icons.Ellipsis, "More", "", 28, false)
				}, func(m *kit.Menu) { root.App.Sidebar.ProjectActions(view, m, listed) })
			}, 480, 360)
			more.SetLocale("en")
			more.Frame()
			if err := more.Click("More"); err != nil {
				t.Fatal(err)
			}
			more.Frame()
			if !more.HasText(en("workspace_copy_path")) {
				t.Fatalf("project menu did not open: %q", more.Texts())
			}
			if more.HasText(en("worktree_create")) != test.code || more.HasText(en("worktree_remove")) != test.code {
				t.Fatalf("project menu worktree items = %q, want shown %v", more.Texts(), test.code)
			}

			// The sidebar session menu's Fork submenu.
			menu := ui.NewTester(func(view *ui.Context) {
				ui.ButtonBase(view.Key("session-menu")).Label("Session menu").Size(160, 28).ContextMenu(func(m *ui.Menu) {
					root.App.Sidebar.SessionMenu(view, m, cv)
				})
			}, 400, 300)
			menu.SetLocale("en")
			menu.Frame()
			if err := menu.RightClick("Session menu"); err != nil {
				t.Fatal(err)
			}
			chosen := menu.ChooseMenuItem(en("session_fork"), en("session_fork_worktree")) == nil
			if chosen != test.code {
				t.Fatalf("Fork in a new worktree offered = %v, want %v", chosen, test.code)
			}
			root.workers.Wait()
			if forked := len(source.sent("read_conversation")) > 0; forked != test.code {
				t.Fatalf("Fork in a new worktree read the conversation = %v, want %v", forked, test.code)
			}

			// The turn's fork menu and the composer's worktree menu.
			comp := root.App.Composer(cv.ID)
			turn := protocol.TurnID{3}.String()
			var ctx composer.Context
			frame := ui.NewTester(func(view *ui.Context) {
				probe := &viewmodel.Conversation{ID: cv.ID, Node: cv.Node, Turns: []conversation.Turn{{ID: turn}, {ID: "unparsed"}}}
				root.projectControls(view, probe, comp)
				for _, row := range probe.Turns {
					if row.Controls.HideWorktree == test.code {
						t.Errorf("turn %s HideWorktree = %v, want %v", row.ID, row.Controls.HideWorktree, !test.code)
					}
				}
				root.projectWorkspaceComposer(cv, comp, &ctx)
				if !test.code {
					x := &conversationActions{r: root, c: view, cv: cv, comp: comp}
					x.Worktree(turn)
					x.CreateWorktree()
				}
			}, 320, 200)
			frame.Frame()
			root.workers.Wait()
			if ctx.NoWorktreeCreate == test.code {
				t.Fatalf("composer NoWorktreeCreate = %v, want %v", ctx.NoWorktreeCreate, !test.code)
			}
			if !test.code {
				if got := sent() - before; got != 0 {
					t.Fatalf("Harness sent %d worktree requests", got)
				}
			}
		})
	}
}
