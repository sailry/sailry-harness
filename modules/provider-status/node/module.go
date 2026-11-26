// Package providerstatusnode provides the compile-time native Harness Node
// module that fetches and normalizes account usage for configured OAuth model
// channels (ChatGPT / Codex OAuth and Claude OAuth).
package providerstatusnode

import (
	"context"

	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const moduleNamespace = "provider-status"

type module struct{}

// Module returns the native Harness Node module for provider account usage.
func Module() moduleapi.Module { return module{} }

var _ moduleapi.CommandHandler = module{}

func (module) ID() string                    { return moduleNamespace }
func (module) ReplacesPlugins() []string     { return []string{moduleNamespace} }
func (module) Catalog() []moduleapi.ToolMeta { return nil }

func (module) ToolsForTurn(context.Context, moduleapi.Host, *store.AgentInvocation) ([]engine.Tool, error) {
	return nil, nil
}

func (module) HandlesCommand(kind string) bool {
	return kind == "read_provider_account_usage"
}

func (module) ExecuteCommand(ctx context.Context, host moduleapi.Host, _ protocol.NodeID, request protocol.Request) (protocol.Output, []protocol.Event, error) {
	input, ok := request.Command.Data.(*protocol.CommandReadProviderAccountUsagePayload)
	if !ok || input == nil {
		return protocol.Output{}, nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "provider account usage request is invalid"}
	}
	auth, resp, err := host.ReadProviderAccountHTTP(ctx, input.Provider, input.ExpectedRevision, moduleNamespace, EndpointFor)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	usage, err := DecodeAccountUsage(auth, resp, host.Now())
	if err != nil {
		return protocol.Output{}, nil, err
	}
	return protocol.Output{Kind: "provider_account_usage", Data: usage}, nil, nil
}
