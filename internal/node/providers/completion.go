package providers

import (
	"reflect"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// CompleteModel resolves automatic metadata without replacing configured limits,
// explicit effort choices or a compaction override.
func CompleteModel(model protocol.ConversationModel, reference *protocol.ConversationCatalogModel, api protocol.ConversationModelApi) protocol.ConversationModel {
	completed := model
	native := model.Capabilities
	var reasoning *bool
	if native != nil {
		reasoning = native.Reasoning
	}
	if reasoning == nil && reference != nil {
		reasoning = reference.Reasoning
	}
	completed.Reasoning = false
	if reasoning != nil {
		completed.Reasoning = *reasoning
	} else {
		if native != nil && native.Efforts != nil {
			for _, effort := range *native.Efforts {
				completed.Reasoning = completed.Reasoning || effort.Kind != "default"
			}
		}
		for _, effort := range model.Efforts {
			completed.Reasoning = completed.Reasoning || effort.Kind != "default"
		}
	}
	completed.Vision = true
	if native != nil && native.Vision != nil {
		completed.Vision = *native.Vision
	} else if reference != nil && len(reference.Inputs) > 0 {
		completed.Vision = false
		for _, input := range reference.Inputs {
			completed.Vision = completed.Vision || input == "image"
		}
	}
	completed.Tools = true
	if native != nil && native.Tools != nil {
		completed.Tools = *native.Tools
	} else if reference != nil && reference.Tools != nil {
		completed.Tools = *reference.Tools
	}
	completed.Generates = []protocol.MediaGeneration{}
	if reference != nil {
		for _, output := range reference.Outputs {
			switch output {
			case "image":
				completed.Generates = append(completed.Generates, protocol.MediaGenerationImage)
			case "video":
				completed.Generates = append(completed.Generates, protocol.MediaGenerationVideo)
			}
		}
	}
	refresh := !model.CustomEfforts
	for _, effort := range model.Efforts {
		refresh = refresh && effort.Kind == "default"
	}
	choices := []protocol.Effort{}
	if native != nil && native.Efforts != nil {
		for _, effort := range *native.Efforts {
			if effort.Kind != "default" {
				choices = append(choices, effort)
			}
		}
	} else if reference != nil {
		choices = CatalogEfforts(*reference, api, model.Output)
	}
	if refresh && len(choices) > 0 {
		completed.Efforts = choices
		completed.DefaultEffort = choices[0]
		if native != nil && native.DefaultEffort != nil && native.DefaultEffort.Kind != "default" {
			for _, effort := range choices {
				if reflect.DeepEqual(effort, *native.DefaultEffort) {
					completed.DefaultEffort = effort
					break
				}
			}
		}
	}
	return completed
}
func CatalogEfforts(model protocol.ConversationCatalogModel, api protocol.ConversationModelApi, output uint32) []protocol.Effort {
	choices := []protocol.Effort{}
	toggle := false
	add := func(effort protocol.Effort) {
		if effort.Kind == "default" || validateEffort(api, effort, output) != nil || len(choices) >= 16 {
			return
		}
		for _, previous := range choices {
			if reflect.DeepEqual(previous, effort) {
				return
			}
		}
		choices = append(choices, effort)
	}
	for _, option := range model.Options {
		switch option.Kind {
		case "toggle":
			toggle = true
		case "effort":
			var value protocol.ConversationCatalogReasoningEffortPayload
			if unionData(option.Data, &value) != nil {
				continue
			}
			for _, name := range value.Values {
				if name != nil && validEffort(*name) {
					add(protocol.Effort{Kind: *name})
				}
			}
		case "budget_tokens":
			var value protocol.ConversationCatalogReasoningBudgetTokensPayload
			if unionData(option.Data, &value) != nil || value.Min == nil {
				continue
			}
			if value.Max != nil && *value.Min > 0 && uint32(*value.Min) > *value.Max {
				continue
			}
			if *value.Min == 0 {
				add(protocol.Effort{Kind: "none"})
			} else {
				tokens := *value.Min
				add(protocol.Effort{Kind: "budget", Data: &tokens})
			}
		}
	}
	if len(choices) > 0 && toggle {
		add(protocol.Effort{Kind: "none"})
	}
	return choices
}
