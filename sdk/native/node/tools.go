package node

import (
	"context"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/prompt"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
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

// FileReadMetadata records the revisions of files whose source a native tool
// returned. Later edits can reuse those reads in the same worktree.
func FileReadMetadata(worktree protocol.WorktreeID, revisions map[string]string) *map[string]json.RawMessage {
	if len(revisions) == 0 {
		return nil
	}
	raw, _ := json.Marshal(revisions)
	return &map[string]json.RawMessage{canonical.FileReadsKey(worktree): raw}
}

func LoadCatalog(raw []byte) (CatalogSpec, error) { return moduleapi.LoadCatalog(raw) }

func BuildCustomTool(spec ToolSpec, instruction string, execute func(context.Context, ToolRequest) ToolResult) (Tool, error) {
	return moduleapi.BuildCustomTool(spec, instruction, execute)
}
