package live

import (
	"slices"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"

	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A terminal started from a project's New terminal is a row under that
// project in the sidebar; its right-click menu opens it and, once
// confirmed, closes its shell on the Node.
func TestSidebarListsAndClosesProjectTerminal(t *testing.T) {
	synctestWithRaster(t, func(t *testing.T) {
		root, source := terminalTabsFixture(t)
		var created protocol.TerminalInfo
		source.respond = func(request protocol.Request) link.Response {
			switch request.Command.Kind {
			case "create_terminal":
				launch, _ := detail[protocol.TerminalLaunch](request.Command.Data)
				created = terminalTabsTestInfo(0, launch.Worktree, "running")
				created.ID, created.Title = protocol.TerminalID(request.ID), new("zsh")
				return link.Response{Output: protocol.Output{Kind: "terminal", Data: created}}
			case "close_terminal":
				return link.Response{Output: protocol.Output{Kind: "terminal", Data: created}}
			}
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture command is unavailable"}}
		}
		host := root.controller.Target().String()
		project := &viewmodel.Project{ID: protocol.ProjectID{1}.String(), Host: host, Name: "Terminal project", Path: "/fixture"}
		root.App.Data.Projects = []*viewmodel.Project{project}
		root.App.Sidebar.Actions = &sidebar.Actions{}
		root.initTerminalActions()

		root.App.Sidebar.Actions.NewTerminal(project)
		synctest.Wait()
		source.snapshot.Terminals = []protocol.TerminalInfo{created}
		if err := root.controller.Refresh(root.ctx); err != nil {
			t.Fatal(err)
		}
		synctest.Wait()

		var answer func(int)
		tt := ui.NewTester(func(c *ui.Context) {
			kit.BindConfirmations(c, func(_ kit.Confirmation, done func(int)) { answer = done })
			sidebar.View(c, root.App.Sidebar)
		}, 320, 600)
		tt.SetLocale("en")
		tt.Frame()
		if !tt.HasText("zsh") {
			t.Fatalf("the project's terminal has no sidebar row: %v", tt.Texts())
		}
		if err := tt.RightClick("zsh"); err != nil {
			t.Fatal(err)
		}
		want := []string{i18n.In("en", "directory_open"), "-", i18n.In("en", "workspace_close_terminal")}
		if menu := tt.Menu(); !slices.Equal(menu, want) {
			t.Fatalf("terminal menu = %v, want %v", menu, want)
		}
		if err := tt.ChooseMenuItem(i18n.In("en", "workspace_close_terminal")); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		if answer == nil || len(source.sent("close_terminal")) != 0 {
			t.Fatal("closing a terminal from the sidebar did not ask first")
		}
		answer(1)
		tt.Frame()
		synctest.Wait()
		closes := source.sent("close_terminal")
		if len(closes) != 1 {
			t.Fatalf("close_terminal commands = %d, want 1", len(closes))
		}
		payload, _ := detail[protocol.CommandCloseTerminalPayload](closes[0].Data)
		if payload.Terminal != created.ID || payload.Worktree != (protocol.WorktreeID{1}) {
			t.Fatalf("the sidebar closed another terminal: %+v", payload)
		}

		source.snapshot.Terminals = nil
		if err := root.controller.Refresh(root.ctx); err != nil {
			t.Fatal(err)
		}
		synctest.Wait()
		tt.Frame()
		if tt.HasText("zsh") {
			t.Fatal("the closed terminal kept its sidebar row")
		}
	})
}

// A terminal's row reads its title without the shell's user@host prefix,
// opens the terminal as a kept tab as a session row does, and is the
// selected row while the terminal shows.
func TestSidebarTerminalRowOpensKeptTab(t *testing.T) {
	synctestWithRaster(t, func(t *testing.T) {
		root, source := terminalTabsFixture(t)
		var created protocol.TerminalInfo
		source.respond = func(request protocol.Request) link.Response {
			if request.Command.Kind == "create_terminal" {
				launch, _ := detail[protocol.TerminalLaunch](request.Command.Data)
				created = terminalTabsTestInfo(0, launch.Worktree, "running")
				created.ID, created.Title = protocol.TerminalID(request.ID), new("dev@mac-2:/fixture/app")
				return link.Response{Output: protocol.Output{Kind: "terminal", Data: created}}
			}
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture command is unavailable"}}
		}
		host := root.controller.Target().String()
		project := &viewmodel.Project{ID: protocol.ProjectID{1}.String(), Host: host, Name: "Terminal project", Path: "/fixture"}
		root.App.Data.Projects = []*viewmodel.Project{project}
		root.App.Sidebar.Actions = &sidebar.Actions{}
		root.initTerminalActions()
		root.App.Sidebar.Actions.NewTerminal(project)
		synctest.Wait()
		source.snapshot.Terminals = []protocol.TerminalInfo{created}
		if err := root.controller.Refresh(root.ctx); err != nil {
			t.Fatal(err)
		}
		synctest.Wait()

		id := created.ID.String()
		target := panes.Target{Kind: panes.Terminal, ID: id}
		tt := ui.NewTester(func(c *ui.Context) { sidebar.View(c, root.App.Sidebar) }, 320, 600)
		tt.SetLocale("en")
		tt.Frame()
		if !tt.HasText("/fixture/app") || tt.HasText("dev@mac-2:/fixture/app") {
			t.Fatalf("the terminal row kept its user@host prefix: %v", tt.Texts())
		}
		if row, ok := tt.Access("/fixture/app"); !ok || row.Selected || row.Checked {
			t.Fatalf("the terminal row is selected before the terminal shows: %+v", row)
		}
		if err := tt.Click("/fixture/app"); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		if !slices.Contains(root.App.Prefs().Tabs, target) {
			t.Fatalf("the sidebar opened a passing tab: kept %v", root.App.Prefs().Tabs)
		}
		root.App.Sidebar.Terminal = id
		tt.Frame()
		if row, _ := tt.Access("/fixture/app"); !row.Selected && !row.Checked {
			t.Fatal("the shown terminal's row is not selected")
		}
	})
}
