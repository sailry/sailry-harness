package model

import (
	"context"
	"fmt"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

var acpLevels = []string{"minimal", "low", "medium", "high", "xhigh", "max"}

// DiscoverACP lists the models the agent offers a new session. No prompt is
// sent. Variants the package declares as effort levels fold into one model;
// nothing is listed that the agent did not report.
func DiscoverACP(ctx context.Context, binding Binding, capture ACPCapture) (protocol.ConversationDiscoveryCatalog, error) {
	catalog := protocol.ConversationDiscoveryCatalog{Endpoint: binding.Provider.Endpoint, Models: []protocol.ConversationDiscoveryModel{}}
	language, err := NewACP(binding, capture)
	if err != nil {
		return catalog, err
	}
	model := language.(*acpModel)
	owner, cancel := context.WithTimeout(ctx, acpStart)
	defer cancel()
	entry, agent, err := model.lease(owner, true)
	if err != nil {
		return catalog, Classify(err)
	}
	err = model.open(owner, agent, "")
	// The session's state is copied before the release: the next lease of the
	// program reopens the same agent and rewrites it.
	offers, images, thought := slices.Clone(agent.offers), agent.images, slices.Clone(agent.thought.Values)
	// A session opened only to read its models holds no history.
	model.capture.Pool.Release(entry, err == nil)
	if err != nil {
		return catalog, Classify(err)
	}
	if len(offers) == 0 {
		return catalog, &Failure{Kind: Unavailable, Message: fmt.Sprintf("%s did not list its models", capture.Program.Command)}
	}
	index := map[string]int{}
	for _, offer := range offers {
		id, name, level := offer.ID, offer.Name, ""
		for _, candidate := range acpLevels {
			if suffix := capture.Adapter.Acp.Efforts[candidate]; suffix != "" && strings.HasSuffix(offer.ID, suffix) && len(offer.ID) > len(suffix) {
				id, level = strings.TrimSuffix(offer.ID, suffix), candidate
				// "Name (High)" names the variant, not the model.
				if open := strings.LastIndex(name, " ("); open > 0 && strings.EqualFold(name[open:], " ("+candidate+")") {
					name = name[:open]
				}
			}
		}
		at, known := index[id]
		if !known {
			at, index[id] = len(catalog.Models), len(catalog.Models)
			capabilities := protocol.DefaultConversationDiscoveryCapabilities()
			vision, tools, reasoning := images, capture.Adapter.Capabilities.Tools, false
			capabilities.Vision, capabilities.Tools, capabilities.Reasoning = &vision, &tools, &reasoning
			entry := protocol.ConversationDiscoveryModel{ID: id, Capabilities: &capabilities}
			if name != "" && name != id {
				display := name
				entry.DisplayName = &display
			}
			catalog.Models = append(catalog.Models, entry)
		}
		levels := []string{level}
		if level == "" {
			levels = thought
		}
		capabilities := catalog.Models[at].Capabilities
		efforts := []protocol.Effort{}
		if capabilities.Efforts != nil {
			efforts = *capabilities.Efforts
		}
		for _, level := range levels {
			if slices.Contains(acpLevels, level) && capture.Adapter.Capabilities.Reasoning &&
				!slices.ContainsFunc(efforts, func(effort protocol.Effort) bool { return effort.Kind == level }) {
				efforts = append(efforts, protocol.Effort{Kind: level})
			}
		}
		if len(efforts) > 0 {
			slices.SortFunc(efforts, func(left, right protocol.Effort) int {
				return slices.Index(acpLevels, left.Kind) - slices.Index(acpLevels, right.Kind)
			})
			reasoning := true
			selected := efforts[0]
			for _, effort := range efforts {
				if effort.Kind == "medium" {
					selected = effort
				}
			}
			capabilities.Reasoning, capabilities.Efforts, capabilities.DefaultEffort = &reasoning, &efforts, &selected
		}
	}
	return catalog, nil
}
