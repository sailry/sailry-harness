package desktop_test

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	files "github.com/sailry/sailry-harness/modules/files/desktop"
)

func TestFilesPageUsesScopedWorkspaceAndRetainsDraft(t *testing.T) {
	const path = "scoped-note.txt"
	target := shell.ResourceTarget{Node: "remote", Project: "project", Worktree: "linked-worktree"}
	data := &viewmodel.Data{
		Projects:  []*viewmodel.Project{{ID: target.Project, Host: target.Node, Name: "Scoped project"}},
		Workspace: &viewmodel.Workspace{},
	}
	workspace := &viewmodel.Workspace{Files: &viewmodel.FileNode{Name: "Worktree", Children: []*viewmodel.FileNode{{Name: path, Path: path}}}}
	project := panel.NewLiveProject(target.Worktree, workspace, nil)
	var read string
	project.Actions = &panel.Actions{ReadFile: func(path string) {
		read = path
		if !project.FileLoaded(path) {
			project.SetFile(path, "Saved text\n")
			project.SetFileWritable(path, true)
		}
	}}
	app := shell.NewLive(data, files.Page())
	app.Hooks.ResourceWorkspace = func(got shell.ResourceTarget) *panel.Project {
		if got == target {
			return project
		}
		return nil
	}
	app.Navigate(shell.FilesPage)
	app.SetResourceTarget(target)
	tester := ui.NewTesterWith(app.View, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("en")
	tester.Frame()
	tester.Advance(time.Second)
	tester.Frame()
	row, ok := tester.AccessByRole(ui.RoleTreeItem, path)
	if !ok {
		t.Fatalf("the selected worktree's file is missing: %q", tester.Texts())
	}
	tester.ClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
	tester.Frame()
	tester.Frame()
	if read != path || project.Files.File() != path || !project.FileLoaded(path) {
		t.Fatalf("opening a file selected %q and read %q", project.Files.File(), read)
	}
	editor, ok := tester.Access("Code editor")
	if !ok {
		t.Fatalf("the file editor is missing: %q", tester.Texts())
	}
	tester.ClickAt(editor.Bounds.X+120, editor.Bounds.Y+8)
	tester.Frame()
	tester.Frame()
	tester.Type("X")
	tester.Frame()
	tester.Frame()
	before, dirty := project.FileDraft(path)
	if !dirty || before == "Saved text\n" {
		t.Fatalf("typing left the draft %q, dirty = %v", before, dirty)
	}
	app.Navigate(shell.ConversationPage)
	tester.Frame()
	app.Navigate(shell.FilesPage)
	tester.Frame()
	if draft, dirty := project.FileDraft(path); !dirty || draft != before || !slices.Equal(project.Unsaved(), []string{path}) {
		t.Fatalf("draft after reopening Files = %q, dirty = %v, unsaved = %v", draft, dirty, project.Unsaved())
	}
}
