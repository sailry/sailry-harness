// Package briefnode provides the native Node tools for session-owned HTML.
package briefnode

import (
	"context"
	"embed"
	"io/fs"
	"sync"

	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/modules/brief/document"
)

const (
	packageName = "brief"
	skillPath   = "skills/brief/SKILL.md"
)

// ArtifactReference is the native identity written to new HTML revisions. The
// legacy package name is unchanged so earlier revisions remain editable.
func ArtifactReference() protocol.PluginReference {
	return document.Reference()
}

//go:embed tools.json
var catalogJSON []byte

//go:embed skills/brief/SKILL.md
var skillsFS embed.FS

var loadCatalog = sync.OnceValues(func() (moduleapi.CatalogSpec, error) {
	catalog, err := moduleapi.LoadCatalog(catalogJSON)
	if err != nil {
		return catalog, err
	}
	for i := range catalog.Tools {
		spec := &catalog.Tools[i]
		spec.LegacyPackage = packageName
		spec.Parameters = operations.Parameters(spec.Operation)
	}
	return catalog, nil
})

var loadSkills = sync.OnceValue(func() []moduleapi.BuiltInSkill {
	content, err := fs.ReadFile(skillsFS, skillPath)
	if err != nil {
		return nil
	}
	skill, err := plugins.ParseSkill(content, packageName)
	if err != nil {
		return nil
	}
	skill.Path = skillPath
	return []moduleapi.BuiltInSkill{{Package: packageName, Skill: skill, Tree: skillsFS}}
})

type module struct{}

// Module returns the native Node HTML module.
func Module() moduleapi.Module { return module{} }

func (module) ID() string                       { return packageName }
func (module) ReplacesPlugins() []string        { return []string{packageName} }
func (module) Skills() []moduleapi.BuiltInSkill { return loadSkills() }

func (module) Catalog() []moduleapi.ToolMeta {
	catalog, err := loadCatalog()
	if err != nil {
		return nil
	}
	metas := make([]moduleapi.ToolMeta, 0, len(catalog.Tools))
	for _, spec := range catalog.Tools {
		metas = append(metas, spec.Meta())
	}
	return metas
}

func (module) ToolsForTurn(ctx context.Context, host moduleapi.Host, inv *store.AgentInvocation) ([]engine.Tool, error) {
	if inv.Session.Delegation != nil {
		return nil, nil
	}
	if installed, _, err := host.PluginInstalled(ctx, packageName); err != nil {
		return nil, err
	} else if installed {
		if _, captured := host.PluginCaptured(inv, packageName); !captured {
			return nil, nil
		}
	}
	catalog, err := loadCatalog()
	if err != nil {
		return nil, err
	}
	tools := make([]engine.Tool, 0, len(catalog.Tools))
	for _, spec := range catalog.Tools {
		if !spec.Available(inv) {
			continue
		}
		tool, err := host.WorkspaceTool(inv, moduleapi.WorkspaceToolConfig{
			Spec:            spec,
			Instruction:     catalog.Instructions,
			ArtifactPackage: ArtifactReference(),
		})
		if err != nil {
			return nil, err
		}
		tools = append(tools, tool)
	}
	return tools, nil
}
