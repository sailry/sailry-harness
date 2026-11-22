package live

import (
	"encoding/json"
	"fmt"
	"runtime"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type nativeComposerFrameFixture struct {
	root    *Runtime
	window  *ui.Tester
	source  *pluginViewTestNode
	mounted *pluginMountedView
	native  plugins.NativeBinding
	scope   protocol.PluginContext
}

func newNativeComposerFrameFixture(t testing.TB) nativeComposerFrameFixture {
	t.Helper()
	const entry = "dev.sailry.platform/desktop/composer.js"
	node, id, worktree := protocol.NodeID{1}, protocol.SessionID{2}, protocol.WorktreeID{3}
	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "composer-frame-fixture", strings.Repeat("a", 64), true
	info.Summary.Compatibility.Kind = "compatible"
	extension := protocol.DefaultPluginExtension()
	extension.Scope = protocol.PluginScopeHost
	extension.Actions = []protocol.PluginAction{protocol.PluginActionReadConversation, protocol.PluginActionProvideModels}
	extension.ModelResources = []protocol.PluginModelAuthResource{{Authentication: protocol.AuthenticationChatGpt}}
	extension.ConversationOptions = new(json.RawMessage(`{"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"budget":{"type":"string","default":"balanced"}},"additionalProperties":false}`))
	desktop := protocol.DefaultPluginDesktopManifest()
	desktop.UiEntry = new(entry)
	extension.Desktop, info.Extension = &desktop, &extension
	if err := json.Unmarshal([]byte(`[{"id":"sample","slot":"commands","kind":"button","label":{"label":"Sample"},"handler":"sample","command":{"name":"sample","kind":"message"}}]`), &extension.Ui); err != nil {
		t.Fatal(err)
	}
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Revision, provider.Enabled = protocol.ProviderID{4}, 1, true
	provider.Name, provider.Api, provider.Endpoint = "Fixture", protocol.ConversationModelApiResponses, "https://fixture.invalid"
	provider.Authentication, provider.DefaultModel = protocol.AuthenticationChatGpt, "fixture-model"
	provider.Models = []protocol.ConversationModel{{ID: provider.DefaultModel, Context: 128_000, Tools: true, DefaultEffort: protocol.Effort{Kind: "default"}}}
	session := protocol.DefaultSession()
	session.ID, session.Worktree, session.Revision = id, worktree, 1
	session.Config.Provider, session.Config.Model = provider.ID, provider.DefaultModel
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Plugins = node, []protocol.PluginSummary{info.Summary}
	snapshot.Sessions, snapshot.Worktrees = []protocol.Session{session}, []protocol.Worktree{{ID: worktree, Main: true}}
	snapshot.Providers = []protocol.ConversationProvider{provider}
	source := &pluginViewTestNode{snapshot: snapshot, bundle: make(chan link.Response, 1), updates: make(chan protocol.Update, 1)}
	root := pluginViewFixture(t, source)
	root.packages.infos[info.Summary.Name] = info
	root.App.Data.Unassigned = append(root.App.Data.Unassigned, &viewmodel.Conversation{ID: id.String(), Node: node.String()})
	root.App.Composer(id.String()).Draft = "/sample fixture arguments"
	root.App.Page = shell.ConversationPage
	root.App.Mode = theme.Light
	root.App.NavOpen = true
	feed := &conversationFeed{taken: make(chan struct{}, 1)}
	root.conversationFeeds = map[protocol.SessionID]*conversationFeed{id: feed}
	root.conversationWatches = map[protocol.SessionID]func(){id: func() {
		delete(root.conversationFeeds, id)
		delete(root.conversationCache, id)
		delete(root.conversationWatches, id)
	}}
	publishNativeComposerFixtureHistory(feed, node, id, 7)
	scope := protocol.DefaultPluginContext()
	scope.Package, scope.Surface.Kind, scope.Session, scope.Worktree = packageReference(info.Summary), "composer", &id, &worktree
	root.pluginRegistry(scope)
	window := ui.NewTesterWith(func(view *ui.Context) {
		theme.Apply(view, root.App.Theme, root.App.Mode)
		root.beginConversationFrame()
		root.conversationSeen[id] = true
		root.mu.Lock()
		queue := root.queue
		root.queue = nil
		root.mu.Unlock()
		for _, apply := range queue {
			apply(view)
		}
		root.pluginViewsFrame(view)
		root.pluginRegistry(scope)
		root.pluginViewsAfterFrame()
		root.finishConversationFrame()
	}, 960, 620, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true}})
	window.SetLocale("en")
	source.bundle <- link.Response{Output: protocol.Output{Kind: "plugin_view", Data: protocol.PluginDesktopBundle{
		Package: scope.Package, Entry: entry,
		Files: map[string]string{entry: `import {View, div} from "gpui-kit";
export default class extends View { render(){return div().child("Composer frame fixture");} }`},
	}}}
	window.Frame()
	root.workers.Wait()
	window.Frame()
	key := pluginMountKey{scope: pluginScopeIdentity(node, scope), role: "contribution", name: scope.Package.Name}
	mounted := root.pluginViews.mounts[key]
	if mounted == nil || mounted.host == nil || mounted.mounted == nil {
		t.Fatalf("fixture composer contribution did not mount: %+v", mounted)
	}
	if err := mounted.hooks.Publish(json.RawMessage(`[{"id":"sample","enabled":true}]`)); err != nil {
		t.Fatal(err)
	}
	window.Frame()
	return nativeComposerFrameFixture{root: root, window: window, source: source, mounted: mounted, native: mounted.host.Native(), scope: scope}
}

func publishNativeComposerFixtureHistory(feed *conversationFeed, node protocol.NodeID, id protocol.SessionID, tokens uint64) {
	statistics := protocol.DefaultConversationStatistics()
	statistics.Turns, statistics.Responses = 1, 1
	statistics.ContextTokens, statistics.ContextLimit = &tokens, new(uint32(128_000))
	statistics.Usage = &protocol.ConversationUsage{Input: 4, Output: 3}
	for i := range 12 {
		request := protocol.DefaultConversationObservationRequest()
		request.ID, request.Turn, request.Model = fmt.Sprintf("fixture-request-%d", i), protocol.TurnID{1}, "fixture-model"
		attempt := protocol.DefaultConversationObservationAttempt()
		attempt.Index, attempt.Transport, attempt.Outcome = 1, "http", protocol.ConversationObservationOutcomeSucceeded
		attempt.Usage = statistics.Usage
		request.Attempts = []protocol.ConversationObservationAttempt{attempt}
		statistics.Observations.Recent = append(statistics.Observations.Recent, request)
	}
	snapshot := protocol.DefaultConversationSnapshot()
	snapshot.Node, snapshot.Sequence, snapshot.Statistics = node, 1, &statistics
	snapshot.Page = cacheTestPage(id, 1, "Fixed composer fixture")
	feed.latest.Store(&preparedConversation{state: client.ConversationState{Connected: true, Snapshot: snapshot}})
}

func (fixture nativeComposerFrameFixture) owners() int {
	return len(fixture.root.pluginViews.mounts) + len(fixture.root.pluginViews.registries) + len(fixture.root.conversationFeeds)
}

func BenchmarkPluginNativeComposerUnchangedFrame(b *testing.B) {
	fixture := newNativeComposerFrameFixture(b)
	const openOwners = 3
	if fixture.owners() != openOwners {
		b.Fatalf("expected a mount, a registry and a feed, got %d owners", fixture.owners())
	}
	if body, err := fixture.native.Operations["sailry/ui.readComposer"](b.Context(), nil); err != nil || len(body) == 0 {
		b.Fatalf("composer fixture did not publish a projection: %s, %v", body, err)
	}
	var baseline, sample runtime.MemStats
	runtime.GC()
	runtime.ReadMemStats(&baseline)
	b.ReportAllocs()
	for b.Loop() {
		fixture.window.Frame()
	}
	for round := 1; round <= 3; round++ {
		const frames = 128
		var before runtime.MemStats
		runtime.ReadMemStats(&before)
		for range frames {
			fixture.window.Frame()
		}
		runtime.ReadMemStats(&sample)
		b.ReportMetric(float64(sample.TotalAlloc-before.TotalAlloc)/frames, fmt.Sprintf("round-%d-B/frame", round))
		runtime.GC()
		runtime.ReadMemStats(&sample)
		b.ReportMetric(float64(int64(sample.HeapAlloc)-int64(baseline.HeapAlloc)), fmt.Sprintf("live-round-%d-delta_B", round))
		if fixture.owners() != openOwners {
			b.Fatalf("round %d changed the plugin owner count to %d", round, fixture.owners())
		}
	}
	b.ReportMetric(float64(fixture.owners()), "open-owners")
	if err := fixture.root.Close(); err != nil {
		b.Fatal(err)
	}
	owners := fixture.owners()
	if owners != 0 {
		b.Fatalf("closing the window retained %d plugin owners", owners)
	}
	fixture = nativeComposerFrameFixture{}
	runtime.GC()
	runtime.ReadMemStats(&sample)
	b.ReportMetric(float64(int64(sample.HeapAlloc)-int64(baseline.HeapAlloc)), "live-closed-delta_B")
	b.ReportMetric(float64(owners), "closed-owners")
}
