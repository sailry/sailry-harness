package shell

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

func TestProjectPageSwitchesProject(t *testing.T) {
	for _, test := range []struct {
		name          string
		noChatProject bool
	}{{"project chat", false}, {"unassigned chat", true}} {
		t.Run(test.name, func(t *testing.T) {
			a, tt := newTester(t)
			if test.noChatProject {
				cv := &preview.Conversation{ID: "projectless-chat", Title: "Unassigned conversation"}
				a.Data.Unassigned = append(a.Data.Unassigned, cv)
				a.Sidebar.Selected = cv.ID
			}
			chat := a.Sidebar.Selected
			comp := a.Composer(chat)
			comp.Draft = "Keep this draft"
			chatProject := a.Data.ProjectOf(chat)
			if !test.noChatProject && chatProject == nil {
				t.Fatal("the fixture chat has no project")
			}

			target := &preview.Project{ID: "registered-project", Name: "Project without a chat", Host: "build-host"}
			a.Data.Projects = append(a.Data.Projects, target)
			a.Data.Hosts = append(a.Data.Hosts, preview.Host{ID: target.Host, Name: "Build host"})
			a.Navigate(FilesPage)
			settle(tt)
			if test.noChatProject && !tt.HasText(i18n.In("en", "shell_sessions")) {
				t.Fatalf("the Files breadcrumb has no chat fallback: %q", tt.Texts())
			}

			openMenu := func() {
				t.Helper()
				b, ok := tt.Access(i18n.In("en", "projects"))
				if !ok {
					t.Fatalf("no project switcher: %q", tt.Texts())
				}
				tt.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
				tt.Frame()
			}
			openMenu()
			if !tt.HasText("Build host") {
				t.Fatalf("the project owner was missing from the menu: %q", tt.Texts())
			}
			if !test.noChatProject {
				snapshot(t, tt, "project-switch")
			}
			clickItem(t, tt, target.Name)
			want := ResourceTarget{Node: target.Host, Project: target.ID}
			for _, page := range []Page{FilesPage, GitPage} {
				if a.Page != page {
					a.Navigate(page)
				}
				settle(tt)
				project := a.project()
				if a.Page != page || a.ResourceTarget() != want || project == nil || project.ID != target.ID ||
					!tt.HasText(target.Name) || a.Sidebar.Selected != chat || comp.Draft != "Keep this draft" {
					t.Fatalf("incorrect page or project, or the chat moved: %v, %v, %q", a.Page, a.ResourceTarget(), a.Sidebar.Selected)
				}
			}

			a.Navigate(ConversationPage)
			settle(tt)
			if a.ResourceTarget() != (ResourceTarget{}) || a.Sidebar.Selected != chat || comp.Draft != "Keep this draft" {
				t.Fatal("returning to the conversation changed its selection or draft")
			}

			a.Navigate(FilesPage)
			settle(tt)
			target.Offline = true
			settle(tt)
			if a.ResourceTarget() != want || a.project() != nil {
				t.Fatal("offline project fell through to the chat workspace")
			}
			openMenu()
			clickItem(t, tt, i18n.In("en", "resource_follow_conversation"))
			settle(tt)
			if a.ResourceTarget() != (ResourceTarget{}) || a.ResourceProject() != chatProject || a.Sidebar.Selected != chat {
				t.Fatal("reset did not follow the original conversation")
			}
		})
	}
}
