package websearchnode

import (
	"context"
	"io/fs"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type webToolHost struct {
	moduleapi.Host
	operations map[string]protocol.ToolOperation
}

func (host webToolHost) WorkspaceTool(_ *store.AgentInvocation, cfg moduleapi.WorkspaceToolConfig) (engine.Tool, error) {
	host.operations[cfg.Spec.Name] = cfg.Spec.Operation
	return engine.Tool{Definition: fantasy.FunctionTool{Name: cfg.Spec.Name}}, nil
}

func TestWebToolsAndResearchSkill(t *testing.T) {
	module := Module()
	host := webToolHost{operations: make(map[string]protocol.ToolOperation)}
	tools, err := module.ToolsForTurn(t.Context(), host, &store.AgentInvocation{})
	if err != nil || len(tools) != 2 || host.operations["web_fetch"] != protocol.ToolOperationFetchWeb || host.operations["web_search"] != protocol.ToolOperationSearchWeb {
		t.Fatalf("web tools = %v, %v", host.operations, err)
	}
	skills := module.(moduleapi.SkillProvider).Skills()
	if len(skills) != 1 || skills[0].Package != "web-search" || skills[0].Skill.Name != "research" || skills[0].Skill.Description == "" {
		t.Fatalf("research registration = %+v", skills)
	}
	if body, err := fs.ReadFile(skills[0].Tree, skills[0].Skill.Path); err != nil || len(body) == 0 {
		t.Fatalf("registered research document is unavailable: %v", err)
	}
	toolCount := len(host.operations)
	inv := &store.AgentInvocation{}
	inv.Turn.Config.Resource = &protocol.ConnectionResource{Kind: "ssh", Data: protocol.NewSshID()}
	if got, err := module.ToolsForTurn(context.Background(), host, inv); err != nil || len(got) != 0 || len(host.operations) != toolCount {
		t.Fatalf("external-resource web tools = %v, %v", len(got), err)
	}
}
