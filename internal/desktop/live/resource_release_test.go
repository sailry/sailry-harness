package live

import (
	"encoding/json"
	"runtime"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSelectTeamClosureDoesNotRetainPreviousSnapshot(t *testing.T) {
	nodeID := protocol.NodeID{1}
	teamA := protocol.DefaultTeamProfile()
	teamA.ID, teamA.Name, teamA.Revision = protocol.TeamID{1}, "Alpha", 1
	session := protocol.DefaultSession()
	session.ID, session.Worktree, session.Revision = protocol.SessionID{10}, protocol.WorktreeID{2}, 1

	initial := protocol.DefaultSnapshot()
	initial.Node, initial.Cursor = nodeID, 1
	initial.Teams = []protocol.TeamProfile{teamA}
	initial.Sessions = []protocol.Session{session}

	updates := make(chan protocol.Update, 2)
	source := &pluginViewTestNode{snapshot: initial, updates: updates}
	r := pluginViewFixture(t, source)
	r.teams = newTeamState()
	r.composerSaving = map[string]bool{}
	r.composerCatalogs = map[string]*composerCatalog{}

	cv := &viewmodel.Conversation{ID: session.ID.String(), Node: nodeID.String()}
	comp := r.App.Composer(cv.ID)
	r.projectTeamComposer(cv, comp, &composer.Context{}, "en")
	if comp.SelectTeam == nil {
		t.Fatal("SelectTeam was not bound")
	}

	// Replace the snapshot on the controller and attach a cleanup to a freshly
	// allocated snapshot observed by a second composer; after replacing the
	// snapshot again, the old snapshot must be collectable while comp.SelectTeam
	// stays retained on App.drafts.
	var collected atomic.Bool
	markerTeam := protocol.DefaultTeamProfile()
	markerTeam.ID, markerTeam.Name, markerTeam.Revision = protocol.TeamID{2}, "Beta", 1
	// Allocate a unique heap pointer inside the snapshot's Teams slice.
	markerModel := &protocol.RoleModel{Model: "marker-model"}
	runtime.AddCleanup(markerModel, func(flag *atomic.Bool) { flag.Store(true) }, &collected)
	markerTeam.Model = markerModel

	snap2 := protocol.DefaultSnapshot()
	snap2.Node, snap2.Cursor = nodeID, 2
	snap2.Teams = []protocol.TeamProfile{teamA, markerTeam}
	snap2.Sessions = []protocol.Session{session}
	applied := make(chan struct{}, 2)
	stopWatch := r.controller.OnChange(func() {
		select {
		case applied <- struct{}{}:
		default:
		}
	})
	defer stopWatch()

	updates <- protocol.Update{Kind: "snapshot", Data: snap2}
	for r.snapshot().Cursor < 2 {
		select {
		case <-applied:
		case <-time.After(2 * time.Second):
			t.Fatal("controller did not apply snapshot 2")
		}
	}
	r.projectTeamComposer(cv, comp, &composer.Context{}, "en")

	// Clear our local references and push snapshot 3 without markerModel.
	markerTeam.Model = nil
	markerModel = nil
	snap2 = protocol.Snapshot{}

	teamC := protocol.DefaultTeamProfile()
	teamC.ID, teamC.Name, teamC.Revision = protocol.TeamID{3}, "Gamma", 1
	snap3 := protocol.DefaultSnapshot()
	snap3.Node, snap3.Cursor = nodeID, 3
	snap3.Teams = []protocol.TeamProfile{teamA, teamC}
	snap3.Sessions = []protocol.Session{session}
	updates <- protocol.Update{Kind: "snapshot", Data: snap3}
	for r.snapshot().Cursor < 3 {
		select {
		case <-applied:
		case <-time.After(2 * time.Second):
			t.Fatal("controller did not apply snapshot 3")
		}
	}

	deadline := time.Now().Add(2 * time.Second)
	for !collected.Load() && time.Now().Before(deadline) {
		runtime.GC()
		time.Sleep(5 * time.Millisecond)
	}
	if !collected.Load() {
		t.Fatal("comp.SelectTeam closure retained a previous protocol.Snapshot across snapshot updates")
	}
	// Invoking SelectTeam resolves the current snapshot (teamC) dynamically.
	if !comp.SelectTeam(teamC.ID.String()) {
		t.Fatal("SelectTeam rejected a team from the current snapshot")
	}
	if state := r.teams.composers[cv.ID]; state == nil || state.selection == nil || state.selection.ID != teamC.ID {
		t.Fatalf("SelectTeam did not record current team selection: %+v", state)
	}
}

func TestPluginContributionUnmountsOnConversationSwitch(t *testing.T) {
	const entry = "dev.sailry.platform/desktop/contrib.js"
	nodeID, worktree := protocol.NodeID{1}, protocol.WorktreeID{2}
	sessionA, sessionB := protocol.SessionID{11}, protocol.SessionID{12}

	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "switch-contrib", strings.Repeat("a", 64), true
	info.Summary.Compatibility.Kind = "compatible"
	ext := protocol.DefaultPluginExtension()
	ext.Scope = protocol.PluginScopeHost
	desktop := protocol.DefaultPluginDesktopManifest()
	desktop.UiEntry = new(entry)
	ext.Desktop, info.Extension = &desktop, &ext
	if err := json.Unmarshal([]byte(`[{"id":"tokens","slot":"statistics","kind":"metric","label":{"label":"Tokens"}}]`), &ext.Ui); err != nil {
		t.Fatal(err)
	}

	sA := protocol.DefaultSession()
	sA.ID, sA.Worktree, sA.Revision = sessionA, worktree, 1
	sB := protocol.DefaultSession()
	sB.ID, sB.Worktree, sB.Revision = sessionB, worktree, 1

	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Plugins = nodeID, []protocol.PluginSummary{info.Summary}
	snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Main: true}}
	snapshot.Sessions = []protocol.Session{sA, sB}

	bundle := link.Response{Output: protocol.Output{Kind: "plugin_view", Data: protocol.PluginDesktopBundle{
		Package: packageReference(info.Summary), Entry: entry,
		Files: map[string]string{entry: `import {View, div} from "gpui-kit"; export default class extends View { render() { return div(); } }`},
	}}}
	source := &pluginViewTestNode{snapshot: snapshot, bundle: make(chan link.Response, 8)}
	root := pluginViewFixture(t, source)
	root.packages.infos[info.Summary.Name] = info

	active := &sessionA
	scopeFor := func(id protocol.SessionID) protocol.PluginContext {
		scope := protocol.DefaultPluginContext()
		scope.Surface.Kind, scope.Worktree, scope.Session = "composer", &worktree, &id
		return scope
	}
	tester := ui.NewTesterWith(func(view *ui.Context) {
		theme.Apply(view, "", theme.Light)
		root.mu.Lock()
		queued := root.queue
		root.queue = nil
		root.mu.Unlock()
		for _, apply := range queued {
			apply(view)
		}
		root.pluginViewsFrame(view)
		if active != nil {
			reg := root.pluginRegistry(scopeFor(*active))
			comp := root.App.Composer(active.String())
			root.projectPluginMetrics(comp, reg)
		}
		root.pluginViewsAfterFrame()
	}, 640, 400, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true}})
	tester.SetLocale("en")

	mountActive := func(id protocol.SessionID) *pluginMountedView {
		t.Helper()
		active = &id
		source.bundle <- bundle
		tester.Frame()
		tester.Frame()
		root.workers.Wait()
		tester.Frame()
		key := pluginMountKey{scope: pluginScopeIdentity(nodeID, scopeFor(id)), role: "contribution", name: info.Summary.Name}
		mounted := root.pluginViews.mounts[key]
		if mounted == nil || mounted.host == nil || mounted.mounted == nil {
			t.Fatalf("session %s did not mount contribution view: %+v", id, mounted)
		}
		return mounted
	}

	var prev *pluginMountedView
	for cycle := range 3 {
		mA := mountActive(sessionA)
		if prev != nil && (prev.ctx.Err() == nil || prev.host != nil || prev.runtime != nil) {
			t.Fatalf("cycle %d: previous session mount stayed alive after switch", cycle)
		}
		if len(root.pluginViews.registries) != 1 || len(root.pluginViews.mounts) != 1 {
			t.Fatalf("cycle %d: expected 1 active registry/mount for session A, got %d/%d", cycle, len(root.pluginViews.registries), len(root.pluginViews.mounts))
		}
		mB := mountActive(sessionB)
		if mA.ctx.Err() == nil || mA.host != nil || mA.runtime != nil {
			t.Fatalf("cycle %d: session A mount was not closed when switching to session B", cycle)
		}
		if compA := root.App.DraftComposer(sessionA.String()); compA != nil && compA.Metrics != nil {
			t.Fatalf("cycle %d: session A composer retained Metrics closure referencing closed registry", cycle)
		}
		if len(root.pluginViews.registries) != 1 || len(root.pluginViews.mounts) != 1 {
			t.Fatalf("cycle %d: expected 1 active registry/mount for session B, got %d/%d", cycle, len(root.pluginViews.registries), len(root.pluginViews.mounts))
		}
		prev = mB
	}

	// Leaving conversations altogether releases the last contribution mount.
	active = nil
	tester.Frame()
	if prev.ctx.Err() == nil || prev.host != nil || prev.runtime != nil || len(root.pluginViews.registries) != 0 || len(root.pluginViews.mounts) != 0 {
		t.Fatalf("leaving conversations retained registries=%d mounts=%d", len(root.pluginViews.registries), len(root.pluginViews.mounts))
	}
}

func TestPluginSettingsUnmountsOnSectionSwitch(t *testing.T) {
	const entry = "dev.sailry.platform/desktop/settings.js"
	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Digest = "settings-unmount", strings.Repeat("e", 64)
	info.Summary.Compatibility.Kind = "compatible"
	info.Extension = new(protocol.DefaultPluginExtension())
	info.Extension.Scope = protocol.PluginScopeHost
	info.Extension.SettingsPage = new(protocol.DefaultPluginDesktopSettings())
	info.Extension.SettingsPage.Navigation.Label = "Unmount settings"
	info.Extension.SettingsPage.Entry = new(entry)

	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Plugins = protocol.NodeID{1}, []protocol.PluginSummary{info.Summary}
	source := &pluginViewTestNode{snapshot: snapshot, bundle: make(chan link.Response, 2)}
	root := pluginViewFixture(t, source)
	root.packages.infos[info.Summary.Name] = info

	section := pluginSettingsSection(root, info)
	root.App.Navigate(shell.SettingsPage)
	tester := ui.NewTesterWith(func(view *ui.Context) {
		theme.Apply(view, "", theme.Light)
		root.mu.Lock()
		queued := root.queue
		root.queue = nil
		root.mu.Unlock()
		for _, apply := range queued {
			apply(view)
		}
		root.pluginViewsFrame(view)
		if root.App.Page == shell.SettingsPage {
			ui.Column(view).Fill().Children(func() { settings.Page(view, root.App.Settings, root.App.Data, settings.Look{}) })
		}
		root.pluginViewsAfterFrame()
	}, 800, 500, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true}})
	tester.SetLocale("en")

	tester.Frame()
	root.App.Settings.Open(section)
	source.bundle <- link.Response{Output: protocol.Output{Kind: "plugin_view", Data: protocol.PluginDesktopBundle{
		Package: packageReference(info.Summary), Entry: entry,
		Files: map[string]string{entry: `import {View, div} from "gpui-kit"; export default class extends View { render() { return div().child("Settings view"); } }`},
	}}}
	tester.Frame()
	root.workers.Wait()
	tester.Frame()

	key := pluginMountKey{scope: pluginScopeKey{node: snapshot.Node, surface: "settings"}, role: "settings", name: info.Summary.Name}
	mounted := root.pluginViews.mounts[key]
	if mounted == nil || mounted.host == nil {
		t.Fatal("settings view did not mount")
	}

	// Switching to another settings section unmounts the previous plugin settings VM.
	root.App.Settings.Open(settings.General)
	tester.Frame()
	if mounted.ctx.Err() == nil || mounted.host != nil || mounted.runtime != nil || len(root.pluginViews.mounts) != 0 {
		t.Fatalf("switching settings section retained mount: mounts=%d host=%v", len(root.pluginViews.mounts), mounted.host != nil)
	}
}

func TestPruneClosedChildTabsAndSecondaryCaches(t *testing.T) {
	nodeID := protocol.NodeID{1}
	childID := protocol.SessionID{42}
	liveSession := protocol.DefaultSession()
	liveSession.ID, liveSession.Worktree, liveSession.Revision = protocol.SessionID{1}, protocol.WorktreeID{2}, 1

	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Revision, provider.Enabled = protocol.ProviderID{5}, 3, true
	removedProvider := protocol.ProviderID{6}

	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = nodeID
	snapshot.Sessions = []protocol.Session{liveSession}
	snapshot.Providers = []protocol.ConversationProvider{provider}

	r := pluginViewFixture(t, &pluginViewTestNode{snapshot: snapshot})
	r.teams = newTeamState()
	r.providerDraftBases = map[string]protocol.ConversationProvider{}

	// Simulate an open child tab that allocated a readonly draft composer, then closed.
	_ = r.App.Composer(childID.String())
	r.teams.composers[childID.String()] = &teamComposer{composer: r.App.DraftComposer(childID.String())}
	r.teams.childScopes.tabs = map[childTabKey]childTabScope{
		{selected: liveSession.ID.String(), id: childID.String()}: {node: nodeID},
	}
	if r.App.HasPanelTab(panel.Tab{Kind: panel.Subagent, ID: childID.String()}) {
		t.Fatal("child tab should not be open")
	}
	r.pruneClosedChildTabs()
	if r.App.DraftComposer(childID.String()) != nil || len(r.teams.childScopes.tabs) != 0 || r.teams.composers[childID.String()] != nil {
		t.Fatalf("closed child tab retained composer or scope: draft=%v tabs=%d team=%v",
			r.App.DraftComposer(childID.String()) != nil, len(r.teams.childScopes.tabs), r.teams.composers[childID.String()] != nil)
	}

	// Simulate stale team composer + superseded/removed providerDraftBases.
	goneSession := protocol.SessionID{99}.String()
	r.teams.composers[goneSession] = &teamComposer{}
	r.teams.composers[liveSession.ID.String()] = &teamComposer{}
	r.providerDraft(settings.ProviderDraft{Channel: viewmodel.Channel{ID: provider.ID.String()}, ExpectedRevision: 2})
	r.providerDraft(settings.ProviderDraft{Channel: viewmodel.Channel{ID: provider.ID.String()}, ExpectedRevision: 3})
	if _, stale := r.providerDraftBases[provider.ID.String()+"/2"]; stale {
		t.Fatal("providerDraft retained superseded revision for the same channel")
	}
	r.providerDraftBases[removedProvider.String()+"/1"] = protocol.DefaultConversationProvider()
	r.providerDraftBases[protocol.ProviderID{7}.String()+"/0"] = protocol.DefaultConversationProvider()

	r.pruneSessions(snapshot)
	if _, exists := r.teams.composers[goneSession]; exists {
		t.Fatal("pruneSessions retained teams.composers for a removed session")
	}
	if _, exists := r.teams.composers[liveSession.ID.String()]; !exists {
		t.Fatal("pruneSessions dropped teams.composers for a live session")
	}
	if len(r.providerDraftBases) != 1 || r.providerDraftBases[provider.ID.String()+"/3"].Revision != 3 {
		t.Fatalf("pruneSessions did not prune stale providerDraftBases: %+v", r.providerDraftBases)
	}
}
