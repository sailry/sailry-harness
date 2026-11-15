package shell

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/i18n"
)

// On a project page the title's project opens a menu of the projects;
// choosing one shows the same page of that project.
func TestProjectPageSwitchesProject(t *testing.T) {
	a, tt := newTester(t)
	a.Navigate(FilesPage)
	settle(tt)
	from := a.Data.ProjectOf(a.Sidebar.Selected)
	if from == nil {
		t.Fatal("the selected session has no project")
	}
	var to = from
	for _, pr := range a.Data.Projects {
		if pr != from && latestSession(pr) != nil {
			to = pr
			break
		}
	}
	if to == from {
		t.Fatal("no other project with a session")
	}
	b, ok := tt.Access(i18n.In("en", "projects"))
	if !ok {
		t.Fatalf("no project switcher: %q", tt.Texts())
	}
	tt.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
	tt.Frame()
	snapshot(t, tt, "project-switch")
	clickItem(t, tt, to.Name)
	settle(tt)
	if a.Page != FilesPage || a.Data.ProjectOf(a.Sidebar.Selected) != to || !tt.HasText(to.Name) {
		t.Fatalf("page %d, project %v", a.Page, a.Data.ProjectOf(a.Sidebar.Selected))
	}
}
