package desktop_test

import (
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
	remindersdesktop "github.com/sailry/sailry-harness/modules/reminders/desktop"
	scheduledtasksdesktop "github.com/sailry/sailry-harness/modules/scheduled-tasks/desktop"
	statisticsdesktop "github.com/sailry/sailry-harness/modules/statistics/desktop"
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
	if assembly.HasPanelResource(panel.Files) {
		t.Fatal("Harness assembled the Code file browser")
	}
	pages := map[shell.Page]bool{}
	for _, page := range assembly.WorkspacePages {
		pages[page.Page] = true
	}
	if pages[shell.GitPage] || pages[shell.FilesPage] ||
		!pages[shell.MarketPage] || !pages[scheduledtasksdesktop.PageID] ||
		!pages[remindersdesktop.PageID] || !pages[statisticsdesktop.UsagePage] {
		t.Fatalf("Harness workspace pages = %v", pages)
	}
	for _, page := range assembly.SettingsPages {
		if page.Section == settings.Terminal {
			t.Fatal("Harness assembled the Code terminal settings page")
		}
	}
	if len(assembly.MetricContributions) != 2 {
		t.Fatalf("Harness metric contributions = %d, want 2", len(assembly.MetricContributions))
	}
	wantReplacements := []shell.PluginControlReplacement{
		{Package: "provider-status", Slot: protocol.PluginUiSlotFooter, ID: "usage"},
		{Package: "provider-status", Slot: protocol.PluginUiSlotProvider, ID: "provider_usage"},
		{Package: "statistics", Slot: protocol.PluginUiSlotStatistics, ID: "composer_tokens"},
		{Package: "statistics", Slot: protocol.PluginUiSlotStatistics, ID: "composer_speed"},
		{Package: "statistics", Slot: protocol.PluginUiSlotStatistics, ID: "composer_cost"},
		{Package: "statistics", Slot: protocol.PluginUiSlotStatistics, ID: "composer_cache"},
		{Package: "statistics", Slot: protocol.PluginUiSlotStatistics, ID: "composer_turns"},
	}
	for _, want := range wantReplacements {
		if !slices.Contains(assembly.PluginControlReplacements, want) {
			t.Fatalf("missing PluginControlReplacement %+v in %+v", want, assembly.PluginControlReplacements)
		}
	}
	// Verify all module locales merge cleanly without duplicate key panics.
	catalog := i18n.New(assembly.Locales...)
	if got := catalog.In("en", "scheduled_tasks_title"); got != "Scheduled tasks" {
		t.Fatalf("scheduled_tasks_title = %q", got)
	}
	if got := catalog.In("zh-CN", "reminders_page_title"); got != "待办提醒" {
		t.Fatalf("reminders_page_title = %q", got)
	}
}
