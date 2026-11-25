package desktop_test

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

func TestHarnessOmitsCodePages(t *testing.T) {
	assembly := desktop.Harness()
	if assembly.CodeIndex {
		t.Fatal("Harness assembled the Code index settings")
	}
	if assembly.Worktrees {
		t.Fatal("Harness assembled the Code worktree actions")
	}
	if assembly.Terminal {
		t.Fatal("Harness assembled the Code terminal surfaces")
	}
	pages := map[shell.Page]bool{}
	for _, page := range assembly.WorkspacePages {
		pages[page.Page] = true
	}
	if pages[shell.GitPage] || !pages[shell.FilesPage] || !pages[shell.MarketPage] {
		t.Fatalf("Harness workspace pages = %v", pages)
	}
	for _, page := range assembly.SettingsPages {
		if page.Section == settings.Terminal {
			t.Fatal("Harness assembled the Code terminal settings page")
		}
	}
}
