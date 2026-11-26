package live

import (
	"reflect"

	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	nodeproviders "github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// modelOptions are the models of the Node's enabled providers as
// kit.ModelSelect offers them across settings, plugin forms, and native
// modules: identified by provider/model with canonical Mark, Brand, BrandName,
// Efforts, DefaultEffort, and Score populated through the shared settings
// channel projection.
func (r *Runtime) modelOptions() []kit.ModelOption {
	return canonicalModelOptions(
		r.controller.Target().String(),
		r.composerProviders(),
		r.providerPresets,
		r.providerBrands,
		func(id string, effort protocol.Effort) float64 {
			if score := r.measuredScore(id, effort); score != nil {
				return score.Overall
			}
			return 0
		},
	)
}

func canonicalModelOptions(
	node string,
	providers []protocol.ConversationProvider,
	presets map[string]protocol.ConversationPresetsEntry,
	brandNames map[string]string,
	score func(id string, effort protocol.Effort) float64,
) []kit.ModelOption {
	channels := make([]viewmodel.Channel, 0, len(providers))
	for _, p := range providers {
		if !p.Enabled {
			continue
		}
		ch := viewmodel.Channel{
			ID:      p.ID.String(),
			Name:    p.Name,
			Enabled: true,
		}
		if p.Preset != nil {
			reference := *p.Preset
			ch.Icon = reference.ID
			route := -1
			for id, entry := range presets {
				i := presetRouteIndex(id)
				if (route < 0 || i < route) &&
					entry.Preset.ID == reference.ID &&
					samePresetSource(entry.Source, reference.Source) &&
					i < len(entry.Preset.Routes) &&
					nodeproviders.RouteAPI(entry.Preset.Routes[i].Protocol) == p.Api &&
					(reference.Route == nil || reflect.DeepEqual(*reference.Route, presetRouteKey(entry.Preset.Routes[i]))) {
					route = i
					ch.Brand = providerBrandKey(node, entry)
					ch.Icon = entry.Preset.ID
				}
			}
		}
		if ch.Brand == "" {
			ch.Brand = apiBrands[p.Api]
		}
		if ch.Brand == "" {
			ch.Brand = modelFamily(p.Api)
		}
		if brandNames != nil {
			ch.BrandName = brandNames[ch.Brand]
		}
		for _, m := range p.Models {
			cm := channelModel(m)
			if !m.Reasoning {
				cm.Efforts = nil
			}
			if score != nil {
				cm.Score = score(modelKey(p.ID, m.ID), m.DefaultEffort)
			}
			ch.Models = append(ch.Models, cm)
		}
		channels = append(channels, ch)
	}
	return settings.ModelOptionsFromChannels(channels, brandNames)
}
