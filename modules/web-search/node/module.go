// Package websearchnode provides the compile-time native Node web search and
// fetch agent tools.
package websearchnode

import (
	"context"
	"embed"
	"sync"

	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
)

//go:embed tools.json
var catalogJSON []byte

//go:embed skills/research/SKILL.md
var researchSkillMD []byte

//go:embed skills/research/SKILL.md
var skillsFS embed.FS

var loadCatalog = sync.OnceValues(func() (moduleapi.CatalogSpec, error) {
	return moduleapi.LoadCatalog(catalogJSON)
})

var loadSkills = sync.OnceValue(func() []moduleapi.BuiltInSkill {
	skill, err := plugins.ParseSkill(researchSkillMD, "research")
	if err != nil {
		return nil
	}
	skill.Path = "skills/research/SKILL.md"
	return []moduleapi.BuiltInSkill{{
		Package: "web-search",
		Skill:   skill,
		Tree:    skillsFS,
	}}
})

type module struct{}

// Module returns the native Node module providing web_search and web_fetch.
func Module() moduleapi.Module { return module{} }

func (module) ID() string                { return "web-search" }
func (module) ReplacesPlugins() []string { return []string{"web-search"} }
func (module) Skills() []moduleapi.BuiltInSkill {
	return loadSkills()
}

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

func (module) ToolsForTurn(_ context.Context, host moduleapi.Host, inv *store.AgentInvocation) ([]engine.Tool, error) {
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
			Spec:        spec,
			Instruction: catalog.Instructions,
		})
		if err != nil {
			return nil, err
		}
		tools = append(tools, tool)
	}
	return tools, nil
}
