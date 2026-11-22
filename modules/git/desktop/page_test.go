package desktop_test

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	files "github.com/sailry/sailry-harness/modules/files/desktop"
	git "github.com/sailry/sailry-harness/modules/git/desktop"
)

func TestGitOpenSourceUsesRegisteredFilesPage(t *testing.T) {
	for _, test := range []struct {
		name      string
		filesPage bool
	}{
		{name: "Git only"},
		{name: "Git and Files", filesPage: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			const path = "changed.txt"
			target := shell.ResourceTarget{Node: "remote", Project: "project", Worktree: "linked-worktree"}
			data := &viewmodel.Data{
				Projects:  []*viewmodel.Project{{ID: target.Project, Host: target.Node, Name: "Scoped project"}},
				Workspace: &viewmodel.Workspace{},
			}
			workspace := &viewmodel.Workspace{
				Files:   &viewmodel.FileNode{Name: "Worktree", Children: []*viewmodel.FileNode{{Name: path, Path: path}}},
				Changes: []viewmodel.FileChange{{Path: path, Status: "M", Unstaged: "M"}},
			}
			var read string
			project := panel.NewLiveProject(target.Worktree, workspace, &panel.Actions{ReadFile: func(path string) { read = path }})
			project.SetGitState(panel.GitState{Known: true, Head: true})
			pages := []shell.NativePage{git.Page()}
			if test.filesPage {
				pages = append(pages, files.Page())
			}
			app := shell.NewLive(data, pages...)
			app.Hooks.ResourceWorkspace = func(got shell.ResourceTarget) *panel.Project {
				if got == target {
					return project
				}
				return nil
			}
			app.Navigate(shell.GitPage)
			app.SetResourceTarget(target)
			tester := ui.NewTesterWith(app.View, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
			tester.SetLocale("en")
			tester.Frame()
			tester.Advance(time.Second)
			tester.Frame()
			row, ok := tester.AccessByRole(ui.RoleTreeItem, path)
			if !ok {
				t.Fatalf("the selected worktree's change is missing: %q", tester.Texts())
			}
			tester.RightClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
			items := tester.Menu()
			openSource := i18n.In("en", "git_open_source")
			if !slices.Contains(items, i18n.In("en", "git_unstaged")) || slices.Contains(items, openSource) != test.filesPage {
				t.Fatalf("change menu with Files installed = %v: %q", test.filesPage, items)
			}
			if !test.filesPage {
				return
			}
			if err := tester.ChooseMenuItem(openSource); err != nil {
				t.Fatal(err)
			}
			tester.Frame()
			if app.Page != shell.FilesPage || app.ResourceTarget() != target || project.Files.File() != path || read != path {
				t.Fatalf("Open source navigated to %q on %+v, selected %q and read %q", app.Page, app.ResourceTarget(), project.Files.File(), read)
			}
		})
	}
}
