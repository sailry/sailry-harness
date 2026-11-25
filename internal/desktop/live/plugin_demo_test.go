package live

import (
	"encoding/json"
	"slices"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDemoStatisticsContributionIsolation(t *testing.T) {
	const entry = "dev.sailry.platform/metrics.js"
	const private = "private-live-history-not-for-demo"
	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = pluginStatisticsPackage, strings.Repeat("f", 64), true
	info.Summary.Compatibility.Kind = "compatible"
	info.Extension = new(protocol.DefaultPluginExtension())
	info.Extension.Actions = []protocol.PluginAction{protocol.PluginActionReadConversation, protocol.PluginActionReadUsage}
	info.Extension.Desktop = &protocol.PluginDesktopManifest{UiEntry: new(entry), Resources: []string{entry}}
	if err := json.Unmarshal([]byte(`[{"id":"probe","slot":"statistics","kind":"metric","label":{"label":"Fixture metric"}}]`), &info.Extension.Ui); err != nil {
		t.Fatal(err)
	}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Plugins = protocol.NodeID{1}, []protocol.PluginSummary{info.Summary}
	session := protocol.DefaultSession()
	session.ID = protocol.SessionID{2}
	session.Activity.Title = private
	snapshot.Sessions = []protocol.Session{session}
	source := &pluginViewTestNode{snapshot: snapshot, bundle: make(chan link.Response, 1), reads: make(chan protocol.Request, 8)}
	root := pluginViewFixture(t, source)
	root.App.Data.Unassigned[0].Title = private
	live := root.App.Data.Unassigned[0]
	root.App.Settings.DemoMode = true
	tester := ui.NewTester(root.App.View, 960, 600)
	tester.Frame()
	demo := slices.Clone(root.App.Data.Unassigned)
	for _, project := range root.App.Data.Projects {
		demo = append(demo, project.Conversations...)
	}
	var selected string
	for _, conversation := range demo {
		if len(conversation.Turns) > 0 {
			selected = conversation.ID
			break
		}
	}
	if selected == "" {
		t.Fatal("demo has no sample statistics")
	}
	registry := root.demoPluginRegistry(root.App.DemoConversation(selected))
	key := pluginScopeIdentity(root.controller.Target(), protocol.PluginContext{Surface: protocol.PluginDesktopSurface{Kind: "composer"}})
	key.presentation = pluginDemoPresentation + selected
	owner := root.pluginViews.registries[key]
	if registry == nil || owner == nil {
		t.Fatal("demo registry was not retained")
	}
	for _, invalid := range []func(*protocol.PluginExtension){
		func(ext *protocol.PluginExtension) { ext.Actions = append(ext.Actions, "conversation.control") },
		func(ext *protocol.PluginExtension) { ext.Ui[0].Handler = new("invoke") },
		func(ext *protocol.PluginExtension) { ext.Desktop.UiOverlay = true },
	} {
		bad := info
		bad.Extension = new(*info.Extension)
		bad.Extension.Actions = slices.Clone(bad.Extension.Actions)
		bad.Extension.Ui = slices.Clone(bad.Extension.Ui)
		bad.Extension.Desktop = new(*bad.Extension.Desktop)
		invalid(bad.Extension)
		root.packages.infos[info.Summary.Name] = bad
		root.delivered++
		root.syncPluginRegistry(nil, owner)
		if len(root.pluginViews.mounts) != 0 {
			t.Fatal("a same-name package with non-statistical capabilities mounted in Demo")
		}
	}
	pluginViewRunQueue(root)
	if len(root.pluginViews.mounts) != 0 || len(source.reads) != 0 {
		t.Fatal("a rejected demo package began a bundle read")
	}
	root.packages.infos[info.Summary.Name] = info
	root.delivered++
	impostor := *live
	impostor.ID = selected
	if root.demoPluginRegistry(live) != nil || root.demoPluginRegistry(&impostor) != nil {
		t.Fatal("a live conversation entered the demo registry")
	}
	source.bundle <- link.Response{Output: protocol.Output{Kind: "plugin_view", Data: protocol.PluginDesktopBundle{
		Package: packageReference(info.Summary), Entry: entry, Files: map[string]string{entry: `
import {View,div} from 'gpui-kit';
import {context} from 'sailry';
import * as sdk from 'sailry/sdk';
import * as ui from 'sailry/ui';
export default class extends View {
  init() {
    const data = ui.readComposer();
    if (Object.keys(sdk).join() !== 'publishContributions' || Object.keys(ui).sort().join() !== 'nextComposerChange,readComposer') throw Error('live capability leaked');
    if (Object.keys(JSON.parse(context())).join() !== 'locale') throw Error('live context leaked');
    if (data.session !== null || !data.readonly || !data.statistics || Number(data.statistics.turns) < 1 || JSON.stringify(data).includes('` + private + `')) throw Error('expected only synthetic statistics');
    sdk.publishContributions([{id:'probe',value:'sample only'}]);
  }
  render() { return div(); }
}`},
	}}}
	root.syncPluginRegistry(nil, owner)
	root.workers.Wait()
	pluginViewRunQueue(root)
	mounted := root.pluginViews.mounts[pluginMountKey{scope: owner.key, role: "contribution", name: info.Summary.Name}]
	if mounted == nil || mounted.failure != nil || mounted.mounted == nil || mounted.host != nil {
		t.Fatalf("the demo metric did not mount without a live host: %+v", mounted)
	}
	if entries := registry.Entries(protocol.PluginUiSlotStatistics); len(entries) != 1 || string(entries[0].State.Value) != `"sample only"` {
		t.Fatalf("the isolated demo did not publish its metric: %+v", entries)
	}
	select {
	case request := <-source.reads:
		if request.Command.Kind != "read_plugin_view" || request.Plugin != nil {
			t.Fatalf("demo sent a scoped request to the live Node: %+v", request)
		}
		want, _ := json.Marshal(protocol.CommandReadPluginViewPayload{Package: packageReference(info.Summary), Surface: protocol.PluginDesktopSurface{Kind: "composer"}})
		got, _ := json.Marshal(request.Command.Data)
		if string(got) != string(want) {
			t.Fatalf("demo bundle fetch sent more than its immutable reference and surface: %s", got)
		}
	default:
		t.Fatal("demo statistics did not read its installed bundle")
	}
	for len(source.reads) != 0 {
		extra := <-source.reads
		t.Errorf("demo statistic sent an unexpected %s request: %+v", extra.Command.Kind, extra.Command.Data)
	}
}
