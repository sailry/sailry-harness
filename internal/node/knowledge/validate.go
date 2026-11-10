package knowledge

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Query and citation bounds of the protocol.
const (
	DefaultLimit  = 5
	PageLimit     = 100
	CitationLimit = 64 * 1024
	// MaxPrefixBytes bounds a plugin storage key prefix.
	MaxPrefixBytes = 128
)

func filters(values []string) bool {
	for _, value := range values {
		if value == "" || strings.ContainsRune(value, 0) {
			return false
		}
	}
	return true
}

// ValidateDefinition checks the shape of a collection definition.
func ValidateDefinition(definition protocol.KnowledgeDefinition) error {
	shared := func(name string) bool {
		return name != "" && !strings.ContainsFunc(name, func(character rune) bool {
			return !(character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' || character >= '0' && character <= '9' || character == '-' || character == '_' || character == '.')
		})
	}
	valid := strings.TrimSpace(definition.Name) != "" && !strings.ContainsRune(definition.Name, 0) &&
		(definition.Model == nil || localmodels.ValidID(*definition.Model)) && filters(definition.Tags) && filters(definition.Globs)
	for _, name := range definition.SharedWith {
		valid = valid && shared(name)
	}
	if !valid {
		return invalid("invalid knowledge collection definition")
	}
	var source struct {
		Path   string `json:"path"`
		Prefix string `json:"prefix"`
	}
	body, err := json.Marshal(definition.Source)
	if err == nil {
		err = json.Unmarshal(body, &source)
	}
	if err != nil {
		return invalid("invalid knowledge collection definition")
	}
	if definition.Source.Kind == "storage" {
		if len(definition.Globs) != 0 || len(source.Prefix) > MaxPrefixBytes || strings.ContainsRune(source.Prefix, 0) {
			return invalid("invalid knowledge storage prefix")
		}
		return nil
	}
	if strings.ContainsRune(source.Path, 0) {
		return invalid("invalid knowledge source path")
	}
	return nil
}

// ValidateQuery checks a search and its filters.
func ValidateQuery(query protocol.KnowledgeQuery) error {
	if strings.TrimSpace(query.Query) == "" || strings.ContainsRune(query.Query, 0) || !filters(query.Tags) || query.Limit == 0 ||
		query.Path != nil && strings.ContainsRune(*query.Path, 0) ||
		query.ModifiedAfterMs != nil && query.ModifiedBeforeMs != nil && *query.ModifiedAfterMs > *query.ModifiedBeforeMs {
		return invalid("invalid knowledge query or filters")
	}
	return nil
}

// Hash reports a lowercase BLAKE3 hex digest.
func Hash(value string) bool {
	return len(value) == 64 && !strings.ContainsFunc(value, func(character rune) bool {
		return !(character >= '0' && character <= '9' || character >= 'a' && character <= 'f')
	})
}

// ValidateCitation checks a citation read.
func ValidateCitation(request protocol.KnowledgeCitationRequest) error {
	if !Hash(request.Chunk) || !Hash(request.Revision) || request.Limit == 0 {
		return invalid("invalid knowledge citation request")
	}
	return nil
}

// CitationURL is the link a Node generates for an indexed chunk.
func CitationURL(citation protocol.KnowledgeCitation) string {
	return fmt.Sprintf("sailry-knowledge://%s/%s?revision=%s", citation.Collection, citation.Chunk, citation.Revision)
}
