package live

import (
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// modelOptions are the models of the Node's enabled providers as
// kit.ModelSelect offers them outside the settings pages, in a package's
// form or view: named as the composer names them (modelKey), each under its
// provider with the mark the provider's channel has in the settings, its
// preset's or else its API's brand, and the score the composer shows. They
// carry no efforts: a form that chooses one takes them from the catalog it
// was given.
func (r *Runtime) modelOptions() []kit.ModelOption {
	var out []kit.ModelOption
	for _, p := range r.composerProviders() {
		if !p.Enabled {
			continue
		}
		mark := icons.Brand(apiBrands[p.Api])
		if p.Preset != nil {
			if own := icons.Brand(p.Preset.ID); own != nil {
				mark = own
			}
		}
		for _, m := range p.Models {
			option := kit.ModelOption{ID: modelKey(p.ID, m.ID), Name: m.ID, Provider: p.Name, Mark: mark}
			if m.DisplayName != nil {
				option.Name = *m.DisplayName
			}
			if score := r.measuredScore(option.ID, m.DefaultEffort); score != nil {
				option.Score = score.Overall
			}
			out = append(out, option)
		}
	}
	return out
}
