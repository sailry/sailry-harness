package node

import (
	"context"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestAccess keeps white-box integration checks at the runtime's ownership boundary.
type TestAccess struct{ node *Node }

func InspectForTesting(node *Node) TestAccess { return TestAccess{node: node} }

func (access TestAccess) Store() *store.Store { return access.node.profile.Store }

func (access TestAccess) InstallPlugin(ctx context.Context, directory, workingDirectory, name string) (protocol.PluginInfo, error) {
	return access.node.plugins.manager.Install(ctx, directory, workingDirectory, name)
}

func (access TestAccess) ModuleTools(ctx context.Context, invocation *store.AgentInvocation) ([]engine.Tool, error) {
	return access.node.moduleTools(ctx, invocation)
}

func (access TestAccess) SwapAssembly(assembly moduleapi.Assembly) moduleapi.Assembly {
	previous := access.node.modules
	access.node.modules = assembly
	access.node.registeredModules = assembly.ResolveServices().Modules
	return previous
}

func (access TestAccess) ConnectionCommand(ctx context.Context, invocation *store.AgentInvocation, declaration operations.Declaration, arguments json.RawMessage, captured []protocol.ConnectionResource) (protocol.Command, error) {
	return access.node.connectionCommand(ctx, invocation, declaration, arguments, captured)
}

func (access TestAccess) ConnectionResult(ctx context.Context, invocation *store.AgentInvocation, owner protocol.PluginReference, output protocol.Output, captured []protocol.ConnectionResource) (any, error) {
	return access.node.connectionResult(ctx, invocation, owner, output, captured)
}

func (access TestAccess) ConnectionTurnInput(ctx context.Context, invocation *store.AgentInvocation, selected []protocol.ToolOperation) (map[string]any, error) {
	return access.node.connectionTurnInput(ctx, invocation, selected)
}

func (access TestAccess) AuthorizeCommand(ctx context.Context, caller protocol.NodeID, request protocol.Request) (context.Context, error) {
	return access.node.authorizeCommand(ctx, caller, request)
}

func (access TestAccess) ValidatePluginRequest(ctx context.Context, caller protocol.NodeID, request protocol.Request) error {
	return access.node.validatePluginRequest(ctx, caller, request)
}

func (access TestAccess) ExecuteAdmittedHost(ctx context.Context, caller protocol.NodeID, request protocol.Request) link.Response {
	worker := access.node.commandContext(commandJob{user: userFromContext(ctx), scope: ctx})
	return access.node.executeHost(worker, caller, request)
}

func (access TestAccess) CodeQueryRunning(caller protocol.NodeID, request protocol.RequestID) bool {
	controls := access.node.codeIndex
	if controls == nil {
		return false
	}
	controls.mu.Lock()
	defer controls.mu.Unlock()
	_, running := controls.running[codeQueryKey{caller, request}]
	return running
}

func (access TestAccess) CodeQueryCount() int {
	controls := access.node.codeIndex
	if controls == nil {
		return 0
	}
	controls.mu.Lock()
	defer controls.mu.Unlock()
	return len(controls.running)
}
