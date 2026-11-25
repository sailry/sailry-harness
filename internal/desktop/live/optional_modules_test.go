package live

import (
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	harness "github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestOptionalDesktopSettingsAndTerminals(t *testing.T) {
	for _, assembly := range []struct {
		name string
		open func() shell.Assembly
		code bool
	}{
		{"empty", func() shell.Assembly { return shell.Assembly{} }, false},
		{"Harness", harness.Harness, false},
		{"Code contributions", testassembly.Full, true},
	} {
		for _, section := range []settings.Section{settings.Conversation, "plugin:fixture"} {
			t.Run(assembly.name+"/"+string(section), func(t *testing.T) {
				project := protocol.DefaultProject()
				project.ID, project.Name, project.Path = protocol.ProjectID{1}, "Optional modules", "/fixture"
				snapshot := protocol.DefaultSnapshot()
				snapshot.Node, snapshot.Projects = protocol.NodeID{1}, []protocol.Project{project}
				snapshot.Worktrees = []protocol.Worktree{{ID: protocol.WorktreeID{1}, Project: &project.ID, Path: project.Path, Main: true}}
				snapshot.Plugins = []protocol.PluginSummary{{Name: "commands", Enabled: true, Compatibility: protocol.PluginCompatibility{Kind: "compatible"}}}
				controller, source := optionalDesktopController(t, snapshot)
				root, err := NewWithAssembly(t.Context(), controller, preferences.Open(filepath.Join(t.TempDir(), "preferences.json")), assembly.open())
				if err != nil {
					t.Fatal(err)
				}
				t.Cleanup(func() { _ = root.Close() })
				model := root.App.Settings
				model.SetPluginPages([]settings.PluginPage{{Section: "plugin:fixture", Label: "Fixture", Render: func(*ui.Context) {}}})
				root.App.Navigate(shell.SettingsPage)
				model.Open(section)
				if model.Section != section {
					t.Fatalf("settings entry = %q, want %q", model.Section, section)
				}
				root.enterNodeSettings()
				root.nodeSettingsFrame()
				root.terminalSettingsFrame()
				root.workers.Wait()
				if len(source.sent("read_sandbox_settings")) == 0 {
					t.Fatal("settings entry did not read ordinary Node settings")
				}
				if got := len(source.sent("read_code_index_settings")) > 0; got != assembly.code {
					t.Fatalf("code-index settings read = %v, want %v", got, assembly.code)
				}
				if model.CodeIndexAvailable != assembly.code || (model.SaveCodeIndex != nil) != assembly.code || (model.SavePersistentCodeIndex != nil) != assembly.code {
					t.Fatalf("code-index settings callbacks disagree with the assembly: available %v, save %v, persistent %v", model.CodeIndexAvailable, model.SaveCodeIndex != nil, model.SavePersistentCodeIndex != nil)
				}
				if (root.packages.terminals != nil) != assembly.code || (root.App.Panel.TerminalActions != nil) != assembly.code {
					t.Fatal("terminal panel adapters disagree with the assembly")
				}
				if root.App.Sidebar.Actions == nil || (root.App.Sidebar.Actions.NewTerminal != nil) != assembly.code {
					t.Fatal("terminal sidebar action disagrees with the assembly")
				}
				if (root.App.Sidebar.Actions.Worktrees != nil) != assembly.code {
					t.Fatal("worktree sidebar action disagrees with the assembly")
				}
				if len(source.sent("read_terminal_settings")) != 0 {
					t.Fatal("an unrelated settings page read terminal settings")
				}
				if !assembly.code && (root.terminalSettings.shown != nil || model.SaveTerminal != nil) {
					t.Fatal("an absent terminal page bound a Node's terminal draft")
				}

				if assembly.code {
					model.Open(settings.Terminal)
					root.terminalSettingsFrame()
					root.workers.Wait()
					if len(source.sent("read_terminal_settings")) == 0 {
						t.Fatal("the assembled terminal settings page did not read its Node")
					}
				}

				listed := root.App.Data.Projects[0]
				terminal := i18n.In("en", "workspace_new_terminal")
				menu := ui.NewTester(func(view *ui.Context) {
					ui.ButtonBase(view.Key("project-menu")).Label("Project menu").Size(160, 28).ContextMenu(func(menu *ui.Menu) {
						root.projectPluginMenu(view, menu, listed)
					})
				}, 400, 300)
				menu.SetLocale("en")
				menu.Frame()
				if err := menu.RightClick("Project menu"); err != nil {
					t.Fatal(err)
				}
				if got := slices.Contains(menu.Menu(), terminal); got != assembly.code {
					t.Fatalf("project menu terminal = %v, want %v: %v", got, assembly.code, menu.Menu())
				}
			})
		}
	}
}

func optionalDesktopController(t *testing.T, snapshot protocol.Snapshot) (*client.Client, *terminalTabsTestNode) {
	t.Helper()
	source := &terminalTabsTestNode{pluginViewTestNode: pluginViewTestNode{snapshot: snapshot}}
	source.respond = func(protocol.Request) link.Response {
		return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture command is unavailable"}}
	}
	controller := client.New(link.NewLocal(snapshot.Node, snapshot.Node, source))
	changes := make(chan struct{}, 1)
	unsubscribe := controller.OnChange(func() {
		select {
		case changes <- struct{}{}:
		default:
		}
	})
	t.Cleanup(func() { unsubscribe(); _ = controller.Close() })
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	deadline := time.NewTimer(5 * time.Second)
	defer deadline.Stop()
	for controller.Snapshot().Node != snapshot.Node || !controller.Connected() {
		select {
		case <-changes:
		case <-deadline.C:
			t.Fatal("fixture Node did not deliver its snapshot")
		}
	}
	return controller, source
}
