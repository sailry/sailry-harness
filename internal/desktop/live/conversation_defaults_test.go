package live

import (
	"path/filepath"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func conversationDefaultsSnapshot() protocol.Snapshot {
	snapshot := selectionSnapshot("Existing conversation")
	model := protocol.DefaultConversationModel()
	model.ID = "shared-model-name"
	for _, id := range []protocol.ProviderID{{1}, {2}} {
		snapshot.Providers = append(snapshot.Providers, protocol.ConversationProvider{ID: id, Enabled: true, DefaultModel: model.ID, Models: []protocol.ConversationModel{model}})
	}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Mode, config.Permission = snapshot.Providers[0].ID, model.ID, protocol.WorkModeCode, protocol.PermissionAsk
	snapshot.Defaults.Config, snapshot.Sessions[0].Config = &config, config
	team := protocol.DefaultTeamProfile()
	team.ID, team.Revision, team.Name, team.Key = protocol.TeamID{3}, 1, "Remembered team", "remembered"
	snapshot.Teams = []protocol.TeamProfile{team}
	return snapshot
}

func defaultsRuntime(t *testing.T, snapshot protocol.Snapshot, prefs *preferences.Store) *Runtime {
	t.Helper()
	controller := client.New(&conversationTestTransport{snapshot: snapshot})
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = controller.Close() })
	return &Runtime{controller: controller, Preferences: prefs, App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}), composerBases: map[*composer.State]protocol.SessionConfig{}}
}

func TestNewConversationSelectionPersistsWithoutSend(t *testing.T) {
	for _, leadOverride := range []bool{false, true} {
		t.Run(map[bool]string{false: "team only", true: "team lead override"}[leadOverride], func(t *testing.T) {
			snapshot := conversationDefaultsSnapshot()
			wantProvider := snapshot.Providers[0].ID
			if leadOverride {
				wantProvider = snapshot.Providers[1].ID
				snapshot.Teams[0].Model = &protocol.RoleModel{Provider: wantProvider, Model: snapshot.Providers[1].DefaultModel}
			}
			f := selectionWindow(t, snapshot)
			r := f.r
			r.newDraft(nil)
			f.window.Frame()
			draft := r.App.Sidebar.Selected
			comp := r.App.Composer(draft)
			if comp.SelectTeam == nil || !comp.SelectTeam(snapshot.Teams[0].ID.String()) {
				t.Fatal("team selection was not accepted")
			}
			f.window.Frame()
			r.workers.Wait()
			reopened := preferences.Open(r.Preferences.Path)
			if !reopened.Readable || reopened.Err != nil {
				t.Fatal("preferences did not survive reopen", reopened.Err)
			}
			saved := reopened.Data.Composer
			if saved == nil || saved.Team != snapshot.Teams[0].ID.String() || saved.Model == nil || saved.Model.Source != snapshot.Node || saved.Model.Provider != wantProvider || saved.Model.Model != snapshot.Providers[0].DefaultModel {
				t.Fatalf("selection was not saved without sending: %+v", saved)
			}
			restarted := defaultsRuntime(t, snapshot, reopened)
			next := composer.New("")
			restarted.initializeDraftComposer(next)
			if next.Team != saved.Team || next.Model != modelKey(wantProvider, saved.Model.Model) {
				t.Fatalf("restarted draft did not use the reopened selection: team=%q model=%q", next.Team, next.Model)
			}
			comp.Draft = "Keep this input"
			r.newDraft(nil)
			if r.App.Sidebar.Selected != draft || r.App.Composer(draft) != comp || comp.Draft != "Keep this input" || comp.Team != saved.Team {
				t.Fatal("New session replaced an existing unsent draft")
			}

			comp.Model = modelKey(snapshot.Providers[1].ID, snapshot.Providers[1].DefaultModel)
			comp.SelectTeam("")
			f.window.Frame()
			r.workers.Wait()
			saved = preferences.Open(r.Preferences.Path).Data.Composer
			if saved == nil || saved.Team != "" || saved.Model.Provider != snapshot.Providers[1].ID {
				t.Fatal("model-only selection did not replace the remembered team", saved)
			}
		})
	}
}

func TestDraftSelectionFallbackUsesExecutionNodeInventory(t *testing.T) {
	for _, name := range []string{"available", "another node", "missing provider", "historical capture", "missing model", "disabled provider", "remote credential", "missing team", "default team", "legacy model only", "legacy team without model"} {
		t.Run(name, func(t *testing.T) {
			snapshot := conversationDefaultsSnapshot()
			chosen := snapshot.Providers[1]
			prefs := preferences.Open(filepath.Join(t.TempDir(), "preferences.json"))
			prefs.Data.Composer = &preferences.Composer{Mode: protocol.WorkModePlan, Permission: protocol.PermissionProject, Team: snapshot.Teams[0].ID.String(), Model: &preferences.Model{Source: snapshot.Node, Provider: chosen.ID, Model: chosen.DefaultModel}}
			wantProvider, wantTeam := chosen.ID, prefs.Data.Composer.Team
			switch name {
			case "another node":
				prefs.Data.Composer.Model.Source = protocol.NodeID{9}
			case "missing provider":
				prefs.Data.Composer.Model.Provider = protocol.ProviderID{9}
			case "historical capture":
				snapshot.Providers = snapshot.Providers[:1]
				snapshot.Sessions[0].Profile = &protocol.SessionProfile{Source: snapshot.Node, Provider: chosen}
			case "missing model":
				prefs.Data.Composer.Model.Model = "removed"
			case "disabled provider":
				snapshot.Providers[1].Enabled = false
			case "remote credential":
				snapshot.Providers[1].Credential = &protocol.CredentialRef{Node: protocol.NodeID{9}, ID: protocol.CredentialID{1}}
			case "missing team":
				snapshot.Teams = nil
				wantTeam = ""
			case "default team":
				prefs.Data.Composer.Team, wantTeam = defaultTeamID, defaultTeamID
			case "legacy model only":
				prefs.Data.Composer.Team, wantTeam = "", ""
			case "legacy team without model":
				prefs.Data.Composer.Model = nil
			}
			if name != "available" && name != "missing team" && name != "default team" && name != "legacy model only" {
				wantProvider, wantTeam = snapshot.Providers[0].ID, ""
			}
			before := encoded(prefs.Data.Composer)
			r := defaultsRuntime(t, snapshot, prefs)
			comp := composer.New("stale")
			comp.Team = defaultTeamID
			r.initializeDraftComposer(comp)
			if comp.Model != modelKey(wantProvider, chosen.DefaultModel) || comp.Team != wantTeam {
				t.Fatalf("fallback crossed selection scope: team=%q model=%q", comp.Team, comp.Model)
			}
			if before != encoded(prefs.Data.Composer) {
				t.Fatal("initializing a draft rewrote unavailable saved preferences")
			}
			config, _, err := r.composerConfig(comp, composer.Message{Model: comp.Model, Mode: comp.Mode, Permission: comp.Permission})
			if err != nil || config.Credential != nil && config.Credential.Node != snapshot.Node {
				t.Fatal("draft borrowed a credential from another Node", err)
			}
		})
	}
}

func TestRememberedDraftDoesNotRewriteExistingSession(t *testing.T) {
	snapshot := conversationDefaultsSnapshot()
	f := selectionWindow(t, snapshot)
	r := f.r
	saved := &preferences.Composer{Mode: protocol.WorkModePlan, Permission: protocol.PermissionProject, Team: snapshot.Teams[0].ID.String(), Model: &preferences.Model{Source: snapshot.Node, Provider: snapshot.Providers[1].ID, Model: snapshot.Providers[1].DefaultModel}}
	r.Preferences.Data.Composer = saved
	existing := snapshot.Sessions[0]
	r.OpenSession(existing.ID.String())
	f.window.Frame()
	comp := r.App.Composer(existing.ID.String())
	if comp.Model != modelKey(existing.Config.Provider, existing.Config.Model) || comp.Team != selectedTeam(existing.Roles) {
		t.Fatal("a remembered selection replaced the existing session capture")
	}
	if !reflect.DeepEqual(saved, r.Preferences.Data.Composer) {
		t.Fatal("opening an existing session replaced the new-conversation seed")
	}
	f.source.mu.Lock()
	defer f.source.mu.Unlock()
	for _, request := range f.source.requests {
		if request.Command.Kind == "set_session_config" || request.Command.Kind == "set_session_team" {
			t.Fatalf("opening an existing session issued %s", request.Command.Kind)
		}
	}
}

func TestDraftHostChangeReseedsTeamAndModelTogether(t *testing.T) {
	snapshot := conversationDefaultsSnapshot()
	prefs := preferences.Open(filepath.Join(t.TempDir(), "preferences.json"))
	prefs.Data.Composer = &preferences.Composer{Team: snapshot.Teams[0].ID.String(), Model: &preferences.Model{Source: snapshot.Node, Provider: snapshot.Providers[1].ID, Model: snapshot.Providers[1].DefaultModel}}
	local := defaultsRuntime(t, snapshot, prefs)
	remoteSnapshot := conversationDefaultsSnapshot()
	remoteSnapshot.Node = protocol.NodeID{2}
	remote := defaultsRuntime(t, remoteSnapshot, prefs)
	local.bindings = map[string]*Runtime{remoteSnapshot.Node.String(): remote}
	remote.parent = local
	cv := &viewmodel.Conversation{ID: "draft/host", Node: snapshot.Node.String()}
	comp := composer.New("")
	local.initializeDraftComposer(comp)
	comp.Host, comp.Draft = remoteSnapshot.Node.String(), "Keep draft text"
	if owner := local.conversationRuntime(cv, comp); owner != remote || comp.Team != "" || comp.Model != modelKey(remoteSnapshot.Providers[0].ID, remoteSnapshot.Providers[0].DefaultModel) || comp.Draft != "Keep draft text" {
		t.Fatal("changing the draft host retained another Node's selection")
	}
	comp.Host = snapshot.Node.String()
	if owner := local.conversationRuntime(cv, comp); owner != local || comp.Team != prefs.Data.Composer.Team || comp.Model != modelKey(snapshot.Providers[1].ID, snapshot.Providers[1].DefaultModel) {
		t.Fatal("returning to the saved Node did not restore its selection")
	}
}

func TestFreshConversationDraftKeepsInputAndSeedsLatestSelection(t *testing.T) {
	snapshot := conversationDefaultsSnapshot()
	f := selectionWindow(t, snapshot)
	r, app := f.r, f.r.App
	r.newDraft(nil)
	f.window.Frame()
	before := app.Sidebar.Selected
	first := app.Composer(before)
	first.Draft = "Keep the first draft"
	if first.SelectTeam == nil || !first.SelectTeam(snapshot.Teams[0].ID.String()) {
		t.Fatal("team selection was unavailable")
	}
	first.Model = modelKey(snapshot.Providers[1].ID, snapshot.Providers[1].DefaultModel)
	r.newConversationDraft(nil)
	after := app.Sidebar.Selected
	if before == after || app.Data.Find(before) == nil || app.Composer(before) != first || first.Draft != "Keep the first draft" {
		t.Fatal("fresh draft creation reused or discarded the typed draft")
	}
	f.window.Frame()
	second := app.Composer(after)
	if second == first || second.Draft != "" || second.Model != first.Model || second.Team != first.Team {
		t.Fatal("fresh draft did not independently seed the latest selection")
	}
	second.Draft = "Keep the second draft"
	r.newConversationDraft(nil)
	third := app.Sidebar.Selected
	f.window.Frame()
	if third == before || third == after || app.Data.Find(before) == nil || app.Data.Find(after) == nil ||
		first.Draft != "Keep the first draft" || second.Draft != "Keep the second draft" {
		t.Fatal("repeated explicit creation lost a draft or reused an identifier")
	}
	current := app.Composer(third)
	current.Draft = "Reuse this location draft"
	r.newDraft(nil)
	if app.Sidebar.Selected != third || app.Composer(third) != current || current.Draft != "Reuse this location draft" {
		t.Fatal("ordinary New session no longer reused the location's draft")
	}
}

func TestFreshProjectlessDraftRetainsExecutionNode(t *testing.T) {
	snapshot := conversationDefaultsSnapshot()
	r := defaultsRuntime(t, snapshot, preferences.Open(filepath.Join(t.TempDir(), "preferences.json")))
	remote := protocol.NodeID{9}.String()
	existing := &viewmodel.Conversation{ID: "draft/existing", Node: remote}
	r.App.Data.Unassigned = []*viewmodel.Conversation{existing}
	r.App.Page, r.App.Sidebar.Selected = shell.ConversationPage, existing.ID
	r.newConversationDraft(nil)
	created := r.App.Data.Find(r.App.Sidebar.Selected)
	if created == nil || created == existing || created.Node != remote || r.App.Composer(created.ID).Host != remote || r.App.Data.Find(existing.ID) != existing {
		t.Fatal("fresh projectless draft switched execution Node or replaced the source draft")
	}
}
