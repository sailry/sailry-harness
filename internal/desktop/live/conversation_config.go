package live

import (
	"encoding/json"
	"slices"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) composerKey(c *composer.State) string {
	effort := ""
	if chosen := c.Chosen(r.App.Data.Models); chosen.Reasoning {
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
	snapshot := r.controller.Snapshot()
	if config := snapshot.Defaults.Config; config != nil {
		for _, p := range snapshot.Providers {
			if p.Enabled && p.ID == config.Provider && slices.ContainsFunc(p.Models, func(m protocol.ConversationModel) bool { return m.ID == config.Model }) {
				r.applyComposer(c, *config)
				return
			}
		}
	}
	for _, p := range snapshot.Providers {
		if !p.Enabled || len(p.Models) == 0 {
			continue
		}
		model := p.Models[0]
		if i := slices.IndexFunc(p.Models, func(m protocol.ConversationModel) bool { return m.ID == p.DefaultModel }); i >= 0 {
			model = p.Models[i]
		}
		c.Model = modelKey(p.ID, model.ID)
		c.Effort = 0
		for _, display := range r.App.Data.Models {
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

// A send waits for this controller's in-flight configuration save. A failed or
// uncertain save preserves the draft and never admits the captured message.
func (r *Runtime) withComposerConfig(session protocol.Session, config protocol.SessionConfig, next func(protocol.Session)) {
	id := session.ID.String()
	if r.composerSaving[id] {
		r.composerWaiters[id] = append(r.composerWaiters[id], func(ok bool) {
			if ok {
				if current, yes := r.session(id); yes {
					r.withComposerConfig(current, config, next)
				}
			}
		})
		return
	}
	before, _ := json.Marshal(session.Config)
	after, _ := json.Marshal(config)
	if string(before) == string(after) {
		next(session)
		return
	}
	r.saveComposerConfig(session, config, func(ok bool) {
		if ok {
			if current, yes := r.session(id); yes {
				next(current)
			}
		}
	})
}

func (r *Runtime) saveComposerConfig(session protocol.Session, config protocol.SessionConfig, done func(bool)) {
	id := session.ID.String()
	r.composerSaving[id] = true
	value, _ := json.Marshal(config)
	r.composerObserved[id] = string(value)
	r.command(protocol.Command{Kind: "set_session_config", Data: protocol.CommandSetSessionConfigPayload{Session: session.ID, ExpectedRevision: session.Revision, Config: config}}, "session_changed", func(output protocol.Output, ok bool) {
		if ok {
			updated, valid := detail[protocol.Session](output.Data)
			ok = valid && updated.ID == session.ID && updated.Revision > session.Revision
			if ok {
				r.admittedSessions[id] = updated
				r.composerRevisions[id] = updated.Revision
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
