package live

import (
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

func TestOmittedNativePagesDoNotReadOrCount(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		fixture, source := terminalTabsFixture(t)
		r, err := NewWithPreferences(t.Context(), fixture.controller, fixture.Preferences)
		if err != nil {
			t.Fatal(err)
		}
		defer r.Close()
		scope := r.App.Settings.MarketScope()
		if scope.Retry != nil || scope.More != nil || scope.Plugin != nil || scope.Skill != nil || scope.Theme != nil || scope.Image != nil {
			t.Fatal("market actions were installed without the market page")
		}
		r.countFrame()
		project := r.App.Data.Projects[0]
		r.App.SetResourceTarget(shell.ResourceTarget{Node: project.Host, Project: project.ID})
		for _, page := range []shell.Page{shell.FilesPage, shell.GitPage, shell.MarketPage} {
			r.App.Navigate(page)
			if r.App.Page != shell.ConversationPage {
				t.Fatalf("navigation opened omitted page %q", page)
			}
		}
		for _, page := range []shell.Page{shell.FilesPage, shell.GitPage, shell.MarketPage} {
			// A stale route must not activate a scope before the shell normalizes it.
			r.App.Page = page
			r.resourceWorkspaceFrame(nil)
			r.packagesFrame(nil)
			r.countFrame()
		}
		synctest.Wait()
		if r.resourcePage.shown != nil || len(r.workspace.projects) != 0 {
			t.Fatal("an omitted resource page bound a worktree")
		}
		for _, kind := range []string{"list_directory", "inspect_git", "search_plugin_catalog", "search_skill_catalog", "count_feature"} {
			if commands := source.sent(kind); len(commands) != 0 {
				t.Errorf("an omitted page sent %d %s commands", len(commands), kind)
			}
		}
	})
}
