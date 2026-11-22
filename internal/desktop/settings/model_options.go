package settings

import (
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

// ModelOptions returns the enabled channels' models as kit.ModelSelect offers
// them: identified by channel/model and grouped by channel and brand.
func (m *Model) ModelOptions(node string) []kit.ModelOption {
	var out []kit.ModelOption
	brandNames := m.modelBrandNames()
	channels := m.ChannelsAt(node)
	for _, ch := range channels {
		if !ch.Enabled {
			continue
		}
		mark := icons.Brand(ch.Icon)
		if mark == nil {
			mark = icons.Brand(ch.Brand)
		}
		for _, x := range ch.Models {
			out = append(out, kit.ModelOption{ID: ch.ID + "/" + x.ID, Name: x.Name, Provider: ch.Name, Mark: mark,
				Brand: ch.Brand, BrandName: brandNames[ch.Brand], Efforts: x.Efforts, DefaultEffort: x.DefaultEffort})
		}
	}
	return out
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
