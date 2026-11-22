package shell

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestProjectOverviewSelection(t *testing.T) {
	first := &viewmodel.Conversation{Node: "local", ID: "first", Title: "Match first"}
	hidden := &viewmodel.Conversation{Node: "local", ID: "hidden", Title: "Outside search"}
	filtered := &viewmodel.Conversation{Node: "local", ID: "filtered", Title: "Match package session"}
	foreign := &viewmodel.Conversation{Node: "remote", ID: "first", Title: "Match from another host"}
	middle := &viewmodel.Conversation{Node: "local", ID: "middle", Title: "Match middle"}
	last := &viewmodel.Conversation{Node: "local", ID: "last", Title: "Match last"}
	single := &viewmodel.Conversation{Node: "local", ID: "single", Title: "Match other"}
	archivedFirst := &viewmodel.Conversation{Node: "local", ID: "archived-first", Title: "Match archived first", Archived: true}
	archivedLast := &viewmodel.Conversation{Node: "local", ID: "archived-last", Title: "Match archived last", Archived: true}
	project := &viewmodel.Project{ID: "project", Host: "local", Name: "Project", Path: "/project",
		Conversations: []*viewmodel.Conversation{first, hidden, filtered, foreign, middle, last, single, archivedFirst, archivedLast}}
	a := NewLive(&viewmodel.Data{Projects: []*viewmodel.Project{project}, Workspace: &viewmodel.Workspace{
		Hosts: []viewmodel.Host{{ID: "local", Name: "Local", Local: true}},
	}})
	a.NavOpen = false
	var opened []*viewmodel.Conversation
	a.OpenProject(project, ProjectActions{
		VisibleSession: func(cv *viewmodel.Conversation) bool { return cv != filtered },
		OpenSession: func(cv *viewmodel.Conversation, done func(bool)) {
			opened = append(opened, cv)
			done(false)
		},
	})
	var targets []*viewmodel.Conversation
	a.Sidebar.Actions = &sidebar.Actions{
		Archive:   func(cv *viewmodel.Conversation) { targets = append(targets, cv) },
		Unarchive: func(cv *viewmodel.Conversation) { targets = append(targets, cv) },
	}
	tt := ui.NewTesterWith(a.View, 1280, 820, opaque)
	tt.SetLocale("en")
	tt.Frame()
	settle(tt)

	rowClick := func(mods ui.Modifiers, cv *viewmodel.Conversation) {
		t.Helper()
		if err := tt.ClickWith(mods, cv.Title); err != nil {
			t.Fatal(err)
		}
		settle(tt)
	}
	selected := func(cv *viewmodel.Conversation, want bool) {
		t.Helper()
		if row, ok := tt.AccessByRole(ui.RoleButton, cv.Title); !ok || row.Checked != want {
			t.Fatalf("row %q: found=%t selected=%t, want selected=%t", cv.Title, ok, row.Checked, want)
		}
	}
	menuAction := func(row *viewmodel.Conversation, action string, batch bool, want ...*viewmodel.Conversation) {
		t.Helper()
		if err := tt.RightClick(row.Title); err != nil {
			t.Fatal(err)
		}
		menu := tt.Menu()
		if batch && !slices.Equal(menu, []string{action, i18n.In("en", "session_delete")}) {
			t.Fatalf("batch menu for %q = %q", row.Title, menu)
		}
		if !batch && !slices.Contains(menu, i18n.In("en", "workspace_rename")) {
			t.Fatalf("single-session menu for %q = %q", row.Title, menu)
		}
		targets = nil
		if err := tt.ChooseMenuItem(action); err != nil {
			t.Fatal(err)
		}
		settle(tt)
		if !slices.Equal(targets, want) {
			titles := func(sessions []*viewmodel.Conversation) []string {
				out := make([]string, len(sessions))
				for i, cv := range sessions {
					out[i] = cv.Title
				}
				return out
			}
			t.Fatalf("%s on %q targeted %q, want %q", action, row.Title, titles(targets), titles(want))
		}
	}

	rowClick(ui.Ctrl, hidden)
	rowClick(ui.Cmd, first)
	selected(hidden, true)
	selected(first, true)
	rowClick(ui.Ctrl, first)
	selected(first, false)
	rowClick(ui.Cmd, first)

	a.overview.search = "MATCH"
	settle(tt)
	archive := i18n.In("en", "workspace_archive")
	menuAction(first, archive, false, first)
	rowClick(ui.Shift, last)
	selected(first, true)
	selected(middle, true)
	selected(last, true)
	selected(single, false)
	menuAction(last, archive, true, first, middle, last)
	menuAction(single, archive, false, single)
	selected(first, false)
	selected(middle, false)
	selected(last, false)
	selected(single, true)
	if len(opened) != 0 {
		t.Fatalf("selection gestures opened %d sessions", len(opened))
	}
	rowClick(0, single)
	if !slices.Equal(opened, []*viewmodel.Conversation{single}) {
		t.Fatal("a plain click did not open its session")
	}
	selected(single, false)

	rowClick(ui.Cmd, first)
	click(t, tt, i18n.In("en", "project_archived"))
	settle(tt)
	rowClick(ui.Cmd, archivedFirst)
	rowClick(ui.Shift, archivedLast)
	selected(archivedFirst, true)
	selected(archivedLast, true)
	menuAction(archivedFirst, i18n.In("en", "backup_unarchive"), true, archivedFirst, archivedLast)
	if len(opened) != 1 {
		t.Fatalf("archive selection gestures opened %d additional sessions", len(opened)-1)
	}
	click(t, tt, i18n.In("en", "workspace_sessions"))
	settle(tt)
	selected(first, false)
}
