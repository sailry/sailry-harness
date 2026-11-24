package model

import (
	_ "embed"
	"encoding/json"
	"regexp"
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
	if provider == "anthropic" {
		model = bedrockClaudeModel(model)
	}
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

// bedrockModel matches a Bedrock Claude model or inference profile ID: an
// optional geographic or global profile prefix ("us.", "eu.", "apac.",
// "global."), "anthropic.", the Claude name and an optional version suffix
// ("-v1", "-v1:0", ":0"). An inference profile ARN names it after its last
// slash.
var bedrockModel = regexp.MustCompile(`^(?:[a-z]{2,8}\.)?anthropic\.(claude-[a-z0-9.-]+?)(?:-v\d+)?(?::\d+)?$`)

// bedrockClaudeModel is the Anthropic API name of a Bedrock Claude model ID,
// or the ID unchanged when it names none. Capability lookups use it; the
// wire model stays the Bedrock ID.
func bedrockClaudeModel(id string) string {
	leaf := id[strings.LastIndex(id, "/")+1:]
	if match := bedrockModel.FindStringSubmatch(leaf); match != nil {
		return match[1]
	}
	return id
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
