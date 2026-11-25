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
	filesPage "github.com/sailry/sailry-harness/modules/files/desktop"
)

func TestWorkspaceGitReadsFollowAssembly(t *testing.T) {
	filesPanel := shell.Assemble(filesPage.Register)
	filesPanel.WorkspacePages = nil
	git := shell.Assemble(testassembly.Register)
	gitPanel := git
	gitPanel.WorkspacePages = nil
	filesPageOnly, gitPageOnly := filesPage.Page(), git.WorkspacePages[0]
	filesNamed, gitNamed := filesPageOnly, gitPageOnly
	filesPageOnly.Page, gitPageOnly.Page = "fixture-file-resource", "fixture-repository-resource"
	filesNamed.Resource, gitNamed.Resource = "", ""
	for _, test := range []struct {
		name      string
		assembly  shell.Assembly
		files     bool
		git       bool
		worktrees bool
	}{
		{name: "empty"},
		{name: "page names without resources", assembly: shell.Assembly{WorkspacePages: []shell.NativePage{filesNamed, gitNamed}}},
		{name: "Harness", assembly: harness.Harness(), files: true},
		{name: "Files page only", assembly: shell.Assembly{WorkspacePages: []shell.NativePage{filesPageOnly}}, files: true},
		{name: "Files panel only", assembly: filesPanel, files: true},
		{name: "Git only", assembly: git, git: true},
		{name: "Git page only", assembly: shell.Assembly{WorkspacePages: []shell.NativePage{gitPageOnly}}, git: true},
		{name: "Git panel only", assembly: gitPanel, git: true},
		{name: "Code contributions", assembly: testassembly.Full(), files: true, git: true, worktrees: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			project := protocol.DefaultProject()
			project.ID, project.Name, project.Path = protocol.ProjectID{1}, "Workspace assembly", "/fixture"
			tree := protocol.Worktree{ID: protocol.WorktreeID{1}, Project: &project.ID, Path: project.Path, Main: true}
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node, snapshot.Projects, snapshot.Worktrees = protocol.NodeID{1}, []protocol.Project{project}, []protocol.Worktree{tree}
			controller, source := optionalDesktopController(t, snapshot)
			const filename, body = "file.txt", "explicit workspace file\n"
			source.respond = func(request protocol.Request) link.Response {
				switch request.Command.Kind {
				case "inspect_git":
					return link.Response{Output: protocol.Output{Kind: "git_status", Data: protocol.GitStatus{Kind: protocol.RepositoryKindDirectory}}}
				case "list_worktrees":
					return link.Response{Output: protocol.Output{Kind: "git_worktrees", Data: protocol.GitWorktrees{Kind: protocol.RepositoryKindDirectory}}}
				case "list_directory":
					return link.Response{Output: protocol.Output{Kind: "directory", Data: protocol.DefaultDirectory()}}
				case "read_file":
					return link.Response{Output: protocol.Output{Kind: "file_content", Data: protocol.FileContent{Path: filename, Text: body}}}
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
			window := closeTestWindow(root)
			settle := func() {
				root.workers.Wait()
				window.Frame()
			}
			previous := map[string]int{}
			checkReads := func(phase string) {
				t.Helper()
				for kind, expected := range map[string]bool{"list_directory": test.files, "inspect_git": test.git} {
					count := len(source.sent(kind))
					if sent := count - previous[kind]; (sent > 0) != expected {
						t.Errorf("%s: %s sent %d times, registered = %v", phase, kind, sent, expected)
					}
					previous[kind] = count
				}
			}
			root.projectWorkspaceLocations()
			workspace.resourceFrame()
			settle()
			checkReads("initial frame")
			if got := len(source.sent("list_worktrees")); (got > 0) != test.worktrees {
				t.Errorf("list_worktrees sent %d times, enabled = %v", got, test.worktrees)
			}
			if loaded := workspace.directories[""]; loaded != test.files {
				t.Errorf("root directory loaded = %v, Files registered = %v", loaded, test.files)
			}
			workspace.refresh()
			settle()
			checkReads("explicit refresh")
			workspace.pendingFiles, workspace.pendingGit = true, true
			workspace.gitLastRead = time.Now().Add(-time.Second)
			workspace.resourceFrame()
			settle()
			checkReads("pending invalidation")
			for range 3 {
				workspace.resourceFrame()
				if _, found := controller.Workspace(tree.ID, false); found != test.files {
					t.Fatalf("directory observation = %v, Files registered = %v", found, test.files)
				}
				if _, found := controller.Workspace(tree.ID, true); found != test.git {
					t.Fatalf("repository observation = %v, Git registered = %v", found, test.git)
				}
				workspace.pendingFiles, workspace.pendingGit = true, true
				workspace.releaseView()
				settle()
				if workspace.pendingFiles || workspace.pendingGit || workspace.gitRefreshTimer != nil {
					t.Fatal("pending resource refresh survived close")
				}
				for _, repository := range []bool{false, true} {
					if _, found := controller.Workspace(tree.ID, repository); found {
						t.Fatalf("workspace observation survived close: repository = %v", repository)
					}
				}
			}
			if !test.files {
				workspace.openWorkspaceFile(filename, 0)
				settle()
				if got := len(source.sent("read_file")); got != 1 || workspace.files[filename].Text != body {
					t.Fatalf("explicit file open without Files: read_file sent %d times, content %q", got, workspace.files[filename].Text)
				}
				workspace.refresh()
				settle()
				if got := len(source.sent("read_file")); got != 2 || len(source.sent("list_directory")) != 0 {
					t.Fatalf("refresh without Files must reread the open file without listing the directory: read_file sent %d times, list_directory sent %d times", got, len(source.sent("list_directory")))
				}
				if _, found := controller.Workspace(tree.ID, false); found {
					t.Fatal("the explicit file read started a directory observation")
				}
			}
		})
	}
}
