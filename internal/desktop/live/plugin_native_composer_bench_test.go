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
	return newSizedNativeComposerFrameFixture(t, 0)
}

// newSizedNativeComposerFrameFixture adds extra sessions, worktrees and
// projects around the mounted composer, and a worktree read so the location
// projection is refreshed each frame as well.
func newSizedNativeComposerFrameFixture(t testing.TB, extra int) nativeComposerFrameFixture {
	t.Helper()
	const entry = "dev.sailry.platform/desktop/composer.js"
	node, id, worktree := protocol.NodeID{1}, protocol.SessionID{2}, protocol.WorktreeID{3}
	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "composer-frame-fixture", strings.Repeat("a", 64), true
	info.Summary.Compatibility.Kind = "compatible"
	extension := protocol.DefaultPluginExtension()
	extension.Scope = protocol.PluginScopeHost
	extension.Actions = []protocol.PluginAction{protocol.PluginActionReadConversation, protocol.PluginActionProvideModels}
	if extra > 0 {
		extension.Actions = append(extension.Actions, protocol.PluginActionReadWorktrees)
	}
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
	for i := range extra {
		project := protocol.ProjectID{byte(i), 5}
		extraWorktree := protocol.WorktreeID{byte(i), 6}
		snapshot.Projects = append(snapshot.Projects, protocol.Project{ID: project, Name: fmt.Sprintf("project-%d", i)})
		snapshot.Worktrees = append(snapshot.Worktrees, protocol.Worktree{ID: extraWorktree, Project: &project, Main: true})
		for j := range 4 {
			extraSession := protocol.DefaultSession()
			extraSession.ID, extraSession.Worktree, extraSession.Revision = protocol.SessionID{byte(i), byte(j), 7}, extraWorktree, 1
			snapshot.Sessions = append(snapshot.Sessions, extraSession)
		}
	}
	source := &pluginViewTestNode{snapshot: snapshot, bundle: make(chan link.Response, 1), updates: make(chan protocol.Update, 1)}
	root := pluginViewFixture(t, source)
	root.packages.infos[info.Summary.Name] = info
	for _, extraSession := range snapshot.Sessions[1:] {
		root.App.Data.Unassigned = append(root.App.Data.Unassigned, &viewmodel.Conversation{ID: extraSession.ID.String(), Node: node.String()})
	}
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
	feed.latest.Store(nativeComposerFixtureHistory(node, id, tokens))
}

// nativeComposerFixtureHistory is one streamed publication: each call is a
// new detached view with its own statistics copy, as the Client delivers.
func nativeComposerFixtureHistory(node protocol.NodeID, id protocol.SessionID, tokens uint64) *preparedConversation {
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
	return &preparedConversation{state: client.ConversationState{Connected: true, Snapshot: snapshot}}
}

func (fixture nativeComposerFrameFixture) owners() int {
	return len(fixture.root.pluginViews.mounts) + len(fixture.root.pluginViews.registries) + len(fixture.root.conversationFeeds)
}

func BenchmarkPluginNativeComposerUnchangedFrame(b *testing.B) {
	benchmarkNativeComposerFrames(b, newNativeComposerFrameFixture(b), nil)
}

func BenchmarkPluginNativeComposerLargeUnchangedFrame(b *testing.B) {
	benchmarkNativeComposerFrames(b, newSizedNativeComposerFrameFixture(b, 64), nil)
}

// Streaming publishes a new transcript view each frame while the usage the
// composer projects stays the same; a changed usage must still publish.
func BenchmarkPluginNativeComposerStreamingFrame(b *testing.B) {
	fixture := newSizedNativeComposerFrameFixture(b, 64)
	node, id := fixture.source.snapshot.Node, *fixture.scope.Session
	feed := fixture.root.conversationFeeds[id]
	publications := []*preparedConversation{nativeComposerFixtureHistory(node, id, 7), nativeComposerFixtureHistory(node, id, 7)}
	frame := 0
	benchmarkNativeComposerFrames(b, fixture, func() {
		frame++
		feed.latest.Store(publications[frame%len(publications)])
	})
}

// Open/close cycles drop and remount the composer contribution on the same
// window and check that each close releases the mount and its registry.
func BenchmarkPluginNativeComposerOpenCloseCycle(b *testing.B) {
	fixture := newSizedNativeComposerFrameFixture(b, 64)
	bundle := *fixture.mounted.bundle
	views := fixture.root.pluginViews
	cycle := func() {
		var released []*pluginMountedView
		for _, owner := range views.registries {
			for _, mounted := range views.mounts {
				released = append(released, mounted)
			}
			fixture.root.closePluginRegistry(owner)
		}
		if len(views.mounts) != 0 || len(views.registries) != 0 {
			b.Fatalf("closing the composer retained %d mounts and %d registries", len(views.mounts), len(views.registries))
		}
		for _, mounted := range released {
			if mounted.host != nil || mounted.runtime != nil || mounted.ctx.Err() == nil {
				b.Fatal("closing the composer retained its host, runtime or context")
			}
		}
		fixture.source.bundle <- link.Response{Output: protocol.Output{Kind: "plugin_view", Data: bundle}}
		// The first frame makes the registry, the second mounts its view and
		// reads the bundle, and the third mounts the bundle read.
		fixture.window.Frame()
		fixture.window.Frame()
		fixture.root.workers.Wait()
		fixture.window.Frame()
		if len(views.mounts) != 1 || len(views.registries) != 1 {
			b.Fatalf("reopening the composer kept %d mounts and %d registries", len(views.mounts), len(views.registries))
		}
		for _, mounted := range views.mounts {
			if mounted.mounted == nil {
				b.Fatal("reopening the composer did not mount it")
			}
		}
	}
	var baseline, sample runtime.MemStats
	cycle()
	runtime.GC()
	runtime.ReadMemStats(&baseline)
	b.ReportAllocs()
	for b.Loop() {
		cycle()
	}
	runtime.GC()
	runtime.ReadMemStats(&sample)
	b.ReportMetric(float64(int64(sample.HeapAlloc)-int64(baseline.HeapAlloc)), "live-after-cycles-delta_B")
	if err := fixture.root.Close(); err != nil {
		b.Fatal(err)
	}
	b.ReportMetric(float64(fixture.owners()), "closed-owners")
}

func benchmarkNativeComposerFrames(b *testing.B, fixture nativeComposerFrameFixture, update func()) {
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
	frame := func() {
		if update != nil {
			update()
		}
		fixture.window.Frame()
	}
	for b.Loop() {
		frame()
	}
	for round := 1; round <= 3; round++ {
		const frames = 128
		var before runtime.MemStats
		runtime.ReadMemStats(&before)
		for range frames {
			frame()
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
