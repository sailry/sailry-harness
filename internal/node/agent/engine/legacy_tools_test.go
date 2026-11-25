package engine

import (
	"encoding/json"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestLegacyNativeAliasesPreserveCapturedPluginAuthority(t *testing.T) {
	t.Parallel()
	for _, row := range []struct{ pack, server, old, native string }{
		{"progress", "", operations.UpdatePlanTool, operations.UpdatePlanTool},
		{"commands", "", "write_command", "write_command"},
		{"files", "", "read_file", "read_file"},
		{"database", "database", "query", "database_query"},
	} {
		t.Run(row.pack, func(t *testing.T) {
			native := objectTool(row.native, true, true, nil)
			native.Deferred = true
			native.LegacyPackage, native.LegacyServer, native.LegacyName = row.pack, row.server, row.old
			plugin := packageTool(row.pack, row.old, "", false, nil)
			if row.server != "" {
				plugin.Definition.Name = mcp.Alias(row.pack, row.server, row.old)
				plugin.Binding.Tool = plugin.Definition.Name
				plugin.Exposed = MCPToolName(row.server, row.old)
			}
			// Even an inconsistent package registration cannot claim a native
			// migration identity while it still has a captured package owner.
			plugin.LegacyPackage, plugin.LegacyName = row.pack, row.old
			if plugin.LegacyAlias() != "" {
				t.Fatal("a package claimed a native migration identity")
			}
			legacy := plugin.Definition.Name
			tools := []Tool{native, plugin, callTool()}
			names := captureExposure(tools, []string{legacy})
			request := canonical.ToolRequest{ID: "legacy", Name: legacy, Arguments: json.RawMessage(`{}`)}
			names.restoreCall(&request)
			if request.Name != legacy {
				t.Fatalf("captured plugin call became native: %s", request.Name)
			}
			selected := findTool(tools, request.Name)
			if selected == nil || selected.Binding != plugin.Binding || nativeTool(*selected) {
				t.Fatal("legacy call lost its captured package approval binding")
			}
			if toolLoaded([]string{legacy}, native, names) || !toolLoaded([]string{legacy}, plugin, names) {
				t.Fatal("saved package discovery state loaded the native replacement")
			}
			input, _ := json.Marshal(dispatched{Name: legacy, Arguments: json.RawMessage(`{}`)})
			request = canonical.ToolRequest{Name: callToolName, Arguments: input}
			names.restoreCall(&request)
			if request.Name != legacy {
				t.Fatal("call_tool substituted native authority for a captured package")
			}
			if row.server == "" {
				selector := protocol.PluginConversationTool{Kind: "package", Data: &protocol.PluginConversationToolPackagePayload{Package: row.pack, Name: row.old}}
				aliases := legacyToolAliases(tools)
				if roleToolMatches(selector, native, aliases) || !roleToolMatches(selector, plugin, aliases) {
					t.Fatal("a package role selector selected native authority")
				}
				fixture, x := delegationFixture(t)
				x.tools = tools
				x.inv.Turn.Config.Assistant = &protocol.PluginConversationBinding{Package: protocol.PluginReference{Name: "assistant"}, ID: "fixture"}
				x.inv.Assistant = &protocol.PluginConversationDeclaration{Tools: []protocol.PluginConversationTool{selector}}
				if err := fixture.r.applyAssistant(x); err != nil {
					t.Fatal(err)
				}
				if !x.tools[0].Deferred || x.tools[1].Deferred {
					t.Fatal("a package assistant selector selected the native replacement")
				}
			}
		})
	}
}

func TestLegacyNativeHistoryUsesCapturedReplacement(t *testing.T) {
	t.Parallel()
	for _, row := range []struct{ pack, server, old, native string }{
		{"progress", "", operations.UpdatePlanTool, operations.UpdatePlanTool},
		{"commands", "", "write_command", "write_command"},
		{"files", "", "read_file", "read_file"},
		{"database", "database", "query", "database_query"},
	} {
		t.Run(row.pack, func(t *testing.T) {
			native := objectTool(row.native, true, true, nil)
			native.Deferred = true
			native.LegacyPackage, native.LegacyServer, native.LegacyName = row.pack, row.server, row.old
			legacy := native.LegacyAlias()
			names := captureExposure([]Tool{native, callTool()}, []string{legacy})
			if names.name(legacy) != row.native || names.alias(legacy) != row.native || !toolLoaded([]string{legacy}, native, names) {
				t.Fatal("saved native replacement identity or discovery state was lost")
			}
			request := canonical.ToolRequest{ID: "legacy", Name: legacy, Arguments: json.RawMessage(`{}`)}
			answer := canonical.ToolResponse{ID: request.ID, Name: legacy}
			records := []canonical.Record{
				canonical.NewRecord(protocol.NewTurnID(), "message", canonical.NewMessage(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: &request}), manualNow()),
				canonical.NewRecord(protocol.NewTurnID(), "message", canonical.NewMessage(canonical.User, canonical.Block{Kind: "tool_response", Data: &answer}), manualNow()),
			}
			before, _ := json.Marshal(records)
			projected, _, _ := names.request(records, []fantasy.FunctionTool{{Name: callToolName}}, nil)
			call := projected[0].Message().Content[0].Data.(*canonical.ToolRequest)
			response := projected[1].Message().Content[0].Data.(*canonical.ToolResponse)
			var dispatchedCall dispatched
			if call.Name != callToolName || json.Unmarshal(call.Arguments, &dispatchedCall) != nil || dispatchedCall.Name != row.native || response.Name != callToolName {
				t.Fatal("deferred historical native call was not projected through call_tool")
			}
			after, _ := json.Marshal(records)
			if string(before) != string(after) {
				t.Fatal("history projection rewrote canonical records")
			}
			input, _ := json.Marshal(dispatched{Name: legacy, Arguments: json.RawMessage(`{}`)})
			restored := canonical.ToolRequest{Name: callToolName, Arguments: input}
			names.restoreCall(&restored)
			if restored.Name != row.native {
				t.Fatal("historical call_tool did not resolve its captured native replacement")
			}
		})
	}
}

func TestLegacyAliasesDoNotClaimAnotherPackage(t *testing.T) {
	t.Parallel()
	plugin := Tool{Definition: fantasy.FunctionTool{Name: "write_command"}, Binding: &store.ApprovalBinding{Owner: protocol.PluginReference{Name: "third-party"}}}
	legacy := operations.Alias("commands", "write_command")
	names := captureExposure([]Tool{plugin}, []string{legacy})
	if names.name(legacy) != legacy || names.alias(legacy) != legacy {
		t.Fatal("a package tool claimed the command module's legacy alias")
	}
}
