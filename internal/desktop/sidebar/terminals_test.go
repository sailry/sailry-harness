package sidebar

import (
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// Sorting by name orders terminal rows by the titles they show.
func TestTerminalRowsSortByShownTitle(t *testing.T) {
	pr := &viewmodel.Project{ID: "project", Name: "Project"}
	s := New(&viewmodel.Data{Projects: []*viewmodel.Project{pr}}, "")
	*s.Sort = int(SortName)
	s.Actions = &Actions{Terminals: func(*viewmodel.Project) []Terminal {
		return []Terminal{
			{ID: "1", Title: "alice@host:~/zeta"},
			{ID: "2", Title: "~/beta"},
			{ID: "3", Title: "zed@host:~/alpha"},
		}
	}}
	tt := selectionTester(s)
	var rows []string
	for _, text := range tt.Texts() {
		if slices.Contains([]string{"~/zeta", "~/beta", "~/alpha"}, text) {
			rows = append(rows, text)
		}
	}
	// A row's button label and its text both read its title.
	rows = slices.Compact(rows)
	if want := []string{"~/alpha", "~/beta", "~/zeta"}; !slices.Equal(rows, want) {
		t.Fatalf("terminal rows = %q, want %q", rows, want)
	}
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
