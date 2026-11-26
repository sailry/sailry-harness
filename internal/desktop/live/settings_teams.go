package live

import (
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func (r *Runtime) BindTeamSettings() settings.WindowBinding {
	m := r.App.Settings
	return settings.WindowBinding{
		Blocked: func(string) {
			m.SetTeamChannels(nil)
			m.Concurrency = settings.Stored[float64]{Pending: true}
		},
		Mount: func(target settings.PageTarget) settings.PageBinding {
			owner := r.runtimeFor(target.Node)
			if owner.teams == nil {
				owner.initTeams()
			}
			m.Concurrency = settings.Stored[float64]{}
			m.TeamActions = &settings.TeamActions{
				SaveConcurrency: func() {
					if target.Current() {
						owner.saveConcurrency()
					}
				},
				SaveTeam: func(team viewmodel.Team, done func(kit.Outcome)) {
					if target.Current() {
						owner.saveTeam(team, done)
					} else {
						done(kit.Failed)
					}
				},
				SaveRole: func(role viewmodel.Role, done func(kit.Outcome)) {
					if target.Current() {
						owner.saveRole(role, done)
					} else {
						done(kit.Failed)
					}
				},
				RemoveTeam: func(team viewmodel.Team, done func(kit.Outcome)) {
					if target.Current() {
						owner.removeTeam(team, done)
					} else {
						done(kit.Failed)
					}
				},
				RemoveRole: func(role viewmodel.Role, done func(kit.Outcome)) {
					if target.Current() {
						owner.removeRole(role, done)
					} else {
						done(kit.Failed)
					}
				},
			}
			owner.readTeamSettings()
			var projected bool
			var delivered, providers, scores uint64
			return settings.PageBinding{
				Follow: func() {
					revision := owner.controller.ProviderRevision()
					if !projected || providers != revision || scores != owner.modelScores.revision {
						m.SetTeamChannels(owner.modelChannels(owner.snapshot().Providers))
						providers, scores = revision, owner.modelScores.revision
					}
					if !projected || delivered != r.delivered {
						owner.teamsFrame()
						projected, delivered = true, r.delivered
					}
				},
				Recover: owner.readTeamSettings,
				Close: func() {
					if page := m.TeamPageState; page != nil {
						page.CloseDialogs()
					}
					owner.teams.settingsUnknown = nil
					m.TeamActions, m.Roles, m.Teams = nil, nil, nil
					m.SetTeamChannels(nil)
					m.Concurrency, m.ConcurrencyReadOnly = settings.Stored[float64]{}, false
				},
			}
		},
	}
}
