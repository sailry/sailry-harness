package engine

import (
	"context"
	"encoding/json"
	"errors"
	"slices"
	"strings"
	"sync/atomic"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// packageTool is a captured package tool as the Node builds it: the alias is
// its identity, and Deferred carries the manifest's initial flag.
func packageTool(owner, name string, operation protocol.ToolOperation, initial bool, run func(context.Context, canonical.ToolRequest) ToolResult) Tool {
	tool := objectTool(operations.Alias(owner, name), true, true, run)
	tool.Deferred = !initial
	tool.Binding = &store.ApprovalBinding{Owner: protocol.PluginReference{Name: owner}, Tool: tool.Definition.Name}
	if operation != "" {
		tool.Binding.Operation = &operation
	} else {
		tool.Hosted = true
	}
	return tool
}

func offeredNames(call fantasy.Call) []string {
	var names []string
	for _, tool := range call.Tools {
		names = append(names, tool.GetName())
	}
	return names
}

func promptText(call fantasy.Call) string {
	var text strings.Builder
	for _, message := range call.Prompt {
		for _, part := range message.Content {
			if value, ok := fantasy.AsMessagePart[fantasy.TextPart](part); ok {
				text.WriteString(value.Text + "\n")
			}
			if value, ok := fantasy.AsMessagePart[fantasy.ToolResultPart](part); ok {
				if output, ok := value.Output.(fantasy.ToolResultOutputContentText); ok {
					text.WriteString(output.Text + "\n")
				}
			}
		}
	}
	return text.String()
}

func TestToolSelectionAndNames(t *testing.T) {
	t.Parallel()
	done := func(context.Context, canonical.ToolRequest) ToolResult {
		return ToolResult{Value: map[string]any{"ok": true}}
	}
	remote := func(owner, server, name string) Tool {
		tool := objectTool(mcp.Alias(owner, server, name), true, true, done)
		tool.Deferred = true
		tool.Binding = &store.ApprovalBinding{Owner: protocol.PluginReference{Name: owner}, Tool: tool.Definition.Name}
		tool.Exposed = MCPToolName(server, name)
		return tool
	}
	t.Run("core tools start visible under declared names and the rest are listed", func(t *testing.T) {
		t.Parallel()
		var read atomic.Int32
		tools := []Tool{
			packageTool("files", "read_file", protocol.ToolOperationReadFile, false, func(context.Context, canonical.ToolRequest) ToolResult {
				read.Add(1)
				return ToolResult{Value: map[string]any{"ok": true}}
			}),
			packageTool("files", "delete_file", protocol.ToolOperationDeleteFile, false, done),
			packageTool("commands", "run_command", protocol.ToolOperationRunCommand, false, done),
			packageTool("memory", "save_memory", "", true, done),
			packageTool("goals", "create_goal", "", false, done),
			remote("github", "github", "run trigger"),
		}
		model := &scriptedModel{scripts: []script{
			response(call("read", "read_file", `{}`), call("find", "tool_search", `{"query":"select:delete_file,mcp__github__run_trigger"}`), finish()),
			response(text("Final answer"), finish()),
		}}
		fixture := newFixture(t, model, tools...)
		fixture.start()
		fixture.waitRun(fixture.submit("Read the file"), protocol.ConversationStatusCompleted)
		if read.Load() != 1 {
			t.Fatalf("declared name did not reach its tool: %d calls", read.Load())
		}
		first := offeredNames(model.calls[0])
		for _, name := range []string{"read_file", "run_command", "save_memory", "read_tool_output", "tool_search", "set_session_title", "ask_user"} {
			if !slices.Contains(first, name) {
				t.Errorf("first request lacks %q: %v", name, first)
			}
		}
		for _, name := range first {
			if strings.HasPrefix(name, "plugin_") || strings.HasPrefix(name, "mcp_") || slices.Contains([]string{"delete_file", "create_goal", "list_sessions", "read_session", "search_session"}, name) {
				t.Errorf("first request offers %q", name)
			}
		}
		if context := promptText(model.calls[0]); !strings.Contains(context, "Deferred tools\ncreate_goal, delete_file, guide_agent, list_sessions, mcp__github__run_trigger, read_session, search_session, spawn_agent, wait_agent\n") {
			t.Fatalf("deferred names are not listed:\n%s", context)
		}
		second := offeredNames(model.calls[1])
		if !slices.Contains(second, "delete_file") || !slices.Contains(second, "mcp__github__run_trigger") || slices.Contains(second, "create_goal") {
			t.Fatalf("discovery did not load the selected tools: %v", second)
		}
		// The history a model reads uses the names it called.
		var called []string
		for _, message := range model.calls[1].Prompt {
			for _, part := range message.Content {
				if value, ok := fantasy.AsMessagePart[fantasy.ToolCallPart](part); ok {
					called = append(called, value.ToolName+" "+value.Input)
				}
			}
		}
		if len(called) != 2 || called[0] != "read_file {}" || !strings.Contains(called[1], "select:delete_file,mcp__github__run_trigger") {
			t.Fatalf("history calls=%v", called)
		}
		if result := promptText(model.calls[1]); !strings.Contains(result, `"name":"delete_file"`) || !strings.Contains(result, `"name":"mcp__github__run_trigger"`) || strings.Contains(result, "plugin_") {
			t.Fatalf("discovery result names:\n%s", result)
		}
		// Records, loaded tools and discovery keep the canonical aliases.
		state := fixture.state()
		for _, alias := range []string{operations.Alias("files", "delete_file"), mcp.Alias("github", "github", "run trigger")} {
			if !slices.Contains(state.LoadedTools, alias) {
				t.Errorf("loaded tools %v lack %q", state.LoadedTools, alias)
			}
		}
		stored := map[string]string{}
		for _, record := range state.Records {
			if message := record.Message(); message != nil {
				for _, block := range message.Content {
					if request, ok := block.Data.(*canonical.ToolRequest); ok {
						stored[request.ID] = request.Name + " " + string(request.Arguments)
					}
				}
			}
		}
		if stored["read"] != operations.Alias("files", "read_file")+" {}" || !strings.Contains(stored["find"], "select:"+operations.Alias("files", "delete_file")+","+mcp.Alias("github", "github", "run trigger")) {
			t.Fatalf("stored calls=%v", stored)
		}
	})
	t.Run("a child has no parent-only built-ins and a listed role defers nothing", func(t *testing.T) {
		fixture, x := delegationFixture(t)
		delegation := &protocol.Delegation{}
		x.inv.Session.Delegation = &delegation
		fixture.r.tools = []Tool{
			packageTool("files", "read_file", protocol.ToolOperationReadFile, false, done),
			packageTool("files", "delete_file", protocol.ToolOperationDeleteFile, false, done),
			packageTool("memory", "search_memory", "", true, done),
		}
		capture := func(role *protocol.RoleProfile) (visible, deferred []string, err error) {
			x.childRole = role
			if err = fixture.r.captureTools(x); err != nil {
				return nil, nil, err
			}
			for _, tool := range x.tools {
				if tool.Deferred {
					deferred = append(deferred, tool.Definition.Name)
				} else {
					visible = append(visible, tool.Definition.Name)
				}
			}
			return visible, deferred, nil
		}
		visible, deferred, err := capture(nil)
		if err != nil {
			t.Fatal(err)
		}
		// A declared initial tool starts visible only to the main agent.
		if !slices.Equal(visible, []string{"read_tool_output", operations.Alias("files", "read_file"), "tool_search"}) ||
			!slices.Equal(deferred, []string{"list_sessions", "read_session", "search_session", operations.Alias("files", "delete_file"), operations.Alias("memory", "search_memory")}) {
			t.Fatalf("standard child visible=%v deferred=%v", visible, deferred)
		}
		selection := func(kind string, data any) protocol.PluginConversationTool {
			return protocol.PluginConversationTool{Kind: kind, Data: data}
		}
		listed := []protocol.PluginConversationTool{selection("package", &protocol.PluginConversationToolPackagePayload{Package: "files", Name: "delete_file"}), selection("builtin", &protocol.PluginConversationToolBuiltinPayload{Name: "list_sessions"})}
		visible, deferred, err = capture(&protocol.RoleProfile{Tools: &listed})
		if err != nil {
			t.Fatal(err)
		}
		if !slices.Equal(visible, []string{"list_sessions", "read_tool_output", operations.Alias("files", "delete_file")}) || len(deferred) != 0 {
			t.Fatalf("listed child visible=%v deferred=%v", visible, deferred)
		}
		for _, name := range []string{"ask_user", "set_session_title"} {
			parentOnly := []protocol.PluginConversationTool{selection("builtin", &protocol.PluginConversationToolBuiltinPayload{Name: name})}
			_, _, err = capture(&protocol.RoleProfile{Tools: &parentOnly})
			var failure *protocol.Fault
			if !errors.As(err, &failure) || failure.Code != protocol.ErrorCodeNotConfigured || failure.Message != "role references an unavailable tool" {
				t.Fatalf("a role gave %s to a child: %v", name, err)
			}
		}
	})
	t.Run("collisions keep an unambiguous name", func(t *testing.T) {
		t.Parallel()
		long := strings.Repeat("s", 40)
		tools := []Tool{
			packageTool("first", "read", protocol.ToolOperationReadFile, false, done),
			packageTool("second", "read", protocol.ToolOperationReadFile, false, done),
			packageTool("third", "ask_user", protocol.ToolOperationReadFile, false, done),
			packageTool("third", "earlier", protocol.ToolOperationReadFile, false, done),
			packageTool("third", "bad name", protocol.ToolOperationReadFile, false, done),
			packageTool("third", "write", protocol.ToolOperationWriteFile, false, done),
			objectTool("ask_user", false, false, done),
			remote("one", "service", "read"),
			remote("two", "service", "read"),
			remote("one", long, long),
			remote("one", "service", "write"),
		}
		names := captureExposure(tools, []string{"earlier"})
		for _, alias := range []string{operations.Alias("first", "read"), operations.Alias("second", "read"), operations.Alias("third", "ask_user"), operations.Alias("third", "earlier"), operations.Alias("third", "bad name")} {
			if names.name(alias) != alias {
				t.Errorf("%q is exposed as %q", alias, names.name(alias))
			}
		}
		if names.name(operations.Alias("third", "write")) != "write" || names.alias("write") != operations.Alias("third", "write") {
			t.Fatal("a unique declared name is not exposed")
		}
		one, two := names.name(mcp.Alias("one", "service", "read")), names.name(mcp.Alias("two", "service", "read"))
		if one == two || !strings.HasPrefix(one, "mcp__service__read_") || !strings.HasPrefix(two, "mcp__service__read_") || len(one) != len("mcp__service__read_")+toolHashChars {
			t.Fatalf("equal MCP names are not separated: %q %q", one, two)
		}
		if name := names.name(mcp.Alias("one", long, long)); len(name) != maxToolName || !providerToolName(name) || names.alias(name) != mcp.Alias("one", long, long) {
			t.Fatalf("overlong MCP name=%q", name)
		}
		if names.name(mcp.Alias("one", "service", "write")) != "mcp__service__write" {
			t.Fatal("a unique MCP name is not exposed")
		}
		for _, tool := range tools {
			if name := names.name(tool.Definition.Name); !providerToolName(name) && name != operations.Alias("third", "bad name") {
				t.Errorf("%q breaks the provider name rule", name)
			}
		}
	})
	t.Run("request and response cross the boundary without touching records", func(t *testing.T) {
		t.Parallel()
		alias := operations.Alias("files", "read_file")
		names := captureExposure([]Tool{packageTool("files", "read_file", protocol.ToolOperationReadFile, false, done)}, nil)
		request := canonical.ToolRequest{ID: "a", Name: alias, Arguments: json.RawMessage(`{}`)}
		failed := canonical.ToolRequest{ID: "b", Name: alias, Arguments: json.RawMessage(`{}`), Error: &canonical.ToolError{Message: "malformed"}}
		record := canonical.NewRecord(protocol.NewTurnID(), "message", canonical.NewMessage(canonical.Assistant,
			canonical.Block{Index: 0, Kind: "tool_request", Data: &request}, canonical.Block{Index: 1, Kind: "tool_request", Data: &failed}), manualNow())
		answer := canonical.NewRecord(record.Turn, "message", canonical.NewMessage(canonical.User,
			canonical.Block{Index: 0, Kind: "tool_response", Data: &canonical.ToolResponse{ID: "a", Name: alias}}), manualNow())
		records, tools, schemas := names.request([]canonical.Record{record, answer}, []fantasy.FunctionTool{{Name: alias}, {Name: "ask_user"}}, map[string]json.RawMessage{alias: json.RawMessage(`{}`)})
		sent := records[0].Message().Content
		if sent[0].Data.(*canonical.ToolRequest).Name != "read_file" || sent[1].Data.(*canonical.ToolRequest).Name != alias || records[1].Message().Content[0].Data.(*canonical.ToolResponse).Name != "read_file" {
			t.Fatalf("history names=%+v", sent)
		}
		if tools[0].Name != "read_file" || tools[1].Name != "ask_user" || schemas["read_file"] == nil || len(schemas) != 1 {
			t.Fatalf("declarations=%v schemas=%v", tools, schemas)
		}
		if request.Name != alias || record.Message().Content[0].Data.(*canonical.ToolRequest).Name != alias {
			t.Fatal("the stored record was renamed")
		}
		received := []canonical.Block{{Kind: "tool_request", Data: &canonical.ToolRequest{ID: "c", Name: "read_file"}}, {Kind: "tool_request", Data: &canonical.ToolRequest{ID: "d", Name: "unknown"}}}
		names.restore(received)
		if received[0].Data.(*canonical.ToolRequest).Name != alias || received[1].Data.(*canonical.ToolRequest).Name != "unknown" {
			t.Fatalf("restored names=%+v", received)
		}
	})
}
