package live

import (
	"reflect"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestTeamsProjection(t *testing.T) {
	t.Run("editor retains the original revision and non-form fields", func(t *testing.T) {
		role := protocol.DefaultRoleProfile()
		role.ID, role.Revision, role.Key, role.Name = protocol.RoleID{1}, 3, "review", "Review"
		budget := uint32(9)
		features := map[string]string{"speed": "fast"}
		role.Model = &protocol.RoleModel{Provider: protocol.ProviderID{2}, Model: "vendor/model", Features: &features}
		role.MaxTurns = &budget
		role.Skills = []string{"fixture:review"}
		role.Appearance = &protocol.ProjectsAppearance{Icon: "code", Color: "blue"}
		tools := []protocol.PluginConversationTool{}
		role.Tools = &tools
		r := &Runtime{teams: &teamState{roles: map[string]protocol.RoleProfile{profileKey(role.ID.String(), 3): role}, teams: map[string]protocol.TeamProfile{}}}
		draft := roleView(role)
		draft.Name = "Code review"
		updated := role
		updated.Revision = 4
		updated.MaxTurns = new(uint32(12))
		r.teams.roles[profileKey(role.ID.String(), 4)] = updated
		got, err := r.roleDraft(draft)
		if err != nil || got.Revision != 3 || got.Name != "Code review" || !reflect.DeepEqual(got.MaxTurns, role.MaxTurns) || !reflect.DeepEqual(got.Skills, role.Skills) || !reflect.DeepEqual(got.Tools, role.Tools) || !reflect.DeepEqual(got.Appearance, role.Appearance) || !reflect.DeepEqual(got.Model.Features, role.Model.Features) {
			t.Fatalf("editor changed immutable base fields: %#v, %v", got, err)
		}
		team := protocol.DefaultTeamProfile()
		team.ID, team.Revision, team.Key, team.Name = protocol.TeamID{1}, 7, "stable-key", "Team"
		team.Appearance = role.Appearance
		team.Model = role.Model
		team.Members = []protocol.RoleID{role.ID}
		r.teams.teams[profileKey(team.ID.String(), team.Revision)] = team
		td := teamView(team)
		td.Name = "Renamed team"
		tg, err := r.teamDraft(td)
		if err != nil || tg.Key != team.Key || tg.Revision != 7 || !reflect.DeepEqual(tg.Appearance, team.Appearance) || !reflect.DeepEqual(tg.Model.Features, team.Model.Features) {
			t.Fatal("team rename discarded captured fields", tg, err)
		}
		if got := teamKey("123 Team", []protocol.TeamProfile{{Key: "team-123-team"}}); got != "team-123-team-2" {
			t.Fatal(got)
		}
		if got := teamKey("中文", nil); got != "team" {
			t.Fatal(got)
		}
	})
	t.Run("an older settings read does not lower the revision held", func(t *testing.T) {
		r := &Runtime{App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}), teams: &teamState{}}
		field := &r.App.Settings.Concurrency
		r.acceptTeamRead(protocol.TeamSettings{Revision: 5, ChildConcurrency: 4})
		r.acceptTeamRead(protocol.TeamSettings{Revision: 4, ChildConcurrency: 2})
		if field.Revision != 5 || field.Saved != 4 || field.Draft != 4 || r.teams.observed != 5 {
			t.Fatalf("an older read left %+v, observed %d", *field, r.teams.observed)
		}
		if r.acceptTeamRead(protocol.TeamSettings{Revision: 6, ChildConcurrency: 3}); field.Revision != 6 || field.Saved != 3 {
			t.Fatalf("a newer read left %+v", *field)
		}
	})

	t.Run("default roster and explicit member overrides", func(t *testing.T) {
		provider, role := protocol.ProviderID{1}, protocol.RoleID{2}
		model := provider.String() + "/model"
		models := []composer.Model{{ID: model, Reasoning: true, Efforts: []string{"low", "high"}, DefaultEffort: "low"}}
		comp := composer.New(model)
		comp.Team = defaultTeamID
		comp.Effort = 0
		profile := protocol.RoleProfile{ID: role, Name: "Review"}
		roles := protocol.DefaultRoleSnapshot()
		roles.Profiles = []protocol.RoleProfile{profile}
		comp.MemberModels = map[string]*composer.MemberModel{role.String(): memberChoice(comp, profile, roles, models)}
		selected, err := selectedMemberModels(comp, models, roles)
		if err != nil || len(selected) != 0 {
			t.Fatal("inherited controls became an override", selected, err)
		}
		comp.MemberModels[role.String()].Effort = 1
		comp.MemberModels[role.String()].Overridden = true
		selected, err = selectedMemberModels(comp, models, roles)
		if err != nil || len(selected) != 1 || selected[role].Effort == nil || selected[role].Effort.Kind != "high" || selected[role].Features != nil || comp.Model != model || comp.Effort != 0 {
			t.Fatal("member controls changed the lead", selected, err)
		}
		comp.MemberModels[(protocol.RoleID{3}).String()] = &composer.MemberModel{Model: "removed-model", Overridden: true}
		selected, err = selectedMemberModels(comp, models, roles)
		if err != nil || len(selected) != 1 {
			t.Fatal("an absent member blocked the captured roster", selected, err)
		}
		catalog := teamCatalog(protocol.Snapshot{Roles: []protocol.RoleProfile{profile}}, protocol.RoleSnapshot{})
		if len(catalog) != 1 || catalog[0].ID != defaultTeamID || len(catalog[0].Members) != 1 {
			t.Fatal(catalog)
		}
		comp.Team = ""
		selected, err = selectedMemberModels(comp, models, roles)
		if err != nil || len(selected) != 0 {
			t.Fatal("model-only execution retained member overrides")
		}
	})
	t.Run("member edits preserve optional inheritance", func(t *testing.T) {
		provider, role := protocol.ProviderID{1}, protocol.RoleID{2}
		model := provider.String() + "/model"
		models := []composer.Model{{ID: model, Reasoning: true, Efforts: []string{"low", "high"}, DefaultEffort: "low", Features: []composer.Feature{{ID: "speed", Options: []composer.FeatureOption{{ID: "fast"}}}}}}
		original := protocol.RoleModel{Provider: provider, Model: "model"}
		for _, row := range []struct {
			name      string
			override  bool
			edit      string
			wantModel protocol.RoleModel
		}{
			{name: "untouched override", override: true, wantModel: original},
			{name: "override effort", override: true, edit: "effort", wantModel: protocol.RoleModel{Provider: provider, Model: "model", Effort: &protocol.Effort{Kind: "high"}}},
			{name: "role effort", edit: "effort", wantModel: protocol.RoleModel{Provider: provider, Model: "model", Effort: &protocol.Effort{Kind: "high"}}},
			{name: "override features", override: true, edit: "features", wantModel: protocol.RoleModel{Provider: provider, Model: "model", Features: new(map[string]string{})}},
		} {
			t.Run(row.name, func(t *testing.T) {
				comp := composer.New(model)
				comp.Team = defaultTeamID
				comp.Features = map[string]string{"speed": "fast", "unsupported": "choice"}
				roles := protocol.DefaultRoleSnapshot()
				profile := protocol.RoleProfile{ID: role, Model: &original}
				roles.Profiles = []protocol.RoleProfile{profile}
				if row.override {
					roles.Models[role] = original
				}
				choice := memberChoice(comp, profile, roles, models)
				if !reflect.DeepEqual(choice.Features, map[string]string{"speed": "fast"}) {
					t.Fatal("member inherited unsupported features", choice.Features)
				}
				choice.Overridden = true
				switch row.edit {
				case "effort":
					choice.Effort = 1
				case "features":
					choice.Features = map[string]string{}
					choice.InheritFeatures = false
				}
				comp.MemberModels = map[string]*composer.MemberModel{role.String(): choice}
				selected, err := selectedMemberModels(comp, models, roles)
				if err != nil || len(selected) != 1 || !reflect.DeepEqual(selected[role], row.wantModel) {
					t.Fatal("member edit changed untouched optional fields", selected, err)
				}
			})
		}
	})
	t.Run("explicit team selection compares captured revisions", func(t *testing.T) {
		role := protocol.RoleProfile{ID: protocol.RoleID{1}, Revision: 2, Name: "Review"}
		team := protocol.TeamProfile{ID: protocol.TeamID{1}, Revision: 3, Members: []protocol.RoleID{role.ID}}
		for _, row := range []struct {
			name        string
			selected    string
			captured    protocol.RoleSnapshot
			wantChanged bool
			wantMembers int
		}{
			{name: "unchanged stored team", selected: team.ID.String(), captured: protocol.RoleSnapshot{Execution: protocol.ConversationExecutionTeam, Team: &protocol.TeamReference{ID: team.ID, Revision: team.Revision}, Profiles: []protocol.RoleProfile{role}}, wantMembers: 1},
			{name: "new stored team revision", selected: team.ID.String(), captured: protocol.RoleSnapshot{Execution: protocol.ConversationExecutionTeam, Team: &protocol.TeamReference{ID: team.ID, Revision: 2}, Profiles: []protocol.RoleProfile{role}}, wantChanged: true, wantMembers: 1},
			{name: "new role revision", selected: team.ID.String(), captured: protocol.RoleSnapshot{Execution: protocol.ConversationExecutionTeam, Team: &protocol.TeamReference{ID: team.ID, Revision: team.Revision}, Profiles: []protocol.RoleProfile{{ID: role.ID, Revision: 1}}}, wantChanged: true, wantMembers: 1},
			{name: "frozen empty default team", selected: defaultTeamID, captured: protocol.DefaultRoleSnapshot(), wantChanged: true},
			{name: "frozen empty stored team", selected: team.ID.String(), captured: protocol.RoleSnapshot{Execution: protocol.ConversationExecutionTeam, Team: &protocol.TeamReference{ID: team.ID, Revision: team.Revision}}, wantChanged: true},
			{name: "unchanged default team", selected: defaultTeamID, captured: protocol.RoleSnapshot{Execution: protocol.ConversationExecutionTeam, Profiles: []protocol.RoleProfile{role}}, wantMembers: 1},
		} {
			t.Run(row.name, func(t *testing.T) {
				snapshot := protocol.Snapshot{Roles: []protocol.RoleProfile{role}, Teams: []protocol.TeamProfile{team}}
				if changed := teamCatalogChanged(row.selected, snapshot, row.captured); changed != row.wantChanged {
					t.Fatal("incorrect explicit recapture decision", changed)
				}
				for _, shown := range teamCatalog(snapshot, row.captured) {
					if shown.ID == row.selected && len(shown.Members) != row.wantMembers {
						t.Fatal("selected menu replaced the frozen roster", shown.Members)
					}
				}
			})
		}
	})
	t.Run("skills use enabled exact package versions", func(t *testing.T) {
		reference := protocol.PluginReference{Name: "fixture", Digest: "captured", SettingsRevision: 7}
		snapshot := protocol.Snapshot{Plugins: []protocol.PluginSummary{{Name: "fixture", Digest: "current", SettingsRevision: 9, Enabled: true}, {Name: "disabled", Enabled: false}}}
		packages := skillPackages(snapshot, &protocol.PluginConversationBinding{Package: reference})
		if len(packages) != 1 || packages[0] != reference {
			t.Fatal("assistant replaced its captured package", packages)
		}
		name := "Review"
		info := protocol.PluginInfo{Summary: protocol.PluginSummary{Name: reference.Name, Digest: reference.Digest, SettingsRevision: reference.SettingsRevision}, Skills: []protocol.PluginSkill{{Name: "review", DisplayName: &name, DisplayLocales: map[string]string{"zh-CN": "审阅"}}}}
		rows := skillRows(reference, info, "zh-CN")
		if len(rows) != 1 || rows[0].ID != "fixture:review" || rows[0].Label != "审阅" {
			t.Fatal(rows)
		}
		info.Summary.Digest = "other"
		if rows := skillRows(reference, info, "en"); len(rows) != 0 {
			t.Fatal("different package digest contributed skills")
		}
	})
	t.Run("skill failures retry only after picker reopen", func(t *testing.T) {
		failed := protocol.PluginReference{Name: "failed", Digest: "captured", SettingsRevision: 1}
		inflight := protocol.PluginReference{Name: "inflight", Digest: "captured", SettingsRevision: 1}
		other := protocol.PluginReference{Name: "other", Digest: "captured", SettingsRevision: 1}
		state := &teamState{skillReads: map[protocol.PluginReference]bool{failed: true, inflight: true, other: true}, skillFailures: map[protocol.PluginReference]bool{failed: true, other: true}}
		comp := composer.New("fixture")
		opened := 0
		comp.SetReferencePickerOpen(func() {
			opened++
			state.retrySkillReads([]protocol.PluginReference{failed, inflight})
		})
		context := &composer.Context{Models: []composer.Model{{ID: "fixture", Name: "Fixture"}}}
		actions := &pickerViewActions{}
		tester := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			ui.Column(c).Fill().Children(func() { composer.View(c, comp, context, actions) })
		}, 800, 500)
		tester.SetLocale("en")
		tester.Frame()
		tester.Frame()
		if opened != 0 || !state.skillReads[failed] {
			t.Fatal("ordinary frame retried failed skill metadata")
		}
		if err := tester.Click(i18n.In("en", "composer")); err != nil {
			t.Fatal(err)
		}
		tester.Type("@")
		tester.Frame()
		if opened != 1 || state.skillReads[failed] || !state.skillReads[inflight] || !state.skillReads[other] || !state.skillFailures[other] {
			t.Fatal("picker open changed unrelated or in-flight requests", opened, state)
		}
		state.skillReads[failed], state.skillFailures[failed] = true, true
		tester.Frame()
		tester.Frame()
		if opened != 1 || !state.skillReads[failed] {
			t.Fatal("visible picker retried a failed request automatically")
		}
		tester.Key(0, ui.KeyEscape)
		tester.Frame()
		tester.Type(" @")
		tester.Frame()
		if opened != 2 || state.skillReads[failed] || !state.skillReads[inflight] {
			t.Fatal("explicit picker reopen did not retry only the failed request", opened, state)
		}
	})
	t.Run("child titles use their exact canonical origin", func(t *testing.T) {
		name, title, blank := "Review", "Inspect files", "  "
		role := protocol.RoleID{1}
		turn := protocol.TurnID{1}
		for _, row := range []struct {
			name       string
			role       *protocol.RoleID
			roleName   *string
			title      *string
			originTurn protocol.TurnID
			want       string
		}{
			{name: "task title", title: &title, originTurn: turn, want: title},
			{name: "role and task title", role: &role, roleName: &name, title: &title, originTurn: turn, want: name + " " + title},
			{name: "task title without role ownership", roleName: &name, title: &title, originTurn: turn, want: title},
			{name: "role only", role: &role, roleName: &name, originTurn: turn, want: name},
			{name: "localized fallback", originTurn: turn, want: i18n.In("zh-CN", "chat_child")},
			{name: "blank metadata", roleName: &blank, title: &blank, originTurn: turn, want: i18n.In("zh-CN", "chat_child")},
			{name: "another turn", title: &title, originTurn: protocol.TurnID{2}, want: i18n.In("zh-CN", "chat_child")},
		} {
			t.Run(row.name, func(t *testing.T) {
				call := protocol.DefaultConversationPartToolCallPayload()
				call.Name = "spawn_agent"
				call.Summary = &protocol.ToolDetailSummary{Title: row.title}
				page := protocol.ConversationPage{Entries: []protocol.ConversationEntry{{ID: "spawn", Turn: turn, Parts: []protocol.ConversationPart{{Kind: "text", Data: "Earlier part"}, {Kind: "tool_call", Data: call}}}}}
				child := protocol.ConversationChild{Run: protocol.ConversationRun{Session: protocol.SessionID{9}}, Origin: protocol.Delegation{Turn: row.originTurn, Entry: "spawn", Index: 1, Role: row.role}, Name: row.roleName}
				if got := childTitle(&page, child, "zh-CN"); got != row.want {
					t.Fatalf("child title=%q, want %q", got, row.want)
				}
			})
		}
	})
	t.Run("capsule watches only children waiting on approval", func(t *testing.T) {
		type row struct {
			status  protocol.ConversationStatus
			waiting *protocol.ActivityWaiting
		}
		var rows []row
		for _, status := range []protocol.ConversationStatus{protocol.ConversationStatusQueued, protocol.ConversationStatusRunning, protocol.ConversationStatusStopping} {
			rows = append(rows, row{status, nil}, row{status, new(protocol.ActivityWaitingInput)}, row{status, new(protocol.ActivityWaitingApproval)})
		}
		for _, row := range rows {
			status, name := row.status, "working"
			if row.waiting != nil {
				name = string(*row.waiting)
			}
			t.Run(string(status)+"/"+name, func(t *testing.T) {
				node, parent, childID, worktree := protocol.NodeID{1}, protocol.SessionID{1}, protocol.SessionID{2}, protocol.WorktreeID{1}
				parentTurn := protocol.TurnID{1}
				origin := &protocol.Delegation{Session: parent, Turn: parentTurn, Entry: "spawn"}
				config := protocol.DefaultSessionConfig()
				config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
				config.Model = "fixture"
				snapshot := protocol.DefaultSnapshot()
				snapshot.Node = node
				snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Main: true}}
				snapshot.Sessions = []protocol.Session{{ID: parent, Revision: 1, Worktree: worktree, Config: config, Roles: protocol.DefaultRoleSnapshot()}, {ID: childID, Revision: 1, Worktree: worktree, Config: config, Roles: protocol.DefaultRoleSnapshot(), Delegation: &origin}}
				snapshot.Sessions[1].Activity.Waiting = row.waiting
				history := protocol.DefaultConversationSnapshot()
				history.Node, history.Page.Session, history.Page.Revision = node, parent, 1
				history.Page.Runs = []protocol.ConversationRun{{Session: parent, Worktree: worktree, Turn: parentTurn, Sequence: 1, Revision: 1, Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusRunning}}
				call := protocol.DefaultConversationPartToolCallPayload()
				call.Name, call.Summary = "spawn_agent", &protocol.ToolDetailSummary{}
				history.Page.Entries = []protocol.ConversationEntry{{ID: "spawn", Sequence: 1, Turn: parentTurn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "tool_call", Data: call}}}}
				history.Page.Children = []protocol.ConversationChild{{Run: protocol.ConversationRun{Session: childID, Worktree: worktree, Turn: protocol.TurnID{2}, Sequence: 2, Revision: 1, Kind: protocol.ConversationRunKindTask, Status: status}, Origin: *origin}}
				controller := client.New(&conversationTestTransport{snapshot: snapshot, conversation: history})
				if err := controller.Start(t.Context()); err != nil {
					t.Fatal(err)
				}
				defer controller.Close()
				changed := make(chan struct{}, 1)
				release, err := controller.WatchConversation(t.Context(), parent, func() {
					select {
					case changed <- struct{}{}:
					default:
					}
				})
				if err != nil {
					t.Fatal(err)
				}
				defer release()
				for {
					state, ready := controller.Conversation(parent)
					if ready && state.Err != nil {
						t.Fatal(state.Err)
					}
					if ready && state.Connected {
						break
					}
					select {
					case <-changed:
					case <-t.Context().Done():
						t.Fatal(t.Context().Err())
					}
				}
				runtime := &Runtime{controller: controller, ctx: t.Context(), conversationSeen: map[protocol.SessionID]bool{}, conversationWatches: map[protocol.SessionID]func(){childID: func() {}}}
				context := &composer.Context{}
				runtime.projectChildComposer(&viewmodel.Conversation{ID: parent.String()}, composer.New(""), context, "en")
				want := 0
				if status == protocol.ConversationStatusRunning {
					want = 1
				}
				watched := row.waiting != nil && *row.waiting == protocol.ActivityWaitingApproval
				if len(context.Activity.Children) != want || runtime.conversationSeen[childID] != watched {
					t.Fatal("capsule rows or child observation changed", context.Activity.Children, runtime.conversationSeen)
				}
			})
		}
	})
	t.Run("child readonly disables the actual running controls", func(t *testing.T) {
		comp := composer.New("fixture")
		comp.Running = true
		comp.Draft = "Keep this draft"
		context := &composer.Context{Readonly: true, Models: []composer.Model{{ID: "fixture", Name: "Fixture"}}}
		actions := &pickerViewActions{}
		tester := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			ui.Column(c).Fill().Children(func() { composer.View(c, comp, context, actions) })
		}, 800, 500)
		tester.SetLocale("en")
		tester.Frame()
		tester.Frame()
		button, ok := tester.AccessByRole(ui.RoleButton, i18n.In("en", "chat_stop"))
		if !ok || !button.Disabled || comp.Draft != "Keep this draft" {
			t.Fatal("readonly child exposed a mutable Stop control", button, ok)
		}
		origin := &protocol.Delegation{Session: protocol.SessionID{1}}
		session := protocol.Session{ID: protocol.SessionID{2}, Delegation: &origin, Config: protocol.SessionConfig{Model: "captured-child", Effort: protocol.Effort{Kind: "high"}}}
		if !delegated(session) {
			t.Fatal("real child was editable")
		}
		child := protocol.ConversationChild{Run: protocol.ConversationRun{Status: "stopping"}}
		view := childView(nil, child, session, "en")
		if !view.Status.Active() || view.Model != "captured-child" || view.Effort != "high" {
			t.Fatal(view)
		}
		child.Run.Status = "continued"
		if view := childView(nil, child, session, "en"); view.Status != conversation.ChildContinued || view.Status.Active() {
			t.Fatal(view)
		}
		started, finished := int64(1000), int64(66000)
		child.Run.StartedMs, child.Run.FinishedMs = &started, &finished
		if elapsed := childElapsed(child.Run, time.UnixMilli(999000)); elapsed != "1m05s" {
			t.Fatal("finished child elapsed used the UI clock", elapsed)
		}
		child.Run.Status = "stopping"
		if elapsed := childElapsed(child.Run, time.UnixMilli(4000)); elapsed != "3s" {
			t.Fatal("active child elapsed ignored the UI clock", elapsed)
		}
		if elapsed := childElapsed(child.Run, time.UnixMilli(0)); elapsed != "0s" {
			t.Fatal("negative elapsed", elapsed)
		}
		origin = nil
		if delegated(session) {
			t.Fatal("explicit null delegation made a main session readonly")
		}
	})
}
