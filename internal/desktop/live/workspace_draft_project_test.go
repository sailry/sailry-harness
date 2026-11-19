package live

import (
	"slices"
	"sync"
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A project chosen in a new draft's location menu sends no command itself; the
// toasts that follow come from the folder and Git reads the draft's workspace
// then starts. They name what was read and why it failed, once.
func TestDraftProjectSelectionReadFailure(t *testing.T) {
	cases := []struct {
		name      string
		directory *protocol.Fault
		git       *protocol.Fault
		linked    bool
		want      []string
	}{
		{
			name: "Git that cannot run reports Git once and keeps the folder",
			git:  &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "Git executable not found on execution Node"},
			want: []string{"git_action_failed"},
		},
		{
			name: "a Node that does not tell a missing Git apart still names Git",
			git:  &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Git executable unavailable on execution Node"},
			want: []string{"git_action_failed"},
		},
		{
			name:   "the linked checkout and project location report the same Git failure once",
			git:    &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "Git executable not found on execution Node"},
			linked: true,
			want:   []string{"git_action_failed"},
		},
		{
			name:      "a missing project folder is named as such",
			directory: &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "path not found"},
			want:      []string{"directory_missing"},
		},
		{
			name:      "a folder without access and a Git without access are both reported",
			directory: &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "permission denied"},
			git:       &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "permission denied"},
			want:      []string{"directory_denied", "git_index_denied"},
		},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				testDraftProjectSelectionReadFailure(t, test.directory, test.git, test.linked, test.want)
			})
		})
	}
}

func testDraftProjectSelectionReadFailure(t *testing.T, directory, git *protocol.Fault, linked bool, want []string) {
	fixture := newPluginLocationTestFixture(t)
	root := fixture.root
	root.busy, root.uncertain = map[string]bool{}, map[string]protocol.Request{}
	root.App.Data.Projects = Sidebar(root.snapshot(), root.Preferences.Data).Projects
	comp := root.App.Composer("draft/first")
	comp.Draft, comp.Permission = "Keep this input", composer.ProjectAccess
	var mu sync.Mutex
	var sent []protocol.Request
	fixture.source.dispatch = func(request protocol.Request) (link.Admission, error) {
		mu.Lock()
		sent = append(sent, request)
		mu.Unlock()
		response := link.Response{}
		switch request.Command.Kind {
		case "list_directory":
			response.Fault = directory
			if directory == nil {
				response.Output = protocol.Output{Kind: "directory", Data: protocol.Directory{Entries: []protocol.FileEntry{{Name: "notes.txt"}}}}
			}
		case "inspect_git", "list_worktrees":
			response.Fault = git
			if git == nil && request.Command.Kind == "inspect_git" {
				response.Output = protocol.Output{Kind: "git_status", Data: protocol.GitStatus{Kind: protocol.RepositoryKindDirectory}}
			} else if git == nil {
				response.Output = protocol.Output{Kind: "git_worktrees", Data: protocol.GitWorktrees{Kind: protocol.RepositoryKindDirectory}}
			}
		default:
			response.Fault = &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "unexpected fixture command"}
		}
		completion := make(chan link.Response, 1)
		completion <- response
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
	}

	body, err := fixture.call(t, "sailry/ui.selectLocation", pluginLocationTestArguments(t, "1", map[string]any{"project": fixture.main.Project}))
	if err != nil || string(body) != "true" {
		t.Fatalf("project selection was refused: %s, %v", body, err)
	}
	if comp.Project != fixture.main.Project.String() || comp.Worktree != fixture.main.Path {
		t.Fatalf("selection did not place the draft in the project's main worktree: %q %q", comp.Project, comp.Worktree)
	}
	mu.Lock()
	selected := len(sent)
	mu.Unlock()
	if selected != 0 {
		t.Fatal("choosing a project sent a command of its own")
	}
	selectedTree := fixture.main
	if linked {
		selectedTree = fixture.target
		root.workspace.admitted[selectedTree.ID] = selectedTree
		comp.Worktree = selectedTree.Path
	}

	root.App.Sidebar.Selected = "draft/first"
	window := closeTestWindow(root)
	settle := func() {
		for range 3 {
			root.workspaceFrame(nil)
			synctest.Wait()
			window.Frame()
		}
	}
	settle()
	project := root.selectedWorkspace()
	if project == nil || project.owner.ID != selectedTree.ID {
		t.Fatal("the draft did not show its selected worktree")
	}
	// A watched repository change reads Git again; a failure already told is
	// not told again.
	project.readGit()
	settle()

	mu.Lock()
	requests := slices.Clone(sent)
	mu.Unlock()
	kinds := []string{}
	for _, request := range requests {
		if request.Target != root.controller.Target() {
			t.Fatalf("a workspace read targeted another Node: %+v", request)
		}
		var target, expected protocol.WorktreeID
		switch payload := request.Command.Data.(type) {
		case protocol.CommandListDirectoryPayload:
			target, expected = payload.Worktree, selectedTree.ID
		case protocol.CommandInspectGitPayload:
			target, expected = payload.Worktree, selectedTree.ID
		case protocol.CommandListWorktreesPayload:
			target, expected = payload.Worktree, fixture.main.ID
		default:
			t.Fatalf("selecting a project sent %q; only workspace reads are expected", request.Command.Kind)
		}
		if target != expected {
			t.Fatalf("%q targeted worktree %s, want %s", request.Command.Kind, target, expected)
		}
		kinds = append(kinds, request.Command.Kind)
	}
	if !slices.Contains(kinds, "list_directory") || !slices.Contains(kinds, "inspect_git") || !slices.Contains(kinds, "list_worktrees") {
		t.Fatalf("the draft did not read its project's folder and repository: %v", kinds)
	}
	texts := window.Texts()
	if slices.Contains(texts, i18n.In("en", "workspace_action_failed")) {
		t.Fatalf("an automatic read was reported as a failed workspace action: %q", texts)
	}
	for _, name := range want {
		if count := countText(texts, i18n.In("en", name)); count != 1 {
			t.Fatalf("%s was shown %d times: %q", name, count, texts)
		}
	}
	if comp.Draft != "Keep this input" || comp.Permission != composer.ProjectAccess || comp.Project != fixture.main.Project.String() {
		t.Fatal("a failed workspace read changed the draft, its permission or its project")
	}
	context := &composer.Context{}
	root.projectWorkspaceComposer(root.App.Data.Find("draft/first"), comp, context)
	if git != nil && context.Repository.Kind != composer.Unavailable || git == nil && context.Repository.Kind != composer.Directory {
		t.Fatalf("the composer showed repository state %v", context.Repository.Kind)
	}

	// An explicit refresh forgets what was told, so a failure that persists is
	// told again.
	project.refresh()
	if len(project.readReported) != 0 {
		t.Fatal("refresh kept the failures already told")
	}
	settle()
	reported := []string{}
	for _, name := range project.readReported {
		reported = append(reported, name)
	}
	for _, name := range want {
		if !slices.Contains(reported, name) {
			t.Fatalf("refresh did not report the persisting %s: %v", name, reported)
		}
	}
}

func countText(texts []string, text string) int {
	count := 0
	for _, value := range texts {
		if value == text {
			count++
		}
	}
	return count
}
