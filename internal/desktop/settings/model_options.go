package settings

import (
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func (m *Model) ChannelsAt(node string) []viewmodel.Channel {
	if m.ChannelsByNode != nil {
		return m.ChannelsByNode[node]
	}
	return m.Channels
}

// ModelOptionsFromChannels projects enabled channels into canonical
// kit.ModelOption entries with Mark, Brand, BrandName, Efforts, DefaultEffort,
// and Score populated consistently across settings, forms, and native modules.
func ModelOptionsFromChannels(channels []viewmodel.Channel, brandNames map[string]string) []kit.ModelOption {
	var out []kit.ModelOption
	for _, ch := range channels {
		if !ch.Enabled {
			continue
		}
		mark := icons.Brand(ch.Icon)
		if mark == nil {
			mark = icons.Brand(ch.Brand)
		}
		brandName := brandNames[ch.Brand]
		if brandName == "" {
			brandName = ch.BrandName
		}
		for _, x := range ch.Models {
			out = append(out, kit.ModelOption{
				ID:            ch.ID + "/" + x.ID,
				Name:          x.Name,
				Provider:      ch.Name,
				Mark:          mark,
				Brand:         ch.Brand,
				BrandName:     brandName,
				Efforts:       slices.Clone(x.Efforts),
				DefaultEffort: x.DefaultEffort,
				Score:         x.Score,
			})
		}
	}
	return out
}

// ModelOptions returns the enabled channels' models as kit.ModelSelect offers
// them: identified by channel/model and grouped by channel and brand.
func (m *Model) ModelOptions(node string) []kit.ModelOption {
	return ModelOptionsFromChannels(m.ChannelsAt(node), m.modelBrandNames())
}

func (m *Model) modelBrandNames() map[string]string {
	node := m.managedNodeID()
	names := make(map[string]string, len(m.Brands))
	for _, brand := range m.Brands {
		if _, seen := names[brand.ID]; !seen && (node == "" || brand.Node == node) {
			names[brand.ID] = brand.Name
		}
	}
	channels := m.Channels
	if m.Live && (m.ChannelsByNode != nil || m.ManagedNode != "") {
		channels = m.ChannelsByNode[node]
	}
	for _, ch := range channels {
		if _, seen := names[ch.Brand]; seen || ch.Brand == "" {
			continue
		}
		name := ch.BrandName
		if name == "" {
			name = ch.Brand[strings.LastIndex(ch.Brand, ":")+1:]
		}
		names[ch.Brand] = name
	}
	return names
}

func (m *Model) modelOptions(node string) []kit.ModelOption { return m.ModelOptions(node) }
