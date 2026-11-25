package node

import (
	"context"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/prompt"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
)

type AgentInvocation = store.AgentInvocation

type Tool = engine.Tool

type ToolResult = engine.ToolResult

type ToolRequest = canonical.ToolRequest

type PromptPart = prompt.Part

type ToolMeta = moduleapi.ToolMeta

type ToolSpec = moduleapi.ToolSpec

type CatalogSpec = moduleapi.CatalogSpec

type WorkspaceToolConfig = moduleapi.WorkspaceToolConfig

type ConnectionToolConfig = moduleapi.ConnectionToolConfig

func LoadCatalog(raw []byte) (CatalogSpec, error) { return moduleapi.LoadCatalog(raw) }

func BuildCustomTool(spec ToolSpec, instruction string, execute func(context.Context, ToolRequest) ToolResult) (Tool, error) {
	return moduleapi.BuildCustomTool(spec, instruction, execute)
}
