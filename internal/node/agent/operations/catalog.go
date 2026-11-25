// Package operations binds declared native host operations without running
// package scripts. Package installation and callback execution have other owners.
package operations

import (
	_ "embed"
	"encoding/hex"
	"encoding/json"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

//go:embed specs.json
var specification []byte

type Spec struct {
	Operation   protocol.ToolOperation `json:"operation"`
	Description string                 `json:"description"`
	Parameters  json.RawMessage        `json:"parameters"`
	ReadOnly    bool                   `json:"read_only"`
	Concurrent  bool                   `json:"concurrent"`
}
type Declaration struct {
	Spec
	Owner        protocol.PluginReference
	Name         string
	Exposed      string
	Initial      bool
	Script       bool
	Contexts     []protocol.PluginConversationResource
	Presentation protocol.ToolPresentation
	Grouping     protocol.ToolGrouping
	Display      *protocol.ToolDisplay
}
type Instruction struct {
	Owner    protocol.PluginReference
	Text     string
	AnyTools []string
}
type Catalog struct {
	Packages        []Package
	ArtifactClasses map[string][]string
	References      []protocol.PluginReference
	Tools           []Declaration
	Instructions    []Instruction
}

// Bundled retains the empty default inventory for callers that also resolve
// installed package captures. Native modules register their own tools and skills.
var Bundled = sync.OnceValues(func() (*Catalog, error) {
	return &Catalog{ArtifactClasses: map[string][]string{}}, nil
})

func Alias(owner, name string) string {
	digest := blake3.Sum256([]byte(owner))
	return "plugin_" + hex.EncodeToString(digest[:8]) + "_" + name
}
