package live

import (
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginPagePresentationLifecycle(t *testing.T) {
	const entry = "dev.sailry.platform/desktop/main.js"
	makePlugin := func(name, digest string) protocol.PluginInfo {
		info := protocol.DefaultPluginInfo()
		info.Summary.Name, info.Summary.Digest, info.Summary.Revision, info.Summary.Enabled = name, strings.Repeat(digest, 64), 1, true
		info.Summary.Compatibility.Kind = "compatible"
		extension := protocol.DefaultPluginExtension()
		extension.Scope = protocol.PluginScopeHost
		extension.Actions = []protocol.PluginAction{protocol.PluginActionReadFiles}
		desktop := protocol.DefaultPluginDesktopManifest()
		desktop.Entry = new(entry)
		desktop.Navigation = &protocol.PluginDesktopNavigation{Label: name}
		desktop.NavigationOptions.Pinned = true
		desktop.NavigationOptions.Target = protocol.PluginDesktopNavigationTargetWorktree
		extension.Desktop, info.Extension = &desktop, &extension
		return info
	}
	one, two := makePlugin("Lifecycle One", "a"), makePlugin("Lifecycle Two", "b")
	node, tree := protocol.NodeID{1}, protocol.WorktreeID{2}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Plugins = node, []protocol.PluginSummary{one.Summary, two.Summary}
	snapshot.Worktrees = []protocol.Worktree{{ID: tree}}
	source := &pluginViewTestNode{snapshot: snapshot, bundle: make(chan link.Response, 1), reads: make(chan protocol.Request, 1),
		updates: make(chan protocol.Update, 1), files: make(chan *pluginViewTestSubscription, 1)}
	root := pluginViewFixture(t, source)
	root.workspace = &workspaceState{selected: tree}
	root.packages.infos[one.Summary.Name] = one
	root.packages.infos[two.Summary.Name] = two
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
	window := ui.NewTesterWith(root.App.View, 960, 620, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true}})
	window.SetLocale("en")
	finishMount := func(info protocol.PluginInfo) (*pluginMountedView, *pluginViewTestSubscription) {
		t.Helper()
		window.Frame()
		select {
		case request := <-source.reads:
			read, ok := detail[protocol.CommandReadPluginViewPayload](request.Command.Data)
			if !ok || read.Package != packageReference(info.Summary) {
				t.Fatalf("wrong view request: %+v", request.Command)
			}
		case <-time.After(2 * time.Second):
			t.Fatal("selected page never requested its view")
		}
		label, err := json.Marshal(info.Summary.Name + " contents")
		if err != nil {
			t.Fatal(err)
		}
		source.bundle <- link.Response{Output: protocol.Output{Kind: "plugin_view", Data: protocol.PluginDesktopBundle{
			Package: packageReference(info.Summary), Entry: entry, Files: map[string]string{entry: `
import {View, div} from "gpui-kit";
import {next_change} from "sailry";
export default class extends View {
  init(){next_change("");}
  render(){return div().child(` + string(label) + `);}
}`},
		}}}
		root.workers.Wait()
		window.Frame()
		if !window.HasText(info.Summary.Name+" contents") || len(root.pluginViews.mounts) != 1 {
			t.Fatalf("selected page did not own exactly one rendered view: %v", window.Texts())
		}
		var mounted *pluginMountedView
		for _, mounted = range root.pluginViews.mounts {
		}
		select {
		case stream := <-source.files:
			return mounted, stream
		case <-time.After(2 * time.Second):
			t.Fatal("plugin never acquired its file observer")
		}
		return nil, nil
	}
	open := func(info protocol.PluginInfo) (*pluginMountedView, *pluginViewTestSubscription) {
		t.Helper()
		if err := window.Click(info.Summary.Name); err != nil {
			t.Fatal(err)
		}
		return finishMount(info)
	}
	checkClosed := func(mounted *pluginMountedView, stream *pluginViewTestSubscription) {
		t.Helper()
		if mounted.ctx.Err() == nil || mounted.runtime != nil || mounted.mounted != nil || mounted.host != nil || mounted.bundle != nil || len(mounted.components) != 0 {
			t.Fatal("retired page retained its execution or presentation")
		}
		select {
		case <-stream.closed:
		case <-time.After(2 * time.Second):
			t.Fatal("retired page retained its file observer")
		}
	}
	window.Frame()
	for range 3 {
		first, firstObserver := open(one)
		late := false
		if !first.host.Native().Post(func(*ui.Context) { late = true }) {
			t.Fatal("active view rejected a native post")
		}
		if !root.App.ClosePage() {
			t.Fatal("the plugin page could not close")
		}
		window.Frame()
		checkClosed(first, firstObserver)
		if late || len(root.pluginViews.mounts) != 0 {
			t.Fatal("closing the page delivered a stale native post or kept a mount")
		}
		first, firstObserver = open(one)
		second, secondObserver := open(two)
		checkClosed(first, firstObserver)
		if !root.App.ClosePage() {
			t.Fatal("the second plugin page could not close")
		}
		window.Frame()
		checkClosed(second, secondObserver)
	}
	publish := func(info protocol.PluginInfo) {
		t.Helper()
		snapshot.Cursor++
		snapshot.Plugins = []protocol.PluginSummary{info.Summary, two.Summary}
		root.packages.infos[info.Summary.Name] = info
		cursor := snapshot.Cursor
		applied := make(chan struct{}, 1)
		defer root.controller.OnChange(func() {
			if root.snapshot().Cursor == cursor {
				select {
				case applied <- struct{}{}:
				default:
				}
			}
		})()
		source.updates <- protocol.Update{Kind: "snapshot", Data: snapshot}
		select {
		case <-applied:
		case <-time.After(2 * time.Second):
			t.Fatal("Client did not project the package update")
		}
	}
	first, firstObserver := open(one)
	updated := one
	updated.Summary.Revision++
	updated.Summary.Digest = strings.Repeat("c", 64)
	publish(updated)
	replacement, replacementObserver := finishMount(updated)
	checkClosed(first, firstObserver)
	if replacement == first || replacement.generation == first.generation {
		t.Fatal("new package version reused the retired view")
	}
	updated.Summary.Revision++
	updated.Summary.Enabled = false
	publish(updated)
	window.Frame()
	checkClosed(replacement, replacementObserver)
	if len(root.pluginViews.mounts) != 0 {
		t.Fatal("disabled package retained its view")
	}
}
