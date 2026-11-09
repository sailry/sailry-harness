// Package observation classifies adapted model requests without retaining bodies.
package observation

import (
	"cmp"
	"encoding/json"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const ProviderKey = "sailry.observation"

type Source struct {
	Kind protocol.ConversationObservationCategoryKind
	Text string
}

// Inventory belongs to one frozen model request, never a cached provider.
// A nil inventory means provenance is unavailable; an empty one means known empty.
type Inventory struct {
	System   string
	Sources  []Source
	Contexts []Context
}

func (*Inventory) Options() {}

type Result struct {
	Categories []protocol.ConversationObservationCategory
	Complete   bool
}

// Prepared inspects the final adapted body. It never changes or retains raw.
func Prepared(raw []byte, inventory *Inventory) Result {
	var body any
	if err := json.Unmarshal(raw, &body); err != nil {
		body = nil
	}
	var sources []Source
	var contexts []Context
	known := inventory != nil && retainsSystem(body, blake3.Sum256([]byte(inventory.System)))
	if known {
		sources = uniqueSources(inventory.Sources)
		contexts = inventory.Contexts
	}
	result := Result{Categories: estimate(body, sources, known, contexts)}
	for _, category := range result.Categories {
		if category.Tokens != nil {
			result.Complete = true
			break
		}
	}
	object, _ := body.(map[string]any)
	return Result{
		Categories: result.Categories,
		Complete:   result.Complete && object["previous_response_id"] == nil && object["previous_interaction_id"] == nil,
	}
}

func uniqueSources(sources []Source) []Source {
	unique := make([]Source, 0, len(sources))
	seen := map[Source]bool{}
	for _, source := range sources {
		if source.Text != "" && !seen[source] {
			seen[source] = true
			unique = append(unique, source)
		}
	}
	slices.SortFunc(unique, func(a, b Source) int {
		if order := cmp.Compare(slices.Index(kinds[:], a.Kind), slices.Index(kinds[:], b.Kind)); order != 0 {
			return order
		}
		return cmp.Compare(a.Text, b.Text)
	})
	return unique
}

func retainsSystem(body any, expected [32]byte) bool {
	object, _ := body.(map[string]any)
	var parts []any
	for _, field := range systemFields {
		if value, ok := object[field]; ok {
			parts = append(parts, value)
		}
	}
	for _, field := range messageFields {
		messages, _ := object[field].([]any)
		for _, message := range messages {
			item, _ := message.(map[string]any)
			role, _ := item["role"].(string)
			if role == "system" || role == "developer" {
				if value, _, ok := messageContent(item); ok {
					parts = append(parts, value)
				}
			}
		}
	}
	if len(parts) == 0 {
		return expected == blake3.Sum256(nil)
	}
	for _, value := range parts {
		if containsSystem(value, expected) {
			return true
		}
	}
	return false
}

func containsSystem(value any, expected [32]byte) bool {
	switch value := value.(type) {
	case string:
		return blake3.Sum256([]byte(value)) == expected
	case []any:
		for _, value := range value {
			if containsSystem(value, expected) {
				return true
			}
		}
	case map[string]any:
		for _, key := range []string{"text", "parts", "content"} {
			if containsSystem(value[key], expected) {
				return true
			}
		}
	}
	return false
}
