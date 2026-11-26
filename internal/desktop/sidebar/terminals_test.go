package sidebar

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestMixedProjectRows(t *testing.T) {
	project := &viewmodel.Project{ID: "project", Name: "Project", Conversations: []*viewmodel.Conversation{
		{ID: "d", Title: "Delta", Created: 6, Updated: time.Unix(1, 0)},
		{ID: "f", Title: "Foxtrot", Created: 4, Updated: time.Unix(6, 0)},
		{ID: "c", Title: "Charlie", Created: 2, Updated: time.Unix(2, 0)},
		{ID: "a", Title: "Alpha", Created: 1, Updated: time.Unix(4, 0)},
		{ID: "old", Title: "Archived", Archived: true},
	}}
	last := &viewmodel.Conversation{ID: "last", Title: "Last session"}
	nextProject := &viewmodel.Project{ID: "next", Name: "ZZ Next project", Conversations: []*viewmodel.Conversation{last}}
	s := New(&viewmodel.Data{Projects: []*viewmodel.Project{project, nextProject}}, "")
	var archived []string
	s.Actions = &Actions{
		Terminals: func(p *viewmodel.Project) []Terminal {
			if p != project {
				return nil
			}
			return []Terminal{
				{ID: "e", Title: "alice@host: Echo", Created: 3, Updated: time.Unix(5, 0)},
				{ID: "b", Title: "zed@host: Bravo", Created: 5, Updated: time.Unix(3, 0)},
			}
		},
		Archive: func(cv *viewmodel.Conversation) { archived = append(archived, cv.Title) },
	}
	tt := selectionTester(s)
	more, less := i18n.In("en", "sidebar_sessions_more"), i18n.In("en", "sidebar_sessions_less")
	labels := []string{"Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Archived", more, less}
	checkRows := func(want ...string) {
		t.Helper()
		tt.Frame()
		var got []string
		for _, row := range tt.AccessAll() {
			if row.Role == ui.RoleButton && slices.Contains(labels, row.Name) {
				got = append(got, row.Name)
			}
		}
		if !slices.Equal(got, want) {
			t.Fatalf("project rows under %s = %q, want %q", s.sort().Preference(), got, want)
		}
	}
	for _, scenario := range []struct {
		order Sort
		rows  []string
	}{
		{SortCreated, []string{"Delta", "Bravo", "Foxtrot", "Echo", "Charlie", "Alpha"}},
		{SortUpdated, []string{"Foxtrot", "Echo", "Alpha", "Bravo", "Charlie", "Delta"}},
		{SortName, []string{"Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot"}},
	} {
		*s.Sort = int(scenario.order)
		collapsed := append(slices.Clone(scenario.rows[:Initial]), more)
		checkRows(collapsed...)
		clickSelectionRow(t, tt, more, 0)
		checkRows(append(slices.Clone(scenario.rows), less)...)
		clickSelectionRow(t, tt, less, 0)
		checkRows(collapsed...)
	}

	// The collapsed project's fifth row is a terminal. A range extending
	// to the next project must skip the hidden sixth row, a conversation.
	clickSelectionRow(t, tt, "Alpha", 0)
	clickSelectionRow(t, tt, last.Title, ui.Shift)
	assertSelectedRows(t, tt, "Alpha", "Charlie", "Delta", last.Title)
	if err := tt.RightClick(last.Title); err != nil {
		t.Fatal(err)
	}
	if err := tt.ChooseMenuItem(i18n.In("en", "workspace_archive")); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if want := []string{"Alpha", "Charlie", "Delta", last.Title}; !slices.Equal(archived, want) {
		t.Fatalf("batch archive targets = %q, want %q", archived, want)
	}

	s.Archived = true
	checkRows("Archived")

	// A manual move changes the Node order of conversations without
	// changing their creation ordinals.
	s.Archived = false
	*s.Sort = int(SortCreated)
	rows := project.Conversations
	rows[2], rows[3] = rows[3], rows[2]
	tt.Frame()
	clickSelectionRow(t, tt, more, 0)
	checkRows("Delta", "Bravo", "Foxtrot", "Echo", "Alpha", "Charlie", less)
}

func TestTerminalTitleDropsShellPrompt(t *testing.T) {
	for title, want := range map[string]string{
		"lixinhua@appledeMBP-2:/Volumes/Web/sailry-go": "/Volumes/Web/sailry-go",
		"dev@host: ~/app": "~/app",
		"zsh":             "zsh",
		"vim notes@2.md":  "vim notes@2.md",
	} {
		if got := TerminalTitle(title); got != want {
			t.Errorf("TerminalTitle(%q) = %q, want %q", title, got, want)
		}
	}
}
