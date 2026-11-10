package live

import (
	"encoding/json"
	"maps"
	"reflect"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type teamComposer struct {
	composer    *composer.State
	members     map[string]*composer.MemberModel
	revision    uint64
	observed    string
	autoAttempt string
	saving      bool
	selecting   bool
	selection   *protocol.TeamSelection
	baseConfig  protocol.SessionConfig
	pending     *protocol.Command
	waiters     []func(bool)
}

const defaultTeamID = "default"

func selectedTeam(roles protocol.RoleSnapshot) string {
	if roles.Execution == protocol.ConversationExecutionModel {
		return ""
	}
	if roles.Team != nil {
		return roles.Team.ID.String()
	}
	return defaultTeamID
}

func memberChoice(comp *composer.State, role protocol.RoleProfile, roles protocol.RoleSnapshot, models []composer.Model) *composer.MemberModel {
	model := role.Model
	if override, ok := roles.Models[role.ID]; ok {
		model = &override
	}
	id, effort := roleModelView(model)
	if id == "" {
		return &composer.MemberModel{Model: comp.Model, Effort: comp.Effort, Features: maps.Clone(comp.Features), InheritFeatures: true}
	}
	_, overridden := roles.Models[role.ID]
	choice := &composer.MemberModel{Model: id, Features: maps.Clone(comp.Features), Overridden: overridden, InheritFeatures: model.Features == nil}
	if model.Features != nil {
		choice.Features = maps.Clone(*model.Features)
	}
	for _, candidate := range models {
		if candidate.ID == id {
			if effort == "" {
				effort = candidate.DefaultEffort
			}
			choice.Effort = float64(max(0, slices.Index(candidate.Efforts, effort)))
			choice.Features = retainedMemberFeatures(choice.Features, candidate)
			break
		}
	}
	return choice
}

func retainedMemberFeatures(selected map[string]string, model composer.Model) map[string]string {
	result := map[string]string{}
	for _, feature := range model.Features {
		value, chosen := selected[feature.ID]
		if chosen && slices.ContainsFunc(feature.Options, func(option composer.FeatureOption) bool { return option.ID == value }) {
			result[feature.ID] = value
		}
	}
	return result
}

func composerTeamKey(comp *composer.State) string {
	encoded, _ := json.Marshal(struct {
		Team   string
		Models map[string]*composer.MemberModel
	}{comp.Team, comp.MemberModels})
	return string(encoded)
}

func teamCatalog(snapshot protocol.Snapshot, roles protocol.RoleSnapshot) []composer.Team {
	all := composer.Team{ID: defaultTeamID, Name: i18n.In("en", "team_default")}
	for _, role := range snapshot.Roles {
		_, overridden := roles.Models[role.ID]
		all.Members = append(all.Members, composer.Member{ID: role.ID.String(), Name: role.Name, Overridden: overridden})
	}
	result := []composer.Team{all}
	for _, team := range snapshot.Teams {
		view := composer.Team{ID: team.ID.String(), Name: team.Name}
		for _, id := range team.Members {
			for _, role := range snapshot.Roles {
				if role.ID == id {
					_, overridden := roles.Models[id]
					view.Members = append(view.Members, composer.Member{ID: id.String(), Name: role.Name, Overridden: overridden})
					break
				}
			}
		}
		result = append(result, view)
	}
	if roles.Execution == protocol.ConversationExecutionTeam {
		id := selectedTeam(roles)
		index := slices.IndexFunc(result, func(team composer.Team) bool { return team.ID == id })
		if index < 0 {
			result = append(result, composer.Team{ID: id, Name: id})
			index = len(result) - 1
		}
		result[index].Members = nil
		for _, role := range roles.Profiles {
			_, overridden := roles.Models[role.ID]
			result[index].Members = append(result[index].Members, composer.Member{ID: role.ID.String(), Name: role.Name, Overridden: overridden})
		}
	}
	return result
}

func teamCatalogChanged(id string, snapshot protocol.Snapshot, roles protocol.RoleSnapshot) bool {
	if selectedTeam(roles) != id {
		return true
	}
	if id == "" {
		return false
	}
	profiles := snapshot.Roles
	if id != defaultTeamID {
		index := slices.IndexFunc(snapshot.Teams, func(team protocol.TeamProfile) bool { return team.ID.String() == id })
		if index < 0 {
			return false
		}
		team := snapshot.Teams[index]
		if roles.Team == nil || roles.Team.Revision != team.Revision {
			return true
		}
		profiles = nil
		for _, role := range snapshot.Roles {
			if slices.Contains(team.Members, role.ID) {
				profiles = append(profiles, role)
			}
		}
	}
	if len(profiles) != len(roles.Profiles) {
		return true
	}
	for _, role := range profiles {
		if !slices.ContainsFunc(roles.Profiles, func(captured protocol.RoleProfile) bool {
			return captured.ID == role.ID && captured.Revision == role.Revision
		}) {
			return true
		}
	}
	return false
}

func (r *Runtime) projectTeamComposer(cv *viewmodel.Conversation, comp *composer.State, ctx *composer.Context, locales ...string) {
	if r.teams == nil {
		return
	}
	snapshot := r.controller.Snapshot()
	session, exists := r.session(cv.ID)
	ctx.Teams = teamCatalog(snapshot, session.Roles)
	language := "en"
	if r.Preferences != nil && r.Preferences.Data.Language != nil && *r.Preferences.Data.Language == "chinese" {
		language = "zh-CN"
	}
	if len(locales) > 0 {
		language = locales[0]
	}
	r.teams.locale = language
	ctx.Teams[0].Name = i18n.In(language, "team_default")
	state := r.teams.composers[cv.ID]
	if state == nil {
		state = &teamComposer{composer: comp, members: map[string]*composer.MemberModel{}}
		r.teams.composers[cv.ID] = state
		if exists {
			r.applyTeamComposer(comp, session, ctx.Models)
		} else {
			if comp.Team == "" {
				comp.Team = defaultTeamID
			}
			comp.MemberModels = map[string]*composer.MemberModel{}
			for _, role := range snapshot.Roles {
				comp.MemberModels[role.ID.String()] = memberChoice(comp, role, protocol.DefaultRoleSnapshot(), ctx.Models)
			}
		}
		state.revision = session.Revision
		state.observed = composerTeamKey(comp)
	}
	if exists && session.Revision != state.revision && !state.saving {
		if composerTeamKey(comp) == state.observed {
			r.applyTeamComposer(comp, session, ctx.Models)
			state.observed = composerTeamKey(comp)
		}
		state.revision = session.Revision
	}
	comp.SelectTeam = func(id string) bool {
		if ctx.Readonly || state.saving || state.pending != nil || r.composerSaving[cv.ID] {
			return false
		}
		changed := id != comp.Team || exists && teamCatalogChanged(id, snapshot, session.Roles)
		if !changed {
			return true
		}
		state.selecting, state.selection, state.autoAttempt = exists, nil, ""
		comp.Team, comp.MemberModels = id, nil
		for _, team := range snapshot.Teams {
			if team.ID.String() != id {
				continue
			}
			revision := team.Revision
			if exists {
				state.selection = &protocol.TeamSelection{ID: team.ID, Revision: &revision}
			}
			if !exists && team.Model != nil {
				choice := memberChoice(comp, protocol.RoleProfile{Model: team.Model}, protocol.DefaultRoleSnapshot(), ctx.Models)
				comp.Model, comp.Effort, comp.Features = choice.Model, choice.Effort, choice.Features
			}
			break
		}
		return true
	}
	// A changed execution starts with that catalog's inherited member choices.
	if comp.MemberModels == nil {
		comp.MemberModels = map[string]*composer.MemberModel{}
	}
	if state.members == nil {
		state.members = map[string]*composer.MemberModel{}
	}
	members := map[string]bool{}
	for _, team := range ctx.Teams {
		if team.ID == comp.Team {
			for _, member := range team.Members {
				members[member.ID] = true
				profiles := snapshot.Roles
				roles := protocol.DefaultRoleSnapshot()
				if exists && !state.selecting && selectedTeam(session.Roles) == comp.Team {
					profiles, roles = session.Roles.Profiles, session.Roles
				}
				for _, role := range profiles {
					if role.ID.String() != member.ID {
						continue
					}
					base := memberChoice(comp, role, roles, ctx.Models)
					choice := comp.MemberModels[member.ID]
					if choice == nil || !choice.Overridden || reflect.DeepEqual(choice, state.members[member.ID]) {
						comp.MemberModels[member.ID] = base
					}
					copy := *base
					copy.Features = maps.Clone(base.Features)
					state.members[member.ID] = &copy
					break
				}
			}
		}
	}
	for id := range comp.MemberModels {
		if !members[id] {
			delete(comp.MemberModels, id)
			delete(state.members, id)
		}
	}
	comp.TeamLocked = state.saving || state.pending != nil || r.composerSaving[cv.ID]
	key := composerTeamKey(comp)
	if !exists || ctx.Readonly || state.saving || state.pending != nil || r.composerSaving[cv.ID] || !state.selecting && key == state.observed || key == state.autoAttempt {
		return
	}
	state.autoAttempt = key
	r.withComposerTeam(session, comp, func(protocol.Session) {})
}

func (r *Runtime) applyTeamComposer(comp *composer.State, session protocol.Session, models []composer.Model) {
	comp.Team = selectedTeam(session.Roles)
	comp.MemberModels = map[string]*composer.MemberModel{}
	for _, role := range session.Roles.Profiles {
		comp.MemberModels[role.ID.String()] = memberChoice(comp, role, session.Roles, models)
	}
}

func selectedMemberModels(comp *composer.State, models []composer.Model, roles protocol.RoleSnapshot) (map[protocol.RoleID]protocol.RoleModel, error) {
	result := map[protocol.RoleID]protocol.RoleModel{}
	if comp.Team == "" {
		return result, nil
	}
	for _, role := range roles.Profiles {
		choice := comp.MemberModels[role.ID.String()]
		if choice == nil || !choice.Overridden {
			continue
		}
		model, err := composerMemberModel(choice, models)
		if err != nil {
			return nil, err
		}
		if model != nil {
			previous := role.Model
			if override, exists := roles.Models[role.ID]; exists {
				previous = &override
			}
			base := memberChoice(comp, role, roles, models)
			if previous != nil && choice.Model == base.Model && choice.Effort == base.Effort {
				model.Effort = previous.Effort
			}
			if previous != nil && previous.Features != nil && choice.Model == base.Model && maps.Equal(choice.Features, base.Features) {
				model.Features = previous.Features
			}
			result[role.ID] = *model
		}
	}
	return result, nil
}

// withComposerTeam serializes exact team writes before admitting user input.
func (r *Runtime) withComposerTeam(session protocol.Session, comp *composer.State, done func(protocol.Session)) {
	if r.teams == nil || delegated(session) {
		if !delegated(session) {
			done(session)
		}
		return
	}
	id := session.ID.String()
	state := r.teams.composers[id]
	if state == nil {
		for key, draft := range r.teams.composers {
			if draft.composer == comp {
				state = draft
				delete(r.teams.composers, key)
				break
			}
		}
		if state == nil {
			state = &teamComposer{composer: comp, revision: session.Revision}
		}
		r.teams.composers[id] = state
	}
	if state.saving {
		state.waiters = append(state.waiters, func(ok bool) {
			if ok {
				if current, exists := r.session(id); exists {
					r.withComposerTeam(current, comp, done)
				}
			}
		})
		return
	}
	var command protocol.Command
	var err error
	if state.pending != nil {
		command = *state.pending
	} else if state.selecting || selectedTeam(session.Roles) != comp.Team {
		command, err = r.teamSelectionCommand(session, comp)
	} else {
		models, modelErr := selectedMemberModels(comp, r.models(), session.Roles)
		if modelErr != nil {
			r.enqueue(func(c *ui.Context) { r.report(c, modelErr, "session_changed") })
			return
		}
		// Preserve the frozen roster while changing a member's override in place.
		for _, role := range session.Roles.Profiles {
			before, beforeOK := session.Roles.Models[role.ID]
			after, afterOK := models[role.ID]
			if beforeOK != afterOK || beforeOK && !reflect.DeepEqual(before, after) {
				var value *protocol.RoleModel
				if afterOK {
					copy := after
					value = &copy
				}
				command = protocol.Command{Kind: "set_session_member_model", Data: protocol.CommandSetSessionMemberModelPayload{Session: session.ID, ExpectedRevision: session.Revision, Role: role.ID, Model: value}}
				break
			}
		}
	}
	if err != nil {
		r.enqueue(func(c *ui.Context) { r.report(c, err, "session_changed") })
		return
	}
	if command.Kind == "" {
		state.observed = composerTeamKey(comp)
		state.revision = session.Revision
		done(session)
		return
	}
	if state.pending == nil {
		state.baseConfig = session.Config
	}
	state.saving = true
	r.executeScoped("session-team/"+id, command, func(c *ui.Context, out protocol.Output, err error) {
		state.saving = false
		if teamOutcome(err) == kit.Uncertain {
			state.pending = &command
		} else {
			state.pending = nil
		}
		if err == nil {
			updated, valid := detail[protocol.Session](out.Data)
			if !valid || updated.ID != session.ID || updated.Revision == 0 {
				err = fileActionFault(protocol.ErrorCodeOutcomeUnknown, "session team result is unavailable")
				state.pending = &command
			} else {
				if current, exists := r.session(id); !exists || updated.Revision > current.Revision {
					r.admittedSessions[id] = updated
				}
				state.selecting = false
				state.selection = nil
			}
		}
		waiters := state.waiters
		state.waiters = nil
		if err != nil {
			r.report(c, err, "session_changed")
			for _, waiter := range waiters {
				waiter(false)
			}
			return
		}
		current, _ := r.session(id)
		effort := ""
		if chosen := comp.Chosen(r.models()); chosen.Reasoning {
			effort = comp.EffortName(nil, chosen)
		}
		if config, _, err := r.composerConfig(comp, composer.Message{Model: comp.Model, Effort: effort, Features: comp.Features, Mode: comp.Mode, Permission: comp.Permission}); err == nil {
			r.applyComposer(comp, composerConfigAfterTeam(state.baseConfig, config, current.Config))
			baseline, _ := json.Marshal(current.Config)
			r.composerObserved[id] = string(baseline)
			r.composerRevisions[id] = current.Revision
		}
		r.project()
		r.withComposerTeam(current, comp, done)
		for _, waiter := range waiters {
			waiter(true)
		}
	})
}

func composerMemberModel(choice *composer.MemberModel, models []composer.Model) (*protocol.RoleModel, error) {
	if choice == nil {
		return nil, nil
	}
	state := composer.New(choice.Model)
	state.Effort = choice.Effort
	effort := ""
	if chosen := state.Chosen(models); chosen.Reasoning {
		effort = state.EffortName(nil, chosen)
	}
	model, err := modelDraft(choice.Model, effort, nil)
	if model != nil && !choice.InheritFeatures {
		features := maps.Clone(choice.Features)
		model.Features = &features
	}
	return model, err
}

func (r *Runtime) teamSelectionCommand(session protocol.Session, comp *composer.State) (protocol.Command, error) {
	value := protocol.CommandSetSessionExecutionPayload{Session: session.ID, ExpectedRevision: session.Revision, Execution: protocol.ConversationExecutionTeam, Models: map[protocol.RoleID]protocol.RoleModel{}}
	snapshot := r.controller.Snapshot()
	roles := protocol.DefaultRoleSnapshot()
	roles.Profiles = snapshot.Roles
	if comp.Team == "" {
		value.Execution = protocol.ConversationExecutionModel
	} else if comp.Team != defaultTeamID {
		id, err := protocol.ParseTeamID(comp.Team)
		if err != nil {
			return protocol.Command{}, err
		}
		for _, team := range snapshot.Teams {
			if team.ID == id {
				revision := team.Revision
				value.Team = &protocol.TeamSelection{ID: id, Revision: &revision}
				roles.Profiles = nil
				for _, role := range snapshot.Roles {
					if slices.Contains(team.Members, role.ID) {
						roles.Profiles = append(roles.Profiles, role)
					}
				}
				break
			}
		}
		if value.Team == nil {
			return protocol.Command{}, fileActionFault(protocol.ErrorCodeNotFound, "selected team is unavailable")
		}
	}
	if value.Execution == protocol.ConversationExecutionTeam {
		models, err := selectedMemberModels(comp, r.models(), roles)
		if err != nil {
			return protocol.Command{}, err
		}
		value.Models = models
	}
	if r.teams != nil {
		if state := r.teams.composers[session.ID.String()]; state != nil && state.selection != nil && state.selection.ID.String() == comp.Team {
			value.Team = state.selection
		}
	}

	return protocol.Command{Kind: "set_session_team", Data: value}, nil
}

func (r *Runtime) teamReferences(cv *viewmodel.Conversation, _ *composer.State) []composer.Reference {
	var agents []composer.Reference
	roles := r.controller.Snapshot().Roles
	if session, ok := r.session(cv.ID); ok {
		roles = session.Roles.Profiles
	}
	for _, role := range roles {
		encoded, _ := json.Marshal(protocol.RoleReference{ID: role.ID, Revision: role.Revision})
		agents = append(agents, composer.Reference{ID: role.ID.String(), Kind: composer.Agent, Label: role.Name, Description: role.Description, Icon: icons.Bot, Insert: string(encoded)})
	}
	rows := []composer.Reference{}
	if len(agents) > 0 {
		rows = append(rows, composer.Reference{ID: "agents", Kind: composer.Page, Label: "reference_agents", Description: "reference_agents_description", Key: true, Alias: "agents", Group: "reference_group_context", Icon: icons.Bot, Children: agents})
	}
	rows = append(rows, r.pluginReferences()...)
	return append(rows, r.skillReferences(cv)...)
}
