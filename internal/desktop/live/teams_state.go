package live

import (
	"encoding/json"
	"fmt"
	"maps"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// teamState retains immutable editor bases and pending commands, not history.
type teamState struct {
	roles           map[string]protocol.RoleProfile
	teams           map[string]protocol.TeamProfile
	creates         map[string]protocol.Command
	observed        uint64
	reading         bool
	settingsUnknown *protocol.TeamSettings
	composers       map[string]*teamComposer
	childParents    map[string]protocol.SessionID
	skills          map[protocol.PluginReference]protocol.PluginInfo
	skillReads      map[protocol.PluginReference]bool
	skillFailures   map[protocol.PluginReference]bool
	locale          string
}

// initTeams starts the local Runtime's team state. The Teams page's actions
// and its reads belong to the Node Settings shows (settings_target.go).
func (r *Runtime) initTeams() {
	r.teams = newTeamState()
}

func newTeamState() *teamState {
	return &teamState{roles: map[string]protocol.RoleProfile{}, teams: map[string]protocol.TeamProfile{}, creates: map[string]protocol.Command{}, composers: map[string]*teamComposer{}, childParents: map[string]protocol.SessionID{}, skills: map[protocol.PluginReference]protocol.PluginInfo{}, skillReads: map[protocol.PluginReference]bool{}, skillFailures: map[protocol.PluginReference]bool{}}
}

func profileKey(id string, revision uint64) string { return fmt.Sprintf("%s/%d", id, revision) }

// teamsFrame shows the Node's roles and teams on the page that holds this
// Node, and reads its team settings when the Node reports newer ones.
func (r *Runtime) teamsFrame() {
	if r.teams == nil || !r.showsSettings() {
		return
	}
	snapshot := r.snapshot()
	r.App.Settings.Roles = nil
	for _, role := range snapshot.Roles {
		r.teams.roles[profileKey(role.ID.String(), role.Revision)] = role
		r.App.Settings.Roles = append(r.App.Settings.Roles, roleView(role))
	}
	r.App.Settings.Teams = nil
	for _, team := range snapshot.Teams {
		r.teams.teams[profileKey(team.ID.String(), team.Revision)] = team
		r.App.Settings.Teams = append(r.App.Settings.Teams, teamView(team))
	}
	if snapshot.TeamSettingsRevision != r.teams.observed && !r.teams.reading && !r.App.Settings.Concurrency.Pending && r.teams.settingsUnknown == nil {
		r.readTeamSettings()
	}
}

func roleView(role protocol.RoleProfile) viewmodel.Role {
	model, effort := roleModelView(role.Model)
	return viewmodel.Role{ID: role.ID.String(), Revision: role.Revision, Key: role.Key, Name: role.Name, Description: role.Description, Model: model, Effort: effort, ReadOnly: role.ReadOnly, Instructions: role.Instructions, Features: roleFeatures(role.Model), Avatar: roleAvatar(role.Appearance)}
}

// roleAvatar is the avatar a role's appearance names: its icon, when that
// is an avatar this build has the picture of. Any other icon, one a newer
// peer stored too, is "" and the role shows its mark.
func roleAvatar(appearance *protocol.ProjectsAppearance) string {
	if appearance == nil || icons.Avatar(appearance.Icon) == nil {
		return ""
	}
	return appearance.Icon
}

// childAvatar is the avatar of the role a child was delegated to, "" for
// a child without a role or whose role the Node no longer has.
func childAvatar(roles []protocol.RoleProfile, child protocol.ConversationChild) string {
	if child.Origin.Role == nil {
		return ""
	}
	for _, role := range roles {
		if role.ID == *child.Origin.Role {
			return roleAvatar(role.Appearance)
		}
	}
	return ""
}

// roleFeatures are the feature options a profile's model sets; nil when it
// follows.
func roleFeatures(model *protocol.RoleModel) map[string]string {
	if model == nil || model.Features == nil {
		return nil
	}
	features := maps.Clone(*model.Features)
	if features == nil {
		features = map[string]string{}
	}
	return features
}

func teamView(team protocol.TeamProfile) viewmodel.Team {
	model, effort := roleModelView(team.Model)
	view := viewmodel.Team{ID: team.ID.String(), Revision: team.Revision, Name: team.Name, Description: team.Description, Lead: model, Effort: effort, Instructions: team.Instructions, Features: roleFeatures(team.Model)}
	for _, member := range team.Members {
		view.Members = append(view.Members, member.String())
	}
	return view
}

func roleModelView(model *protocol.RoleModel) (string, string) {
	if model == nil {
		return "", ""
	}
	effort := ""
	if model.Effort != nil {
		effort = effortName(*model.Effort)
	}
	return model.Provider.String() + "/" + model.Model, effort
}

func (r *Runtime) readTeamSettings() {
	if r.teams.reading {
		return
	}
	r.teams.reading = true
	r.teams.observed = r.snapshot().TeamSettingsRevision
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_team_settings"})
		r.enqueue(func(c *ui.Context) {
			r.teams.reading = false
			value, valid := detail[protocol.TeamSettings](out.Data)
			if err == nil && (out.Kind != "team_settings" || !valid) {
				err = fmt.Errorf("unexpected team settings response")
			}
			if err != nil {
				r.reportKeys(c, err, "teams_settings_failed", "teams_settings_conflict", "teams_settings_unknown")
				return
			}
			r.acceptTeamRead(value)
		})
	})
}

// acceptTeamRead takes a read's answer, unless a save is under way or in
// doubt, or the answer is older than the settings held: such a read was
// answered before a save whose result was accepted since.
func (r *Runtime) acceptTeamRead(value protocol.TeamSettings) {
	field := &r.App.Settings.Concurrency
	if !r.showsSettings() || field.Pending || r.teams.settingsUnknown != nil || value.Revision < field.Revision {
		return
	}
	if field.Draft == field.Saved {
		field.Draft = float64(value.ChildConcurrency)
	}
	field.Saved, field.Revision = float64(value.ChildConcurrency), value.Revision
	r.teams.observed = value.Revision
}

func (r *Runtime) saveConcurrency() {
	m := &r.App.Settings.Concurrency
	if m.Pending {
		return
	}
	value := protocol.TeamSettings{Revision: m.Revision, ChildConcurrency: uint64(m.Draft)}
	if r.teams.settingsUnknown != nil {
		value = *r.teams.settingsUnknown
		m.Draft = float64(value.ChildConcurrency)
	}
	m.Pending = true
	r.executeScoped("teams/settings", protocol.Command{Kind: "save_team_settings", Data: value}, func(c *ui.Context, out protocol.Output, err error) {
		outcome := teamOutcome(err)
		settings.ReportRetry(c, outcome, settings.Messages{Failed: "teams_settings_failed", Conflict: "teams_settings_conflict", Uncertain: "teams_settings_unknown"}, func() {
			if r.showsSettings() {
				r.saveConcurrency()
			}
		})
		// The slider is this Node's only while the page shows it.
		if !r.showsSettings() {
			return
		}
		m.Pending = false
		if outcome == kit.Uncertain {
			r.teams.settingsUnknown = &value
		} else {
			r.teams.settingsUnknown = nil
		}
		if err == nil {
			if saved, ok := detail[protocol.TeamSettings](out.Data); ok {
				m.Saved, m.Draft, m.Revision = float64(saved.ChildConcurrency), float64(saved.ChildConcurrency), saved.Revision
				r.teams.observed = saved.Revision
			}
		}
	})
}

func draftKey(kind string, value any) string {
	encoded, _ := json.Marshal(value)
	return kind + "/" + string(encoded)
}
