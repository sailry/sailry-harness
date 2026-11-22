package desktop

import (
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
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
func (e *providerEditor) lookupModelCapabilities(c *ui.Context, m *Controller, d *modelDraft) {
	lookup := m.ProviderActions.LookupModel
	if !m.Live || lookup == nil || d.model.CustomCapabilities {
		return
	}
	target := e.modelCapabilityTarget(d.id)
	if !d.seeded && !d.relookup && d.model.ID != "" && target.id == d.model.ID {
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
	draft := settings.ProviderDraft{
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
				if row.relookup && target.id == row.model.ID {
					// The saved ID now has the catalog's tags to come back to.
					row.relookup, row.lookup = false, nil
				}
				return
			}
		}
	})
}

// followCatalog hands the row's tags back to the catalog: the editor shows
// the catalog's tags it knows, or asks for them, and a save lets the Node
// complete them.
func (d *modelDraft) followCatalog() {
	d.model.CustomCapabilities = false
	switch {
	case d.lookup != nil:
		// A fresh lookup for the ID as typed; the saved ID's tags stay its
		// original, and without them the saved ID is looked up too.
		d.relookup = d.lookup.original == nil
		d.lookup = &modelCapabilityLookup{original: d.lookup.original}
	case d.catalog != nil:
		d.applyAutomaticCapabilities(d.catalog)
	default:
		d.applyAutomaticCapabilities(nil)
		d.relookup = true
	}
}

func (d *modelDraft) applyAutomaticCapabilities(model *viewmodel.ChannelModel) {
	d.model.Vision, d.model.Tools, d.model.Reasoning = true, true, false
	d.model.Inputs, d.model.Outputs, d.model.Generates, d.catalog, d.hosted = nil, nil, nil, nil, false
	if model == nil {
		return
	}
	catalog := *model
	catalog.Inputs, catalog.Outputs, catalog.Generates = slices.Clone(model.Inputs), slices.Clone(model.Outputs), slices.Clone(model.Generates)
	d.catalog = &catalog
	d.model.Vision, d.model.Tools, d.model.Reasoning = model.Vision, model.Tools, model.Reasoning
	d.model.Inputs, d.model.Outputs, d.model.Generates = slices.Clone(model.Inputs), slices.Clone(model.Outputs), slices.Clone(model.Generates)
	d.hosted = slices.Contains(model.Generates, viewmodel.GenerateImageTool)
}
