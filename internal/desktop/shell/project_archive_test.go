package shell

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestProjectArchiveTabs(t *testing.T) {
	active := &viewmodel.Conversation{Node: "local", ID: "active", Title: "Current conversation"}
	archived := &viewmodel.Conversation{Node: "local", ID: "archived", Title: "Archived conversation", Archived: true}
	foreign := &viewmodel.Conversation{Node: "remote", ID: "foreign", Title: "Another host's conversation", Archived: true}
	assistant := &viewmodel.Conversation{Node: "local", ID: "assistant", Title: "Package conversation", Archived: true}
	project := &viewmodel.Project{ID: "project", Host: "local", Name: "Sailry", Path: "/Projects/sailry", Branch: "main",
		Conversations: []*viewmodel.Conversation{active, archived, foreign, assistant}}
	data := &viewmodel.Data{Projects: []*viewmodel.Project{project}, Workspace: &viewmodel.Workspace{
		Hosts: []viewmodel.Host{{ID: "local", Name: "MacBook Pro", Local: true}},
	}}
	a := NewLive(data)
	var opened *viewmodel.Conversation
	a.OpenProject(project, ProjectActions{
		VisibleSession: func(cv *viewmodel.Conversation) bool { return cv != assistant },
		OpenSession: func(cv *viewmodel.Conversation, done func(bool)) {
			opened = cv
			done(true)
		},
	})
	tt := ui.NewTesterWith(a.View, 1280, 820, opaque)
	tt.SetLocale("en")
	tt.Frame()
	settle(tt)
	row := func(title string) (ui.AccessInfo, bool) {
		for _, entry := range tt.AccessAll() {
			if entry.Role == ui.RoleButton && entry.Name == title && entry.Bounds.X >= RailWidth+a.NavWidth {
				return entry, true
			}
		}
		return ui.AccessInfo{}, false
	}
	if _, ok := row(active.Title); !ok {
		t.Fatal("the active tab omitted its conversation")
	}
	if _, ok := row(archived.Title); ok {
		t.Fatal("the active tab included an archived conversation")
	}
	snapshot(t, tt, "project-overview-sessions")
	click(t, tt, i18n.In("en", "project_archived"))
	settle(tt)
	for _, cv := range []*viewmodel.Conversation{active, foreign, assistant} {
		if _, ok := row(cv.Title); ok {
			t.Fatalf("the archive tab included %q", cv.Title)
		}
	}
	entry, ok := row(archived.Title)
	if !ok || !archived.Archived {
		t.Fatal("the archive tab omitted or restored its conversation")
	}
	a.Mode = theme.Dark
	tt.SetDark(true)
	settle(tt)
	snapshot(t, tt, "project-overview-archived")
	// Opening is delegated to the captured controller; switching the tabs and
	// rendering the records do not mutate archive status themselves.
	tt.ClickAt(entry.Bounds.X+entry.Bounds.W/2, entry.Bounds.Y+entry.Bounds.H/2)
	settle(tt)
	if opened != archived || !archived.Archived || a.overview.pending {
		t.Fatal("the archived row did not preserve its captured open action")
	}
}

// The terminals tab lists the Node's running project terminals, including
// those another controller started, and opens one through its action.
func TestProjectTerminalsTab(t *testing.T) {
	session := &viewmodel.Conversation{Node: "local", ID: "active", Title: "Current conversation"}
	project := &viewmodel.Project{ID: "project", Host: "local", Name: "Sailry", Path: "/Projects/sailry",
		Conversations: []*viewmodel.Conversation{session}}
	data := &viewmodel.Data{Projects: []*viewmodel.Project{project}, Workspace: &viewmodel.Workspace{
		Hosts: []viewmodel.Host{{ID: "local", Name: "MacBook Pro", Local: true}},
	}}
	a := NewLive(data)
	terminals := []ProjectTerminal{{ID: "mobile", Title: "zsh", Path: "/Projects/sailry"}}
	var opened string
	a.OpenProject(project, ProjectActions{
		Terminals:    func() []ProjectTerminal { return terminals },
		OpenTerminal: func(id string) { opened = id },
	})
	tt := ui.NewTesterWith(a.View, 1280, 820, opaque)
	tt.SetLocale("en")
	tt.Frame()
	settle(tt)
	row := func(title string) (ui.AccessInfo, bool) {
		for _, entry := range tt.AccessAll() {
			if entry.Role == ui.RoleButton && entry.Name == title && entry.Bounds.X >= RailWidth+a.NavWidth {
				return entry, true
			}
		}
		return ui.AccessInfo{}, false
	}
	if _, ok := row("zsh"); ok {
		t.Fatal("the sessions tab listed a terminal")
	}
	click(t, tt, i18n.In("en", "project_terminals"))
	settle(tt)
	if _, ok := row(session.Title); ok {
		t.Fatal("the terminals tab listed a conversation")
	}
	entry, ok := row("zsh")
	if !ok {
		t.Fatal("the terminals tab omitted the running terminal")
	}
	tt.ClickAt(entry.Bounds.X+entry.Bounds.W/2, entry.Bounds.Y+entry.Bounds.H/2)
	settle(tt)
	if opened != "mobile" {
		t.Fatalf("opened terminal = %q, want mobile", opened)
	}
	terminals = nil
	settle(tt)
	if _, ok := row("zsh"); ok {
		t.Fatal("an ended terminal stayed listed")
	}
}
