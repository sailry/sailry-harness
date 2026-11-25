package sdktest

import (
	"context"

	"github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	nodemcp "github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/protocol"
	native "github.com/sailry/sailry-harness/sdk/native/node"
)

type Access = node.TestAccess

type Binding = model.Binding

func Inspect(n *native.Node) Access { return node.InspectForTesting(n) }

func MCPOptions() *nodemcp.Options {
	return &nodemcp.Options{Open: func(context.Context, nodemcp.Launch, func(context.Context, *mcp.ElicitRequest) (*mcp.ElicitResult, error)) (*mcp.ClientSession, error) {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture MCP server is unavailable"}
	}}
}
