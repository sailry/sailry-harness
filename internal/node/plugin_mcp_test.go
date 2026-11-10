package node

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestMCPPackageAndScriptCapture(t *testing.T) {
	t.Run("conversation selects MCP and preserves scripted native receipt", pluginMCPConversationAcceptance)
	t.Run("assistant conversation uses its declared workspace, packages, tools and context", pluginAssistantAcceptance)
	t.Run("startup retains safe unavailable diagnostics and terminal cleanup", func(t *testing.T) {
		invocation := &store.AgentInvocation{}
		info := protocol.DefaultPluginInfo()
		info.Summary.Name, info.Summary.Digest = "unavailable-fixture", strings.Repeat("a", 64)
		unavailable := &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "private-token https://private.test"}
		tools, err := (&Node{}).bindMCPTools(t.Context(), invocation, []pluginMCPBinding{{info: info, server: "first", failure: unavailable}, {info: info, server: "second", failure: unavailable}})
		if err != nil || len(tools) != 0 || len(invocation.Instructions) != 2 {
			t.Fatalf("ordinary MCP startup failure became fatal: %v, %+v", err, invocation.Instructions)
		}
		for _, instruction := range invocation.Instructions {
			if strings.Contains(instruction.Text, "private-token") || strings.Contains(instruction.Text, "https://") || !strings.Contains(instruction.Text, "do not claim to use it") {
				t.Fatalf("unsafe MCP startup diagnostic: %s", instruction.Text)
			}
		}
		unknown := &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "cleanup could not be confirmed"}
		_, err = (&Node{}).bindMCPTools(t.Context(), invocation, []pluginMCPBinding{{info: info, server: "third", failure: errors.Join(unavailable, unknown)}})
		var failure *protocol.Fault
		if !errors.As(err, &failure) || failure.Code != protocol.ErrorCodeOutcomeUnknown {
			t.Fatalf("unknown cleanup was hidden by startup failure: %v", err)
		}
	})
	t.Run("script native wrapper preserves exact approval lifecycle", func(t *testing.T) {
		node := nodeFixture(t)
		turn := protocol.NewTurnID()
		config := protocol.DefaultSessionConfig()
		config.Mode = protocol.WorkModeCode
		owner := &pluginScriptTurn{node: node, invocation: &store.AgentInvocation{Turn: protocol.QueuedTurn{Config: config}, Run: protocol.ConversationRun{Turn: turn}},
			info: protocol.DefaultPluginInfo(), scope: protocol.PluginContext{Package: protocol.PluginReference{Name: "native-fixture", Digest: strings.Repeat("a", 64)}, Turn: &turn, Surface: protocol.PluginDesktopSurface{Kind: "workspace"}},
			state: json.RawMessage("null"), bundle: plugins.ScriptBundle{Entry: "main.js", Files: map[string]string{"main.js": `
export function prepare(input) { return {path:input.path,content:input.content,expected_revision:input.expected_revision}; }
export function format(input) { return {captured:input.output}; }
`}}}
		prepared, approved, executed := false, false, false
		request := protocol.NewRequestID()
		bound := engine.Tool{Approval: true, ReadOnly: false}
		bound.Prepare = func(_ context.Context, call canonical.ToolRequest) error {
			prepared = string(call.Arguments) == `{"path":"captured.txt","content":"captured","expected_revision":"frozen"}`
			if !prepared {
				return errors.New("script preparation did not reach native capture")
			}
			return nil
		}
		bound.Approved = func(_ canonical.ToolRequest, _ protocol.ApprovalID, identity protocol.RequestID) {
			approved = prepared && identity == request
		}
		bound.Execute = func(_ context.Context, call canonical.ToolRequest) engine.ToolResult {
			executed = approved && call.ID == "captured-call"
			return engine.ToolResult{Value: map[string]bool{"committed": executed}}
		}
		format := "format"
		tool := owner.bindNative(engine.Tool{}, bound, protocol.ToolDeclaration{Name: "write"}, protocol.ToolHandler{Name: "prepare", Result: &format})
		call := canonical.ToolRequest{ID: "captured-call", Name: "write", Arguments: json.RawMessage(`{"path":"captured.txt","content":"captured","expected_revision":"frozen"}`)}
		if err := tool.Prepare(t.Context(), call); err != nil {
			t.Fatal(err)
		}
		tool.Approved(call, protocol.NewApprovalID(), request)
		result := tool.Execute(t.Context(), call)
		if !prepared || !approved || !executed || result.Failure != nil || !tool.Approval {
			t.Fatalf("native approval lifecycle was replaced: prepared=%v approved=%v executed=%v result=%+v", prepared, approved, executed, result)
		}
		if repeated := tool.Execute(t.Context(), call); repeated.Failure == nil {
			t.Fatal("a consumed native preparation was executed again")
		}
		cancelled, cancel := context.WithCancel(t.Context())
		cancel()
		call.ID = "cancelled-call"
		if err := tool.Prepare(cancelled, call); err == nil {
			t.Fatal("a cancelled script preparer reached the native owner")
		}
		if result := tool.Execute(t.Context(), call); result.Failure == nil {
			t.Fatal("a cancelled preparation was retained for execution")
		}
	})
	t.Run("delegation receives prepared arguments without native Prepare", func(t *testing.T) {
		node := nodeFixture(t)
		turn := protocol.NewTurnID()
		config := protocol.DefaultSessionConfig()
		config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
		owner := &pluginScriptTurn{node: node, invocation: &store.AgentInvocation{Turn: protocol.QueuedTurn{Config: config}, Run: protocol.ConversationRun{Turn: turn}},
			info: protocol.DefaultPluginInfo(), scope: protocol.PluginContext{Package: protocol.PluginReference{Name: "delegate-fixture", Digest: strings.Repeat("a", 64)}, Turn: &turn, Surface: protocol.PluginDesktopSurface{Kind: "workspace"}},
			state: json.RawMessage("null"), bundle: plugins.ScriptBundle{Entry: "main.js", Files: map[string]string{"main.js": `export function prepare(input){return {task:"prepared:"+input.task}}`}}}
		var captured canonical.ToolRequest
		bound := engine.Tool{Delegation: true, Execute: func(_ context.Context, call canonical.ToolRequest) engine.ToolResult {
			captured = call
			return engine.ToolResult{Value: map[string]bool{"admitted": true}}
		}}
		tool := owner.bindNative(engine.Tool{}, bound, protocol.ToolDeclaration{Name: "delegate"}, protocol.ToolHandler{Name: "prepare"})
		call := canonical.ToolRequest{ID: "delegate-call", Name: "delegate", Arguments: json.RawMessage(`{"task":"captured"}`)}
		if err := tool.Prepare(t.Context(), call); err != nil {
			t.Fatal(err)
		}
		result := tool.Execute(t.Context(), call)
		if !tool.Delegation || tool.Approval || result.Failure != nil || captured.ID != call.ID || string(captured.Arguments) != `{"task":"prepared:captured"}` {
			t.Fatalf("delegation bypassed its prepared arguments: %+v, %+v", captured, result)
		}
	})
	t.Run("standalone uses immutable empty credential slots", func(t *testing.T) {
		node := &Node{plugins: &pluginControls{manager: plugins.NewManager(t.TempDir())}}
		definition := protocol.PluginMcpDefinition{Kind: "streamable-http", Data: &protocol.PluginMcpDefinitionStreamableHttpPayload{
			Url: "https://example.test/mcp", Headers: []string{"Authorization"},
		}}
		info, err := node.prepareStandaloneMCP(t.Context(), protocol.CommandInstallMcpPayload{Name: "mcp-fixture", Definition: definition})
		if err != nil {
			t.Fatal(err)
		}
		if info.McpSource == nil || info.Settings == nil || info.Summary.Version == nil || *info.Summary.Version != "0.1.0" {
			t.Fatalf("standalone metadata = %+v", info)
		}
		body, err := node.plugins.manager.ReadResource(t.Context(), plugins.PackageReference(info), "mcp.json", 256*1024)
		if err != nil {
			t.Fatal(err)
		}
		if !strings.Contains(string(body), `"Authorization":""`) || strings.Contains(string(body), "captured-secret") {
			t.Fatalf("standalone package credential projection = %s", body)
		}
		definition.Data.(*protocol.PluginMcpDefinitionStreamableHttpPayload).Headers = []string{"Authorization", "authorization"}
		if _, err := standaloneMCPDocuments("mcp-invalid", definition); err == nil {
			t.Fatal("case-colliding credential slots were accepted")
		}
	})
	t.Run("settings projection cleanup retains data directory identity", func(t *testing.T) {
		node := nodeFixture(t)
		definition := protocol.PluginMcpDefinition{Kind: "stdio", Data: &protocol.PluginMcpDefinitionStdioPayload{Command: "node", Args: []string{"${PLUGIN_ROOT}", "${PLUGIN_DATA}"}, Env: []string{}}}
		info, err := node.prepareStandaloneMCP(t.Context(), protocol.CommandInstallMcpPayload{Name: "mcp-projection", Definition: definition})
		if err != nil {
			t.Fatal(err)
		}
		declared, err := node.declaredPluginMCP(t.Context(), plugins.PackageReference(info))
		if err != nil {
			t.Fatal(err)
		}
		launch, err := node.prepareMCPLaunch(t.Context(), info, store.ResolvedPluginSettings{Values: map[string]json.RawMessage{"label": json.RawMessage(`"Public"`)}}, "server", declared.Servers["server"])
		if err != nil {
			t.Fatal(err)
		}
		projection := launch.Launch.Env["SAILRY_PLUGIN_SETTINGS_FILE"]
		body, err := os.ReadFile(projection)
		if err != nil || string(body) != `{"label":"Public"}` {
			t.Fatalf("projection = %q, %v", body, err)
		}
		owned := filepath.Dir(projection)
		retained := owned + "-retained"
		if err := os.Rename(owned, retained); err != nil {
			t.Fatal(err)
		}
		if err := os.Mkdir(owned, 0o700); err != nil {
			t.Fatal(err)
		}
		other := filepath.Join(owned, filepath.Base(projection))
		if err := os.WriteFile(other, []byte("replacement"), 0o600); err != nil {
			t.Fatal(err)
		}
		if err := launch.Release(); err != nil {
			t.Fatal(err)
		}
		if _, err := os.Stat(filepath.Join(retained, filepath.Base(projection))); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("owned projection remained: %v", err)
		}
		if body, err := os.ReadFile(other); err != nil || string(body) != "replacement" {
			t.Fatal("cleanup touched a replacement data directory")
		}
		if expandMCP("${PLUGIN_ROOT}/${PLUGIN_DATA}", "${PLUGIN_DATA}", "/data") != "${PLUGIN_DATA}//data" {
			t.Fatal("MCP variables were recursively expanded")
		}
	})
	t.Run("script turn state commits only after a successful evaluation", func(t *testing.T) {
		node := nodeFixture(t)
		config := protocol.DefaultSessionConfig()
		config.Mode = protocol.WorkModeCode
		turn := protocol.NewTurnID()
		owner := &pluginScriptTurn{node: node, invocation: &store.AgentInvocation{Turn: protocol.QueuedTurn{Config: config}, Run: protocol.ConversationRun{Turn: turn}},
			info: protocol.DefaultPluginInfo(), scope: protocol.PluginContext{Package: protocol.PluginReference{Name: "script-fixture", Digest: strings.Repeat("a", 64)}, Turn: &turn, Surface: protocol.PluginDesktopSurface{Kind: "workspace"}},
			state: json.RawMessage("null"), bundle: plugins.ScriptBundle{Entry: "main.js", Files: map[string]string{"main.js": `
import {readTurnState,stageTurnState} from "sailry/sdk";
export function save(value) { stageTurnState(value); return readTurnState(); }
export function fail(value) { stageTurnState(value); throw new Error("fixture failure"); }
export function read() { return readTurnState(); }
`}}}
		if _, err := owner.evaluate(context.Background(), "save", "tool", "first", map[string]string{"state": "captured"}, false); err != nil {
			t.Fatal(err)
		}
		if _, err := owner.evaluate(context.Background(), "fail", "tool", "second", map[string]string{"state": "failed"}, false); err == nil {
			t.Fatal("script failure was lost")
		}
		body, err := owner.evaluate(context.Background(), "read", "tool", "third", nil, false)
		if err != nil || string(body) != `{"state":"captured"}` {
			t.Fatalf("failed script committed staged turn state: %s %v", body, err)
		}
		if pluginHandlerReadOnly(protocol.ToolHandler{}, []protocol.PluginAction{protocol.PluginActionWriteStorage}) {
			t.Fatal("plain write handler was implicitly read-only")
		}
		if !pluginHandlerReadOnly(protocol.ToolHandler{}, []protocol.PluginAction{protocol.PluginActionReadStorage}) {
			t.Fatal("plain read handler lost read-only classification")
		}
	})
}
