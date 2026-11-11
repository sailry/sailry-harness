package model

import (
	_ "embed"
	"encoding/json"
	"strings"
	"sync"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The pinned source catalog supplies request capabilities, not model-name tables.
//
//go:embed data/model-catalog.json
var catalogJSON []byte

// Request policies come from the one bundled preset catalog.
var bundledPolicies = sync.OnceValue(func() []protocol.ConversationPresetsRequestPolicy {
	return providers.Bundled().Policies
})

var catalogOnce sync.Once
var catalog map[string]struct {
	Models map[string]catalogModel `json:"models"`
}

type catalogModel struct {
	Reasoning        bool `json:"reasoning"`
	ReasoningOptions []struct {
		Type   string    `json:"type"`
		Values []*string `json:"values"`
	} `json:"reasoning_options"`
	Rig struct {
		Adaptive bool `json:"adaptive_thinking"`
	} `json:"rig"`
	Limit struct {
		Context uint32 `json:"context"`
	} `json:"limit"`
}

func catalogEntry(provider, model string) (catalogModel, bool) {
	catalogOnce.Do(func() {
		if err := json.Unmarshal(catalogJSON, &catalog); err != nil {
			panic("bundled model capability catalog is invalid")
		}
	})
	models := catalog[provider].Models
	if entry, ok := models[model]; ok {
		return entry, true
	}
	for at := len(model) - 5; at >= 0; at-- {
		if strings.HasPrefix(model[at:], "-20") && model[at+3] >= '0' && model[at+3] <= '9' && model[at+4] >= '0' && model[at+4] <= '9' {
			if entry, ok := models[model[:at]]; ok {
				return entry, true
			}
		}
	}
	return catalogModel{}, false
}
func (entry catalogModel) canDisable() bool {
	for _, option := range entry.ReasoningOptions {
		if option.Type == "toggle" {
			return true
		}
		for _, value := range option.Values {
			if value == nil || *value == "none" {
				return true
			}
		}
	}
	return false
}
