package engine

import (
	"context"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAssistantPreferencesKeepOrdinaryToolDiscovery(t *testing.T) {
	t.Parallel()
	fixture, x := delegationFixture(t)
	done := func(context.Context, canonical.ToolRequest) ToolResult { return ToolResult{Value: true} }
	fixture.r.tools = []Tool{
		packageTool("files", "read_file", protocol.ToolOperationReadFile, false, done),
		packageTool("commands", "run_command", protocol.ToolOperationRunCommand, false, done),
		packageTool("assistant", "preferred", "", false, done),
		packageTool("other", "discoverable", "", false, done),
	}
	x.inv.Turn.Config.Resource = &protocol.ConnectionResource{Kind: "ssh", Data: protocol.SshID{1}}
	x.inv.Turn.Config.Assistant = &protocol.PluginConversationBinding{Package: protocol.PluginReference{Name: "assistant"}, ID: "ssh"}
	legacy := "Work only with the primary connection. Never access another connection."
	x.inv.Assistant = &protocol.PluginConversationDeclaration{Context: legacy, Tools: []protocol.PluginConversationTool{
		{Kind: "plugin", Data: &protocol.PluginConversationToolPluginPayload{Name: "preferred"}},
		{Kind: "builtin", Data: &protocol.PluginConversationToolBuiltinPayload{Name: "read_session"}},
	}}
	if err := fixture.r.captureTools(x); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"ask_user", "set_session_title", "read_tool_output", "read_session", operations.Alias("files", "read_file"), operations.Alias("commands", "run_command"), operations.Alias("assistant", "preferred")} {
		if tool := findTool(x.tools, name); tool == nil || tool.Deferred {
			t.Fatalf("the assistant lost an ordinary or preferred initial tool: %s", name)
		}
	}
	if tool := findTool(x.tools, operations.Alias("other", "discoverable")); tool == nil || !tool.Deferred || findTool(x.tools, "tool_search") == nil {
		t.Fatal("the assistant removed ordinary discovery or eagerly injected the whole catalog")
	}
	if !strings.HasPrefix(x.assistantContext, legacy+"\n\nAssistant runtime capabilities\n") ||
		!strings.Contains(x.assistantContext, "do not restrict the ordinary conversation catalog or skills") ||
		!strings.Contains(x.assistantContext, "bound resource is the default target") || x.inv.Assistant.Context != legacy {
		t.Fatal("the assistant lost immutable context or retained its obsolete capability restriction")
	}
}
