package live

import (
	"fmt"
	"maps"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func modelDraft(id, effort string, previous *protocol.RoleModel) (*protocol.RoleModel, error) {
	if id == "" {
		return nil, nil
	}
	provider, model, ok := strings.Cut(id, "/")
	p, err := protocol.ParseProviderID(provider)
	if !ok || err != nil || model == "" {
		return nil, fileActionFault(protocol.ErrorCodeInvalidRequest, "invalid role model")
	}
	value := protocol.RoleModel{Provider: p, Model: model}
	if previous != nil && previous.Provider == p && previous.Model == model {
		value = *previous
	}
	value.Effort = nil
	if effort != "" {
		e := parseEffort(effort)
		value.Effort = &e
	}
	return &value, nil
}

// draftFeatures puts the editor's feature options on the model: nil
// follows, a map (empty too) sets them.
func draftFeatures(model *protocol.RoleModel, features map[string]string) {
	if model == nil {
		return
	}
	model.Features = nil
	if features != nil {
		set := maps.Clone(features)
		model.Features = &set
	}
}

func teamKey(name string, teams []protocol.TeamProfile) string {
	var slug strings.Builder
	for _, ch := range strings.ToLower(strings.TrimSpace(name)) {
		if ch >= 'a' && ch <= 'z' || ch >= '0' && ch <= '9' {
			slug.WriteRune(ch)
		} else if slug.Len() > 0 && !strings.HasSuffix(slug.String(), "-") {
			slug.WriteByte('-')
		}
	}
	base := strings.TrimRight(slug.String(), "-")
	if base == "" {
		base = "team"
	} else if base[0] < 'a' || base[0] > 'z' {
		base = "team-" + base
	}
	taken := map[string]bool{}
	for _, team := range teams {
		taken[team.Key] = true
	}
	if !taken[base] {
		return base
	}
	for index := 2; ; index++ {
		key := fmt.Sprintf("%s-%d", base, index)
		if !taken[key] {
			return key
		}
	}
}

func (r *Runtime) roleDraft(d viewmodel.Role) (protocol.RoleProfile, error) {
	profile := protocol.DefaultRoleProfile()
	if d.ID != "" {
		var ok bool
		profile, ok = r.teams.roles[profileKey(d.ID, d.Revision)]
		if !ok {
			return profile, fileActionFault(protocol.ErrorCodeNotFound, "role editor base is unavailable")
		}
	} else {
		profile.ID = protocol.NewRoleID()
	}
	model, err := modelDraft(d.Model, d.Effort, profile.Model)
	if err != nil {
		return profile, err
	}
	draftFeatures(model, d.Features)
	profile.Revision = d.Revision
	profile.Key, profile.Name = strings.TrimSpace(d.Key), strings.TrimSpace(d.Name)
	profile.Description, profile.Instructions, profile.ReadOnly, profile.Model = d.Description, d.Instructions, d.ReadOnly, model
	// An avatar is the appearance's icon, beside the color the role had.
	// Without one, an appearance that named an avatar goes back to the
	// Node, which gives the role an icon again; any other is kept.
	switch {
	case d.Avatar != "":
		appearance := protocol.ProjectsAppearance{Icon: d.Avatar, Color: "none"}
		if profile.Appearance != nil {
			appearance.Color = profile.Appearance.Color
		}
		profile.Appearance = &appearance
	case roleAvatar(profile.Appearance) != "":
		profile.Appearance = nil
	}
	return profile, nil
}

func (r *Runtime) teamDraft(d viewmodel.Team) (protocol.TeamProfile, error) {
	profile := protocol.DefaultTeamProfile()
	if d.ID != "" {
		var ok bool
		profile, ok = r.teams.teams[profileKey(d.ID, d.Revision)]
		if !ok {
			return profile, fileActionFault(protocol.ErrorCodeNotFound, "team editor base is unavailable")
		}
	} else {
		profile.ID = protocol.NewTeamID()
		profile.Key = teamKey(d.Name, r.controller.Snapshot().Teams)
	}
	model, err := modelDraft(d.Lead, d.Effort, profile.Model)
	if err != nil {
		return profile, err
	}
	draftFeatures(model, d.Features)
	profile.Revision = d.Revision
	profile.Name, profile.Description, profile.Instructions, profile.Model = strings.TrimSpace(d.Name), d.Description, d.Instructions, model
	profile.Members = nil
	for _, member := range d.Members {
		id, err := protocol.ParseRoleID(member)
		if err != nil {
			return profile, err
		}
		profile.Members = append(profile.Members, id)
	}
	return profile, nil
}

func (r *Runtime) saveRole(d viewmodel.Role, done func(kit.Outcome)) {
	key := draftKey("role", d)
	command, ok := r.teams.creates[key]
	if !ok {
		profile, err := r.roleDraft(d)
		if err != nil {
			r.teamDraftFailure(false, err, done)
			return
		}
		command = protocol.Command{Kind: "put_role", Data: protocol.CommandPutRolePayload{Role: profile, ExpectedRevision: d.Revision}}
		r.teams.creates[key] = command
	}
	r.teamCommand(key, command, false, "teams_role_saved", func(outcome kit.Outcome) {
		if outcome != kit.Uncertain {
			delete(r.teams.creates, key)
		}
		done(outcome)
	})
}

func (r *Runtime) saveTeam(d viewmodel.Team, done func(kit.Outcome)) {
	key := draftKey("team", d)
	command, ok := r.teams.creates[key]
	if !ok {
		profile, err := r.teamDraft(d)
		if err != nil {
			r.teamDraftFailure(true, err, done)
			return
		}
		command = protocol.Command{Kind: "put_team", Data: protocol.CommandPutTeamPayload{Team: profile, ExpectedRevision: d.Revision}}
		r.teams.creates[key] = command
	}
	r.teamCommand(key, command, true, "team_saved", func(outcome kit.Outcome) {
		if outcome != kit.Uncertain {
			delete(r.teams.creates, key)
		}
		done(outcome)
	})
}

func (r *Runtime) removeRole(d viewmodel.Role, done func(kit.Outcome)) {
	id, err := protocol.ParseRoleID(d.ID)
	if err != nil {
		r.teamDraftFailure(false, err, done)
		return
	}
	r.teamCommand("remove-role/"+profileKey(d.ID, d.Revision), protocol.Command{Kind: "remove_role", Data: protocol.CommandRemoveRolePayload{Role: id, ExpectedRevision: d.Revision}}, false, "teams_role_deleted", done)
}

func (r *Runtime) removeTeam(d viewmodel.Team, done func(kit.Outcome)) {
	id, err := protocol.ParseTeamID(d.ID)
	if err != nil {
		r.teamDraftFailure(true, err, done)
		return
	}
	r.teamCommand("remove-team/"+profileKey(d.ID, d.Revision), protocol.Command{Kind: "remove_team", Data: protocol.CommandRemoveTeamPayload{Team: id, ExpectedRevision: d.Revision}}, true, "team_deleted", done)
}

func (r *Runtime) teamDraftFailure(team bool, err error, done func(kit.Outcome)) {
	code := faultCode(err)
	if code == protocol.ErrorCodeInternal {
		code = protocol.ErrorCodeInvalidRequest
		err = fileActionFault(code, "invalid catalog draft")
	}
	r.enqueue(func(c *ui.Context) {
		outcome := teamOutcome(err)
		kit.Toast(c, outcome, i18n.T(c, teamErrorKey(team, code)))
		done(outcome)
	})
}

func teamOutcome(err error) kit.Outcome {
	if err == nil {
		return kit.Done
	}
	switch faultCode(err) {
	case protocol.ErrorCodeOutcomeUnknown, protocol.ErrorCodeUnavailable:
		return kit.Uncertain
	case protocol.ErrorCodeRevisionConflict, protocol.ErrorCodeNotFound, protocol.ErrorCodeConflict:
		return kit.Conflict
	default:
		return kit.Failed
	}
}

func teamErrorKey(team bool, code protocol.ErrorCode) string {
	switch code {
	case protocol.ErrorCodeRevisionConflict, protocol.ErrorCodeNotFound:
		if team {
			return "team_conflict"
		}
		return "teams_role_conflict"
	case protocol.ErrorCodeConflict:
		if team {
			return "team_name_taken"
		}
		return "role_duplicate"
	case protocol.ErrorCodeInvalidRequest:
		if team {
			return "team_invalid"
		}
		return "teams_role_invalid"
	case protocol.ErrorCodeNotConfigured:
		return "role_model_unavailable"
	case protocol.ErrorCodeBusy:
		if !team {
			return "role_capacity"
		}
	case protocol.ErrorCodeOutcomeUnknown, protocol.ErrorCodeUnavailable:
		return "role_unknown"
	}
	return "role_failed"
}

func (r *Runtime) teamCommand(scope string, command protocol.Command, team bool, success string, done func(kit.Outcome)) {
	r.executeScoped("teams/"+scope, command, func(c *ui.Context, _ protocol.Output, err error) {
		outcome, key := teamOutcome(err), success
		if err != nil {
			key = teamErrorKey(team, faultCode(err))
		}
		kit.Toast(c, outcome, i18n.T(c, key))
		r.teamsFrame()
		done(outcome)
	})
}
