package live

import (
	"path/filepath"
	"testing"
	"time"

	harness "github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWorkspaceGitReadsFollowAssembly(t *testing.T) {
	for _, test := range []struct {
		name      string
		assembly  shell.Assembly
		git       bool
		worktrees bool
	}{
		{name: "empty"},
		{name: "Harness", assembly: harness.Harness()},
		{name: "Git only", assembly: shell.Assemble(testassembly.Register), git: true},
		{name: "Code contributions", assembly: testassembly.Full(), git: true, worktrees: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			project := protocol.DefaultProject()
			project.ID, project.Name, project.Path = protocol.ProjectID{1}, "Workspace assembly", "/fixture"
			tree := protocol.Worktree{ID: protocol.WorktreeID{1}, Project: &project.ID, Path: project.Path, Main: true}
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node, snapshot.Projects, snapshot.Worktrees = protocol.NodeID{1}, []protocol.Project{project}, []protocol.Worktree{tree}
			controller, source := optionalDesktopController(t, snapshot)
			source.respond = func(request protocol.Request) link.Response {
				switch request.Command.Kind {
				case "inspect_git":
					return link.Response{Output: protocol.Output{Kind: "git_status", Data: protocol.GitStatus{Kind: protocol.RepositoryKindDirectory}}}
				case "list_worktrees":
					return link.Response{Output: protocol.Output{Kind: "git_worktrees", Data: protocol.GitWorktrees{Kind: protocol.RepositoryKindDirectory}}}
				case "list_directory":
					return link.Response{Output: protocol.Output{Kind: "directory", Data: protocol.DefaultDirectory()}}
				default:
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture command is unavailable"}}
				}
			}
			root, err := NewWithAssembly(t.Context(), controller, preferences.Open(filepath.Join(t.TempDir(), "preferences.json")), test.assembly)
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = root.Close() })
			workspace := root.workspaceFor(tree)
			root.projectWorkspaceLocations()
			workspace.resourceFrame()
			root.workers.Wait()
			window := closeTestWindow(root)
			window.Frame()
			workspace.readGit()
			workspace.pendingGit = true
			workspace.gitLastRead = workspace.gitLastRead.Add(-time.Second)
			workspace.resourceFrame()
			root.workers.Wait()
			window.Frame()
			for kind, expected := range map[string]bool{"inspect_git": test.git, "list_worktrees": test.worktrees} {
				if got := len(source.sent(kind)); (got > 0) != expected {
					t.Errorf("%s sent %d times, enabled = %v", kind, got, expected)
				}
			}
			if len(source.sent("list_directory")) == 0 || !workspace.directories[""] {
				t.Fatal("an absent Git module prevented ordinary workspace reads")
			}
			for range 3 {
				workspace.resourceFrame()
				if _, found := controller.Workspace(tree.ID, false); !found {
					t.Fatal("ordinary file observation was not retained")
				}
				if _, found := controller.Workspace(tree.ID, true); found != test.git {
					t.Fatalf("repository observation = %v, enabled = %v", found, test.git)
				}
				workspace.releaseView()
				for _, repository := range []bool{false, true} {
					if _, found := controller.Workspace(tree.ID, repository); found {
						t.Fatalf("workspace observation survived close: repository = %v", repository)
					}
				}
			}
		})
	}
}
