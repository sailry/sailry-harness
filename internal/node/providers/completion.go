package providers

import (
	"reflect"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// CompleteChannelModel enriches a model with a reference from its channel lookup.
func CompleteChannelModel(provider protocol.ConversationProvider, model protocol.ConversationModel, reference *protocol.ConversationCatalogModel) protocol.ConversationModel {
	var matchedChannel bool
	if reference != nil && reference.ID == model.ID && reference.Inputs != nil && !model.CustomCapabilities {
		namespace, _ := ChannelCatalogNamespace(provider.Endpoint, provider.Authentication)
		matchedChannel = namespace != "" // Unlisted channels may have an ID-only reference.
	}
	model = completeModel(model, reference, provider.Api, matchedChannel)
	// OpenAI's Responses channel also offers a hosted image generation tool.
	official := provider.Authentication == protocol.AuthenticationChatGptOAuth ||
		provider.Authentication == protocol.AuthenticationApiKey && strings.TrimRight(provider.Endpoint, "/") == "https://api.openai.com/v1"
	if !model.CustomCapabilities && official && provider.Api == protocol.ConversationModelApiResponses && model.Tools && model.Vision && len(model.Generates) == 0 {
		model.Generates = []protocol.MediaGeneration{protocol.MediaGenerationImageTool}
	}
	return model
}

// CompleteModel resolves metadata from a matched offering without replacing
// configured limits, explicit effort choices, user-chosen tags or compaction.
func CompleteModel(model protocol.ConversationModel, reference *protocol.ConversationCatalogModel, api protocol.ConversationModelApi) protocol.ConversationModel {
	return completeModel(model, reference, api, true)
}

func completeModel(model protocol.ConversationModel, reference *protocol.ConversationCatalogModel, api protocol.ConversationModelApi, matchedChannel bool) protocol.ConversationModel {
	if reference != nil && reference.ID != model.ID {
		reference = nil
	}
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
	var inputs []string
	if !model.CustomCapabilities {
		completed.Vision = reference == nil || reference.Inputs == nil || slices.Contains(reference.Inputs, "image")
		if matchedChannel && reference != nil {
			inputs = reference.Inputs
		}
	}
	completed = CompleteModelInputs(completed, inputs)
	completed.Tools = true
	if native != nil && native.Tools != nil {
		completed.Tools = *native.Tools
	} else if reference != nil && reference.Tools != nil {
		completed.Tools = *reference.Tools
	}
	completed.Generates = []protocol.MediaGeneration{}
	if reference != nil {
		text := slices.Contains(reference.Outputs, "text")
		for _, output := range reference.Outputs {
			switch output {
			case "image":
				completed.Generates = append(completed.Generates, protocol.MediaGenerationImage)
			case "video":
				completed.Generates = append(completed.Generates, protocol.MediaGenerationVideo)
			case "audio":
				// A model that also answers in text is a chat model with audio
				// output, not a speech synthesis model.
				if !text {
					completed.Generates = append(completed.Generates, protocol.MediaGenerationSpeech)
				}
			}
		}
	}
	if model.CustomCapabilities {
		// The user's tags are authoritative across catalog refresh and discovery.
		completed.Tools, completed.Reasoning = model.Tools, model.Reasoning
		completed.Generates = append([]protocol.MediaGeneration{}, model.Generates...)
		slices.Sort(completed.Generates)
		completed.Generates = slices.Compact(completed.Generates)
	}
	refresh := !model.CustomEfforts
	for _, effort := range model.Efforts {
		refresh = refresh && effort.Kind == "default"
	}
	choices := []protocol.Effort{}
	if native != nil && native.Efforts != nil {
		for _, effort := range *native.Efforts {
			if effort.Kind != "default" && validateEffort(api, effort, model.Output) == nil {
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

// CompleteModelInputs updates just the input tags. Native vision overrides the
// catalog's image entry, but says nothing about the other modalities.
func CompleteModelInputs(model protocol.ConversationModel, catalogInputs []string) protocol.ConversationModel {
	if !model.CustomCapabilities {
		model.Inputs = slices.Clone(catalogInputs)
		if native := model.Capabilities; native != nil && native.Vision != nil {
			model.Vision = *native.Vision
			if model.Inputs != nil && model.Vision != model.AcceptsInput("image") {
				if model.Vision {
					model.Inputs = append(model.Inputs, "image")
				} else {
					model.Inputs = slices.DeleteFunc(model.Inputs, func(input string) bool { return input == "image" })
				}
			}
		}
	}
	model.Vision = model.AcceptsInput("image")
	return model
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
