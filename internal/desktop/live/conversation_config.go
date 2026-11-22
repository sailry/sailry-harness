package live

import (
	"encoding/json"
	"maps"
	"reflect"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) composerKey(c *composer.State) string {
	effort := ""
	if chosen := c.Chosen(r.models()); chosen.Reasoning {
		effort = c.EffortName(nil, chosen)
	}
	config, _, err := r.composerConfig(c, composer.Message{Model: c.Model, Mode: c.Mode, Permission: c.Permission, Features: c.Features, Effort: effort})
	if err != nil {
		return ""
	}
	value, _ := json.Marshal(config)
	return string(value)
}

func (r *Runtime) defaultComposer(c *composer.State) {
	snapshot := r.snapshot()
	if config := snapshot.Defaults.Config; config != nil {
		for _, p := range snapshot.Providers {
			if p.Enabled && r.hasLocalProviderCredential(p) && p.ID == config.Provider && slices.ContainsFunc(p.Models, func(m protocol.ConversationModel) bool { return m.ID == config.Model }) {
				r.applyComposer(c, *config)
				return
			}
		}
	}
	for _, p := range snapshot.Providers {
		if !p.Enabled || !r.hasLocalProviderCredential(p) || len(p.Models) == 0 {
			continue
		}
		model := p.Models[0]
		if i := slices.IndexFunc(p.Models, func(m protocol.ConversationModel) bool { return m.ID == p.DefaultModel }); i >= 0 {
			model = p.Models[i]
		}
		c.Model = modelKey(p.ID, model.ID)
		c.Effort = 0
		for _, display := range r.models() {
			if display.ID == c.Model {
				if i := slices.Index(display.Efforts, effortName(model.DefaultEffort)); i >= 0 {
					c.Effort = float64(i)
				}
				break
			}
		}
		return
	}
}

// The controller remembers a selection, not a provider capture. Resolve it only
// against the authenticated user's listed channels on this execution Node.
func (r *Runtime) initializeDraftComposer(comp *composer.State) {
	comp.Model, comp.Effort, comp.Team = "", 0, ""
	comp.Features, comp.MemberModels = nil, nil
	r.clearComposerBase(comp)
	root := r.localRuntime()
	if root.Preferences != nil && root.Preferences.Data.Composer != nil {
		saved := root.Preferences.Data.Composer
		comp.Mode, comp.Permission = modeChoice(saved.Mode), permissionChoice(saved.Permission)
		if model := saved.Model; model != nil && model.Source == r.controller.Target() {
			snapshot := r.snapshot()
			for _, provider := range snapshot.Providers {
				if provider.ID != model.Provider || !provider.Enabled || !r.hasLocalProviderCredential(provider) ||
					!slices.ContainsFunc(provider.Models, func(candidate protocol.ConversationModel) bool { return candidate.ID == model.Model }) {
					continue
				}
				config := protocol.DefaultSessionConfig()
				config.Provider, config.Model, config.Effort, config.Features = model.Provider, model.Model, model.Effort, model.Features
				config.Mode, config.Permission = saved.Mode, saved.Permission
				r.applyComposer(comp, config)
				if saved.Team == defaultTeamID || slices.ContainsFunc(snapshot.Teams, func(team protocol.TeamProfile) bool { return team.ID.String() == saved.Team }) {
					comp.Team = saved.Team
				}
				return
			}
		}
	}
	r.defaultComposer(comp)
}

func (r *Runtime) resetDraftComposer(id string, comp *composer.State) {
	delete(r.composerInitialized, id)
	delete(r.composerObserved, id)
	delete(r.composerCatalogs, id)
	r.clearComposerBase(comp)
	if r.teams != nil {
		delete(r.teams.composers, id)
	}
}

// Controls mutate the draft after its projection. Capture those edits before
// the frame saves preferences, including a team change with the same lead model.
func (r *Runtime) rememberDraftComposers() {
	root := r.localRuntime()
	remember := func(owner *Runtime) {
		if owner.teams == nil {
			return
		}
		for id, state := range owner.teams.composers {
			cv := root.App.Data.Find(id)
			if cv == nil || !unsentDraft(cv) || cv.Node != owner.controller.Target().String() || owner.draftSubmitted(cv) {
				continue
			}
			comp := state.composer
			if comp.Host != "" && comp.Host != cv.Node {
				continue
			}
			key := owner.composerKey(comp) + "\x00" + comp.Team
			if key == state.draftConfig {
				continue
			}
			state.draftConfig = key
			effort := ""
			if chosen := comp.Chosen(owner.models()); chosen.Reasoning {
				effort = comp.EffortName(nil, chosen)
			}
			config, _, err := owner.composerConfig(comp, composer.Message{Model: comp.Model, Mode: comp.Mode, Permission: comp.Permission, Features: comp.Features, Effort: effort})
			if _, executionErr := owner.composerExecution(comp); err == nil && executionErr == nil && config.Assistant == nil {
				owner.rememberConfig(config, comp.Team)
			}
		}
	}
	remember(root)
	for _, bound := range root.bindings {
		remember(bound)
	}
}

// A send waits for this controller's in-flight configuration save. A failed or
// uncertain save preserves the draft and never admits the captured message.
func (r *Runtime) withComposerConfig(session protocol.Session, config protocol.SessionConfig, next func(protocol.Session), failed func()) {
	id := session.ID.String()
	if r.teams != nil {
		if state := r.teams.composers[id]; state != nil {
			if state.pending != nil || state.saving {
				before := state.baseConfig
				resume := func(current protocol.Session) {
					r.withComposerConfig(current, composerConfigAfterTeam(before, config, current.Config), next, failed)
				}
				if state.pending != nil {
					r.withComposerTeam(session, state.composer, resume, failed)
				} else {
					state.waiters = append(state.waiters, r.afterComposerSave(id, resume, failed))
				}
				return
			}
		}
	}
	if r.composerSaving[id] {
		r.composerWaiters[id] = append(r.composerWaiters[id], r.afterComposerSave(id, func(current protocol.Session) {
			r.withComposerConfig(current, config, next, failed)
		}, failed))
		return
	}
	before, _ := json.Marshal(session.Config)
	after, _ := json.Marshal(config)
	if string(before) == string(after) {
		next(session)
		return
	}
	r.saveComposerConfig(session, config, r.afterComposerSave(id, next, failed))
}

func (r *Runtime) afterComposerSave(id string, next func(protocol.Session), failed func()) func(bool) {
	return func(ok bool) {
		if ok {
			if current, yes := r.session(id); yes {
				next(current)
				return
			}
		}
		failed()
	}
}

func composerConfigAfterTeam(before, selected, current protocol.SessionConfig) protocol.SessionConfig {
	if selected.Provider == before.Provider && selected.Model == before.Model {
		selected.Provider, selected.Model, selected.Credential = current.Provider, current.Model, current.Credential
		if reflect.DeepEqual(selected.Effort, before.Effort) {
			selected.Effort = current.Effort
		}
		if maps.Equal(selected.Features, before.Features) {
			selected.Features = current.Features
		}
	}
	return selected
}

func (r *Runtime) saveComposerConfig(session protocol.Session, config protocol.SessionConfig, done func(bool)) {
	id := session.ID.String()
	r.composerSaving[id] = true
	value, _ := json.Marshal(config)
	r.composerObserved[id] = string(value)
	r.execute(protocol.Command{Kind: "set_session_config", Data: protocol.CommandSetSessionConfigPayload{Session: session.ID, ExpectedRevision: session.Revision, Config: config}}, func(c *ui.Context, output protocol.Output, err error) {
		ok := err == nil
		if ok {
			updated, valid := detail[protocol.Session](output.Data)
			ok = valid && updated.ID == session.ID && updated.Revision > session.Revision
			if ok {
				r.admittedSessions[id] = updated
				r.composerRevisions[id] = updated.Revision
			} else {
				err = fileActionFault(protocol.ErrorCodeOutcomeUnknown, "session configuration result is unavailable")
			}
		}
		if !ok {
			if !r.failUnsubmitted(c, id, err) {
				r.report(c, err, "session_changed")
			}
		}
		r.composerSaving[id] = false
		waiters := r.composerWaiters[id]
		delete(r.composerWaiters, id)
		if done != nil {
			done(ok)
		}
		for _, f := range waiters {
			f(ok)
		}
	})
}
