// Package operations binds declared native host operations without running
// package scripts. Package installation and callback execution have other owners.
package operations

import (
	"bytes"
	_ "embed"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"io/fs"
	"path"
	"runtime"
	"slices"
	"strings"
	"sync"

	sailry "github.com/sailry/sailry-harness"
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
	Contexts     []string
	Presentation protocol.ToolPresentation
	Grouping     protocol.ToolGrouping
	Display      *protocol.ToolDisplay
}
type Instruction struct {
	Text     string
	AnyTools []string
}
type Catalog struct {
	ArtifactClasses map[string][]string
	References      []protocol.PluginReference
	Tools           []Declaration
	Instructions    []Instruction
}
type manifestTool struct {
	Name        string                 `json:"name"`
	Description string                 `json:"description"`
	Operation   protocol.ToolOperation `json:"operation"`
	Handler     *struct {
		Operation protocol.ToolOperation `json:"operation"`
	} `json:"handler"`
	Initial              bool                      `json:"initial"`
	Contexts             []string                  `json:"contexts"`
	Platforms            []string                  `json:"platforms"`
	PlatformDescriptions map[string]string         `json:"platform_descriptions"`
	Presentation         protocol.ToolPresentation `json:"presentation"`
	Grouping             protocol.ToolGrouping     `json:"grouping"`
	Display              *protocol.ToolDisplay     `json:"display"`
}

var Bundled = sync.OnceValues(load)

func load() (*Catalog, error) {
	var specs []Spec
	if err := json.Unmarshal(specification, &specs); err != nil {
		return nil, err
	}
	byOperation := map[protocol.ToolOperation]Spec{}
	for _, spec := range specs {
		byOperation[spec.Operation] = spec
	}
	paths, err := fs.Glob(sailry.Plugins, "plugins/*/plugin.json")
	if err != nil {
		return nil, err
	}
	catalog := &Catalog{ArtifactClasses: map[string][]string{}}
	for _, name := range paths {
		data, err := fs.ReadFile(sailry.Plugins, name)
		if err != nil {
			return nil, err
		}
		var manifest struct {
			Name       string `json:"name"`
			Extensions map[string]struct {
				Tools        []manifestTool `json:"tools"`
				ArtifactView *struct {
					AllowedClasses []string `json:"allowed_classes"`
				} `json:"artifact_view"`
				Instructions string `json:"instructions"`
			} `json:"extensions"`
		}
		if err = json.Unmarshal(data, &manifest); err != nil {
			return nil, err
		}
		extension := manifest.Extensions["dev.sailry.platform"]
		if extension.ArtifactView != nil {
			catalog.ArtifactClasses[manifest.Name] = append([]string(nil), extension.ArtifactView.AllowedClasses...)
		}
		var declarations []Declaration
		var instructionTools []string
		for _, tool := range extension.Tools {
			op := tool.Operation
			if tool.Handler != nil && tool.Handler.Operation != "" {
				op = tool.Handler.Operation
			}
			spec, exists := byOperation[op]
			if !exists {
				continue
			}
			if len(tool.Platforms) > 0 && !slices.Contains(tool.Platforms, runtime.GOOS) {
				continue
			}
			if description := tool.PlatformDescriptions[runtime.GOOS]; description != "" {
				spec.Description = description
			} else if tool.Description != "" {
				spec.Description = tool.Description
			}
			presentation := tool.Presentation
			if presentation == "" {
				presentation = protocol.ToolPresentationSummary
			}
			grouping := tool.Grouping
			if grouping == "" {
				grouping = protocol.ToolGroupingSequence
			}
			declarations = append(declarations, Declaration{Spec: spec, Name: Alias(manifest.Name, tool.Name), Exposed: tool.Name, Initial: tool.Initial, Contexts: tool.Contexts, Presentation: presentation, Grouping: grouping, Display: tool.Display})
			instructionTools = append(instructionTools, Alias(manifest.Name, tool.Name))
		}
		if len(declarations) == 0 {
			continue
		}
		tree, err := fs.Sub(sailry.Plugins, path.Dir(name))
		if err != nil {
			return nil, err
		}
		digest, err := packageDigest(tree)
		if err != nil {
			return nil, err
		}
		owner := protocol.PluginReference{Name: manifest.Name, Digest: digest}
		catalog.References = append(catalog.References, owner)
		for _, declaration := range declarations {
			declaration.Owner = owner
			catalog.Tools = append(catalog.Tools, declaration)
		}
		if extension.Instructions != "" {
			catalog.Instructions = append(catalog.Instructions, Instruction{Text: extension.Instructions, AnyTools: instructionTools})
		}
	}
	return catalog, nil
}
func Alias(owner, name string) string {
	digest := blake3.Sum256([]byte(owner))
	return "plugin_" + hex.EncodeToString(digest[:8]) + "_" + name
}
func (d Declaration) Available(project bool) bool {
	return len(d.Contexts) == 0 || project && slices.Contains(d.Contexts, "workspace") || !project && slices.Contains(d.Contexts, "plugin")
}
func packageDigest(tree fs.FS) (string, error) {
	var names []string
	err := fs.WalkDir(tree, ".", func(name string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if name != "." {
			names = append(names, name)
		}
		return nil
	})
	if err != nil {
		return "", err
	}
	slices.SortFunc(names, func(a, b string) int { return slices.Compare(strings.Split(a, "/"), strings.Split(b, "/")) })
	var encoded bytes.Buffer
	var size [8]byte
	for _, name := range names {
		info, err := fs.Stat(tree, name)
		if err != nil {
			return "", err
		}
		binary.LittleEndian.PutUint64(size[:], uint64(len(name)))
		encoded.Write(size[:])
		encoded.WriteString(name)
		if info.IsDir() {
			encoded.WriteString("directory")
			continue
		}
		data, err := fs.ReadFile(tree, name)
		if err != nil {
			return "", err
		}
		encoded.WriteString("file")
		encoded.WriteByte(0)
		binary.LittleEndian.PutUint64(size[:], uint64(len(data)))
		encoded.Write(size[:])
		encoded.Write(data)
	}
	var digest [32]byte
	blake3.DeriveKey(digest[:], "Sailry plugin package v1", encoded.Bytes())
	return hex.EncodeToString(digest[:]), nil
}
