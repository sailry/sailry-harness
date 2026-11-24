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
		Adaptive     bool   `json:"adaptive_thinking"`
		BindsContext bool   `json:"binds_context"`
		ThinkingOff  string `json:"thinking_off"`
	} `json:"rig"`
	Limit struct {
		Context uint32 `json:"context"`
		Output  uint32 `json:"output"`
	} `json:"limit"`
}

func catalogEntry(provider, model string) (catalogModel, bool) {
	catalogOnce.Do(func() {
		if err := json.Unmarshal(catalogJSON, &catalog); err != nil {
			panic("bundled model capability catalog is invalid")
		}
	})
	models := catalog[provider].Models
	lookup := func(id string) (catalogModel, bool) {
		if provider == "anthropic" {
			// Some compatible providers spell the minor version with a dot.
			// Normalize capability lookup only; the wire model stays unchanged.
			switch id {
			case "claude-haiku-5.5":
				id = "claude-haiku-5-5"
			case "claude-sonnet-5.5":
				id = "claude-sonnet-5-5"
			case "claude-opus-5.5":
				id = "claude-opus-5-5"
			}
		}
		entry, ok := models[id]
		if ok && provider == "anthropic" && id == "claude-haiku-5-5" {
			// The pinned catalog lists its efforts but omits adaptive thinking.
			// Apply the reviewed correction to this copy, keeping the source intact.
			entry.Rig.Adaptive = true
			entry.Rig.BindsContext = true
		}
		return entry, ok
	}
	if entry, ok := lookup(model); ok {
		return entry, true
	}
	for at := len(model) - 5; at >= 0; at-- {
		if strings.HasPrefix(model[at:], "-20") && model[at+3] >= '0' && model[at+3] <= '9' && model[at+4] >= '0' && model[at+4] <= '9' {
			if entry, ok := lookup(model[:at]); ok {
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
