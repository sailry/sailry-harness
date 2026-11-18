package settings

import (
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

const modelCapabilityLookupDelay = 400 * time.Millisecond

type modelCapabilityTarget struct {
	node, channel, route, api, endpoint, version, id string
	auth                                             viewmodel.Auth
	revision                                         uint64
}

type modelCapabilityLookup struct {
	target    modelCapabilityTarget
	deadline  time.Time
	requested bool
	original  *viewmodel.ChannelModel
}

func (e *providerEditor) modelCapabilityTarget(id string) modelCapabilityTarget {
	route := e.current()
	return modelCapabilityTarget{
		node: e.owner, channel: e.id, route: route.ID, api: route.API,
		endpoint: strings.TrimSpace(e.endpoint), version: strings.TrimSpace(e.apiVersion),
		id: strings.TrimSpace(id), auth: e.auth(), revision: e.revision,
	}
}

// lookupModelCapabilities fills automatic tags for a new model ID. A lookup
// belongs to the draft row and the channel it was entered for.
func (e *providerEditor) lookupModelCapabilities(c *ui.Context, m *Model, d *modelDraft) {
	lookup := m.ProviderActions.LookupModel
	if !m.Live || lookup == nil || d.model.CustomCapabilities {
		return
	}
	target := e.modelCapabilityTarget(d.id)
	if !d.seeded && d.model.ID != "" && target.id == d.model.ID {
		if d.lookup != nil {
			d.applyAutomaticCapabilities(d.lookup.original)
			d.lookup = nil
		}
		return
	}
	if d.lookup == nil || d.lookup.target != target {
		var original *viewmodel.ChannelModel
		if d.lookup != nil {
			original = d.lookup.original
		} else if !d.seeded {
			original = d.catalog
		}
		if d.lookup != nil || original != nil {
			d.applyAutomaticCapabilities(nil)
		}
		d.lookup = &modelCapabilityLookup{target: target, deadline: c.Now().Add(modelCapabilityLookupDelay), original: original}
	}
	if target.id == "" || d.lookup.requested || e.probing || !m.providerHostAvailable(target.node) {
		return
	}
	if e.current().Custom && !strings.HasPrefix(target.endpoint, "https://") && !strings.HasPrefix(target.endpoint, "http://") {
		return
	}
	if wait := d.lookup.deadline.Sub(c.Now()); wait > 0 {
		c.After(wait)
		return
	}
	pending, generation := d.lookup, e.generation
	pending.requested = true
	draft := ProviderDraft{
		Channel: viewmodel.Channel{Node: target.node, ID: target.channel, Method: target.route, API: target.api,
			Endpoint: target.endpoint, APIVersion: target.version, Auth: target.auth},
		ExpectedRevision: target.revision,
	}
	lookup(draft, target.id, func(model *viewmodel.ChannelModel) {
		if model == nil || model.ID != target.id || !e.open || e.step != stepConfigure || e.generation != generation {
			return
		}
		for i := range e.models {
			row := &e.models[i]
			if row.lookup == pending && !row.model.CustomCapabilities && e.modelCapabilityTarget(row.id) == target {
				row.applyAutomaticCapabilities(model)
				return
			}
		}
	})
}

func (d *modelDraft) applyAutomaticCapabilities(model *viewmodel.ChannelModel) {
	d.model.Vision, d.model.Tools, d.model.Reasoning = true, true, false
	d.model.Generates, d.catalog, d.hosted = nil, nil, false
	if model == nil {
		return
	}
	catalog := *model
	catalog.Generates = slices.Clone(model.Generates)
	d.catalog = &catalog
	d.model.Vision, d.model.Tools, d.model.Reasoning = model.Vision, model.Tools, model.Reasoning
	d.model.Generates = slices.Clone(model.Generates)
	d.hosted = slices.Contains(model.Generates, viewmodel.GenerateImageTool)
}
