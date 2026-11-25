package live

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"testing/synctest"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func projectMenuFixture(t *testing.T, source *pluginViewTestNode) (*Runtime, *viewmodel.Project, protocol.WorktreeID) {
	t.Helper()
	project := protocol.DefaultProject()
	project.ID, project.Name, project.Path = protocol.ProjectID{1}, "Menu fixture project", t.TempDir()
	worktree := protocol.WorktreeID{2}
	source.snapshot.Node = protocol.NodeID{1}
	source.snapshot.Projects = []protocol.Project{project}
	source.snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Project: &project.ID, Path: project.Path, Main: true}}
	root := pluginViewFixture(t, source)
	row := &viewmodel.Project{ID: project.ID.String(), Host: source.snapshot.Node.String(), Name: project.Name, Path: project.Path}
	root.App.Data.Projects = []*viewmodel.Project{row}
	root.App.Sidebar.Actions = &sidebar.Actions{ProjectMenu: root.projectPluginMenu}
	return root, row, worktree
}

func TestProjectPluginMenuCapturesContribution(t *testing.T) {
	synctestWithRaster(t, func(t *testing.T) {
		root, project, _ := projectMenuFixture(t, &pluginViewTestNode{snapshot: protocol.DefaultSnapshot()})
		_, registry := root.projectPluginRegistry(project, "")
		if registry == nil {
			t.Fatal("project menu has no contribution registry")
		}
		ref := protocol.PluginReference{Name: "fixture", Digest: strings.Repeat("a", 64)}
		var declarations []protocol.PluginUiContribution
		if err := json.Unmarshal([]byte(`[
			{"id":"open","slot":"project_menu","kind":"menu","label":{"label":"Open"},"handler":"open",
			 "choices":[{"id":"folder","label":{"label":"Folder"},"children":[{"id":"first","label":{"label":"Notes"}}]}]}
		]`), &declarations); err != nil {
			t.Fatal(err)
		}
		mount := func(state string) plugins.ContributionHooks {
			t.Helper()
			hooks, err := registry.Hooks(ref, declarations)
			if err != nil {
				t.Fatal(err)
			}
			if err := hooks.Publish(json.RawMessage(state)); err != nil {
				t.Fatal(err)
			}
			if err := registry.SetActivity(ref, true, false); err != nil {
				t.Fatal(err)
			}
			return hooks
		}
		pick := func(tt *ui.Tester, path ...string) {
			t.Helper()
			if err := tt.ChooseMenuItem(path...); err != nil {
				t.Fatal(err)
			}
		}
		open := func(tt *ui.Tester) {
			t.Helper()
			if err := tt.RightClick(project.Name); err != nil {
				t.Fatal(err)
			}
		}
		next := func(hooks plugins.ContributionHooks) (json.RawMessage, error) {
			t.Helper()
			ctx, cancel := context.WithTimeout(t.Context(), time.Second)
			defer cancel()
			return hooks.Next(ctx)
		}
		const enabled = `[{"id":"open","visible":true,"enabled":true}]`
		mount(enabled)
		tt := ui.NewTester(root.App.View, 1240, 900)
		tt.SetLocale("en")
		tt.SetScale(0.125)
		tt.Frame()
		open(tt)
		current := mount(enabled)
		pick(tt, "Open", "Folder", "Notes")
		if body, err := next(current); !errors.Is(err, context.DeadlineExceeded) {
			t.Fatalf("a choice from the old menu reached a new binding: %s (%v)", body, err)
		}

		if err := current.Publish(json.RawMessage(`[{"id":"open","visible":true,"enabled":true,"label":{"label":"Open notes"}}]`)); err != nil {
			t.Fatal(err)
		}
		open(tt)
		pick(tt, "Open notes", "Folder", "Notes")
		body, err := next(current)
		var event protocol.PluginUiEvent
		if err != nil || json.Unmarshal(body, &event) != nil || event.ID != "open" || event.Kind != "change" || string(event.Value) != `"first"` {
			t.Fatalf("menu returned an unexpected event: %s (%v)", body, err)
		}
		open(tt)
		if err := tt.ChooseMenuItem("Open notes", "Folder", "Notes"); err == nil || !strings.Contains(err.Error(), `item "Open notes" is disabled`) {
			t.Fatalf("pending contribution did not disable its submenu: %v", err)
		}
	})
}

func TestProjectPluginMenuAndToolbarShareAppAction(t *testing.T) {
	synctestWithRaster(t, func(t *testing.T) {
		const entry = "dev.sailry.platform/ui.js"
		const action = "fixture.project-tools"
		info := protocol.DefaultPluginInfo()
		info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "project-tools", strings.Repeat("b", 64), true
		info.Summary.Compatibility.Kind = "compatible"
		info.Extension = new(protocol.DefaultPluginExtension())
		info.Extension.Scope = protocol.PluginScopeHost
		info.Extension.Desktop = new(protocol.DefaultPluginDesktopManifest())
		info.Extension.Desktop.UiEntry = new(entry)
		if err := json.Unmarshal([]byte(`[
			{"id":"menu","slot":"project_menu","kind":"button","label":{"label":"Project tools menu"},"handler":"open"},
			{"id":"toolbar","slot":"project","kind":"button","label":{"label":"Project tools toolbar"},"handler":"open"}
		]`), &info.Extension.Ui); err != nil {
			t.Fatal(err)
		}
		snapshot := protocol.DefaultSnapshot()
		snapshot.Plugins = []protocol.PluginSummary{info.Summary}
		source := &pluginViewTestNode{snapshot: snapshot, bundle: make(chan link.Response, 1)}
		root, project, worktree := projectMenuFixture(t, source)
		root.packages.infos[info.Summary.Name] = info
		source.bundle <- link.Response{Output: protocol.Output{Kind: "plugin_view", Data: protocol.PluginDesktopBundle{
			Package: packageReference(info.Summary), Entry: entry, Files: map[string]string{entry: `
import {View, div} from 'gpui-kit';
import {publishContributions, nextContributionEvent} from 'sailry/sdk';
import {invokeAppAction} from 'sailry/ui';
export default class extends View {
  init(_props, cx) {
    publishContributions([{id:'menu'}, {id:'toolbar'}]);
    cx.spawn(async () => {
      while (true) {
        const event = await nextContributionEvent();
        await invokeAppAction('` + action + `', {entry:event.id});
        publishContributions([{id:event.id, reply_to:event.sequence}]);
      }
    });
  }
  render() { return div(); }
}`},
		}}}
		var calls []string
		var scopes []shell.AppActionContext
		wantScope := shell.AppActionScope{Node: source.snapshot.Node, Worktree: worktree}
		if _, err := root.App.RegisterAppAction(shell.AppAction{
			ID: action, PublicNavigation: true,
			Run: func(view *ui.Context, app *shell.App, captured shell.AppActionContext, input json.RawMessage) error {
				var payload struct{ Entry string }
				if view == nil || app != root.App || !captured.Current() || captured.AppActionScope != wantScope || json.Unmarshal(input, &payload) != nil {
					t.Fatalf("action received the wrong context or input: %+v %s", captured.AppActionScope, input)
				}
				calls, scopes = append(calls, payload.Entry), append(scopes, captured)
				return nil
			},
		}); err != nil {
			t.Fatal(err)
		}
		root.App.Hooks.BeforeFrame = func(view *ui.Context) {
			root.mu.Lock()
			queue := root.queue
			root.queue = nil
			root.mu.Unlock()
			for _, apply := range queue {
				apply(view)
			}
			root.pluginViewsFrame(view)
		}
		root.App.Hooks.AfterFrame = root.pluginViewsAfterFrame
		tt := ui.NewTester(root.App.View, 1240, 900)
		tt.SetLocale("en")
		tt.SetScale(0.125)
		wait := func(label string, ready func() bool) {
			t.Helper()
			for range 8 {
				tt.Frame()
				synctest.Wait()
				for _, mounted := range root.pluginViews.mounts {
					if mounted.failure != nil {
						t.Fatalf("%s: %v", label, mounted.failure)
					}
				}
				if ready() {
					return
				}
			}
			t.Fatalf("%s: calls %v, visible text %q", label, calls, tt.Texts())
		}
		wait("mount project contribution", func() bool {
			_, registry := root.projectPluginRegistry(project, "")
			entries := registry.Entries(protocol.PluginUiSlotProjectMenu)
			return len(entries) == 1 && entries[0].State.Enabled
		})
		if err := tt.RightClick(project.Name); err != nil {
			t.Fatal(err)
		}
		if err := tt.ChooseMenuItem("Project tools menu"); err != nil {
			t.Fatal(err)
		}
		wait("menu invokes action", func() bool { return len(calls) == 1 && calls[0] == "menu" })
		root.App.OpenProject(project, shell.ProjectActions{Controls: func(view *ui.Context) { root.renderProjectControls(view, project, "") }})
		tt.Frame()
		if err := tt.Click("Project tools toolbar"); err != nil {
			t.Fatal(err)
		}
		wait("toolbar invokes the same action", func() bool { return len(calls) == 2 && calls[1] == "toolbar" })
		root.closePluginViews()
		for _, scope := range scopes {
			if scope.Current() {
				t.Fatal("closed plugin retained a live action source")
			}
		}
	})
}
