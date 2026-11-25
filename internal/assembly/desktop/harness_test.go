package desktop_test

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

func TestHarnessOmitsCodePages(t *testing.T) {
	assembly := desktop.Harness()
	if len(assembly.ExecutionSettings) != 0 {
		t.Fatal("Harness assembled the Code index settings")
	}
	if len(assembly.ProjectWorkflows) != 0 {
		t.Fatal("Harness assembled the Code worktree actions")
	}
	if assembly.HasPanelResource(panel.Terminal) {
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
