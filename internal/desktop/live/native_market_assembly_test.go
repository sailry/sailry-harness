package live

import (
	"context"
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type nativeMarketTransport struct {
	resourcePageTransport
	node *conversationTestSubscription
}

func (source *nativeMarketTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind == "node" {
		return source.node, nil
	}
	return source.resourcePageTransport.Subscribe(ctx, topic)
}

func TestNativeMarketAssembly(t *testing.T) {
	for _, includeMarket := range []bool{true, false} {
		name := "without-market"
		if includeMarket {
			name = "default"
		}
		t.Run(name, func(t *testing.T) {
			const pluginName = "fixture-tool"
			const pluginLabel = "Fixture Tool"
			const toolName = "fixture_action"
			const fileName = "featured.go"
			const changedName = "changed.go"
			project := protocol.DefaultProject()
			project.ID, project.Name, project.Path = protocol.ProjectID{1}, "Fixture", "/fixture"
			tree := protocol.Worktree{ID: protocol.WorktreeID{1}, Project: &project.ID, Path: project.Path, Main: true}
			info := protocol.DefaultPluginInfo()
			info.Summary.Name, info.Summary.Version, info.Summary.Digest = pluginName, new("1.2.3"), "fixture-digest"
			info.Origin = &protocol.PluginOrigin{Kind: "bundled"}
			extension := protocol.DefaultPluginExtension()
			extension.ApiVersion = "v1"
			extension.Display = &protocol.PluginDesktopNavigation{Label: pluginLabel}
			tool := protocol.DefaultToolDeclaration()
			tool.Name = toolName
			extension.Tools = []protocol.ToolDeclaration{tool}
			info.Extension = &extension
			entry := protocol.DefaultPluginCatalogEntry()
			entry.ID, entry.Name, entry.Version = pluginName, pluginLabel, info.Summary.Version
			entry.Compatibility = protocol.PluginCompatibility{Kind: "compatible"}
			entry.CompatibleVersion = &protocol.PluginCatalogVersion{Version: *entry.Version, Compatibility: entry.Compatibility}
			installed := info
			installed.Summary.Revision, installed.Summary.Enabled = 3, true
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node, snapshot.Projects, snapshot.Worktrees = protocol.NodeID{1}, []protocol.Project{project}, []protocol.Worktree{tree}
			if !includeMarket {
				snapshot.Plugins = []protocol.PluginSummary{installed.Summary}
			}
			source := &nativeMarketTransport{
				resourcePageTransport: resourcePageTransport{conversationTestTransport: conversationTestTransport{snapshot: snapshot}},
				node:                  &conversationTestSubscription{updates: make(chan protocol.Update, 4), closed: make(chan struct{})},
			}
			source.node.updates <- protocol.Update{Kind: "snapshot", Data: snapshot}
			current, cursor := installed, snapshot.Cursor
			publish := func(info protocol.PluginInfo) protocol.Output {
				current = info
				cursor++
				source.node.updates <- protocol.Update{Kind: "event", Data: protocol.EventEnvelope{
					Node: snapshot.Node, Cursor: cursor, Event: protocol.Event{Kind: "plugin_changed", Data: info.Summary},
				}}
				return protocol.Output{Kind: "plugin", Data: info}
			}
			source.handle = func(request protocol.Request) (link.Response, bool) {
				var out protocol.Output
				switch request.Command.Kind {
				case "read_python_settings":
					out = protocol.Output{Kind: "python_settings", Data: protocol.DefaultPythonInfo()}
				case "search_plugin_catalog":
					out = protocol.Output{Kind: "plugin_catalog", Data: protocol.PluginCatalogPage{Entries: []protocol.PluginCatalogEntry{entry}, Page: 1, Pages: 1}}
				case "read_catalog_plugin_info":
					out = protocol.Output{Kind: "catalog_details", Data: protocol.PluginCatalogDetails{Kind: "plugin", Data: info}}
				case "install_catalog_plugin":
					out = publish(installed)
				case "read_plugin", "read_plugin_version":
					out = protocol.Output{Kind: "plugin", Data: current}
				case "check_plugin_updates":
					out = protocol.Output{Kind: "plugin_updates", Data: protocol.DefaultPluginUpdatesBatch()}
				case "set_plugin_enabled":
					disabled := current
					disabled.Summary.Enabled, disabled.Summary.Revision = false, current.Summary.Revision+1
					out = publish(disabled)
				case "list_directory":
					out = protocol.Output{Kind: "directory", Data: protocol.Directory{Entries: []protocol.FileEntry{{Name: fileName, Kind: protocol.EntryKindFile}}}}
				case "inspect_git":
					out = protocol.Output{Kind: "git_status", Data: protocol.GitStatus{Kind: protocol.RepositoryKindReady, Branch: new("main"), Entries: []protocol.GitEntry{{Path: changedName, Unstaged: new(protocol.GitChangeKindModified)}}}}
				default:
					return link.Response{}, false
				}
				return link.Response{Output: out}, true
			}
			controller := client.New(source)
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			defer controller.Close()
			prefs := preferences.Open(filepath.Join(t.TempDir(), "preferences.json"))
			prefs.Data.WelcomeSeen = new(true)
			pages := desktop.Harness()
			if !includeMarket {
				pages = slices.DeleteFunc(pages, func(page shell.NativePage) bool { return page.Page == shell.MarketPage })
			}
			root, err := NewWithPreferences(t.Context(), controller, prefs, pages...)
			if err != nil {
				t.Fatal(err)
			}
			defer root.Close()
			app := root.App
			tester := ui.NewTesterWith(app.View, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
			tester.SetLocale("en")
			wakes := make(chan struct{}, 1)
			root.SetInvalidate(func() {
				select {
				case wakes <- struct{}{}:
				default:
				}
			})
			until := func(description string, ready func() bool) {
				t.Helper()
				timeout := time.NewTimer(5 * time.Second)
				defer timeout.Stop()
				for {
					tester.Frame()
					if ready() {
						return
					}
					select {
					case <-wakes:
					case <-timeout.C:
						t.Fatalf("waiting for %s: %q", description, tester.Texts())
					}
				}
			}
			click := func(role ui.Role, label string, inDetails bool) {
				t.Helper()
				until(label, func() bool {
					for _, item := range tester.AccessAll() {
						b := item.Bounds
						if item.Role != role || item.Name != label || item.Disabled || b.W <= 0 || b.H <= 0 || (inDetails && b.X < 1280-app.PanelWidth) {
							continue
						}
						tester.ClickAt(b.X+b.W/2, b.Y+b.H/2)
						tester.Frame()
						return true
					}
					return false
				})
			}
			requests := func(kind string) []protocol.Request {
				source.mu.Lock()
				defer source.mu.Unlock()
				var matches []protocol.Request
				for _, request := range source.requests {
					if request.Command.Kind == kind {
						matches = append(matches, request)
					}
				}
				return matches
			}
			market := i18n.In("en", "market_title")
			if includeMarket {
				click(ui.RoleButton, market, false)
				click(ui.RoleButton, pluginLabel, false)
				tester.Advance(250 * time.Millisecond)
				tester.Frame()
				if app.Settings.Details == nil || app.Settings.Details.ID != pluginName {
					t.Fatal("the catalog entry did not open its details")
				}
				click(ui.RoleButton, i18n.In("en", "market_install"), true)
				until("installed catalog plugin", func() bool {
					return slices.ContainsFunc(app.Settings.Packages, func(p viewmodel.Package) bool { return p.ID == pluginName && p.Installed }) && tester.HasText(i18n.In("en", "market_installed"))
				})
				search, install := requests("search_plugin_catalog"), requests("install_catalog_plugin")
				if app.Page != shell.MarketPage || len(search) != 1 || len(install) != 1 {
					t.Fatalf("catalog browse/install: page=%q searches=%d installs=%d", app.Page, len(search), len(install))
				}
				query := search[0].Command.Data.(protocol.CommandSearchPluginCatalogPayload)
				selection := install[0].Command.Data.(protocol.CommandInstallCatalogPluginPayload)
				if query.Source != protocol.PluginCatalogSourceOfficial || query.Page != 1 || selection.Selection.Info.Summary.Digest != info.Summary.Digest || selection.Selection.Info.Summary.Name != pluginName || selection.Mode != protocol.PluginInstallModeInstall || selection.ExpectedRevision != 0 {
					t.Fatalf("catalog installed a different selection: query=%+v install=%+v", query, selection)
				}
				return
			}

			app.Navigate(shell.MarketPage)
			tester.Frame()
			if _, found := tester.AccessByRole(ui.RoleButton, market); found || app.Page != shell.ConversationPage {
				t.Fatalf("omitted marketplace remains available: page=%q", app.Page)
			}
			app.SetResourceTarget(shell.ResourceTarget{Node: snapshot.Node.String(), Project: project.ID.String()})
			app.Navigate(shell.FilesPage)
			until("Files tree", func() bool { _, shown := tester.AccessByRole(ui.RoleTreeItem, fileName); return shown })
			app.Navigate(shell.GitPage)
			until("Git changes", func() bool { _, shown := tester.AccessByRole(ui.RoleTreeItem, changedName); return shown })
			if len(requests("list_directory")) == 0 || len(requests("inspect_git")) == 0 {
				t.Fatal("Files and Git did not load their workspace")
			}
			if len(requests("search_plugin_catalog")) != 0 || len(requests("search_skill_catalog")) != 0 {
				t.Fatal("opening Files and Git in the omitted assembly browsed the marketplace")
			}
			app.Navigate(shell.SettingsPage)
			app.Settings.Open(settings.Plugins)
			click(ui.RoleButton, pluginLabel, false)
			if app.Settings.Details == nil || app.Settings.Details.ID != pluginName {
				t.Fatal("the installed plugin did not open its settings details")
			}
			tester.Advance(250 * time.Millisecond)
			click(ui.RoleButton, i18n.In("en", "market_details_tools"), true)
			if !tester.HasText(toolName) {
				t.Fatalf("the installed plugin did not show its tool: %q", tester.Texts())
			}
			click(ui.RoleSwitch, i18n.In("en", "plugins_enabled")+" "+pluginLabel, false)
			until("installed plugin disabled", func() bool {
				return slices.ContainsFunc(controller.Snapshot().Plugins, func(p protocol.PluginSummary) bool { return p.Name == pluginName && !p.Enabled }) && !app.Settings.Busy[settings.BusyPackage+pluginName]
			})
			toggles := requests("set_plugin_enabled")
			if len(toggles) != 1 || toggles[0].Command.Data != (protocol.CommandSetPluginEnabledPayload{Name: pluginName, ExpectedRevision: installed.Summary.Revision}) {
				t.Fatalf("installed plugin switch sent %+v", toggles)
			}
			for _, request := range requests("search_plugin_catalog") {
				query := request.Command.Data.(protocol.CommandSearchPluginCatalogPayload)
				if query.Source != protocol.PluginCatalogSourceOfficial || query.Query != pluginName {
					t.Fatalf("installed plugin details issued an unrelated catalog search: %+v", query)
				}
			}
		})
	}
}
