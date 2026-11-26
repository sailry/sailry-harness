package node

import (
	"context"
	"database/sql"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	browserReadOperation    protocol.ToolOperation = "browser.read"
	browserControlOperation protocol.ToolOperation = "browser.control"
)

var browserToolSchema = json.RawMessage(`{"type":"object","properties":{"action":{"type":"string"},"arguments":{"type":"object"}},"required":["action","arguments"],"additionalProperties":false}`)

// browserTools binds the browser operations a turn's packages declare to the
// initiating controller's browser. The package's host script prepares each
// call and formats its result around these native tools.
func (node *Node) browserTools(ctx context.Context, inv *store.AgentInvocation) ([]engine.Tool, error) {
	// Only the controller that started the turn has a browser to drive. A
	// turn started by Mobile, a Host or a schedule has none, and tools that
	// could only answer "disconnected" are not offered.
	if !node.browsers.available(inv.Caller) {
		return nil, nil
	}
	var infos []protocol.PluginInfo
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		for _, reference := range inv.Turn.Plugins {
			info, err := store.ReadPluginVersion(ctx, db, reference)
			if err != nil {
				return err
			}
			infos = append(infos, info)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	var tools []engine.Tool
	for _, info := range infos {
		if info.Extension == nil || info.Extension.Host == nil || node.hasReplacedPlugin(info.Summary.Name, info.Summary.Digest) {
			continue
		}
		owner := plugins.PackageReference(info)
		for _, declaration := range info.Extension.Tools {
			if declaration.Handler == nil || declaration.Handler.Operation == nil {
				continue
			}
			operation := *declaration.Handler.Operation
			if operation != browserReadOperation && operation != browserControlOperation {
				continue
			}
			name := plugins.ToolAlias(info.Summary.Name, declaration.Name)
			tools = append(tools, node.browserTool(inv, owner, name, operation, nil))
		}
	}
	return tools, nil
}

func (node *Node) browserTool(inv *store.AgentInvocation, owner protocol.PluginReference, name string, operation protocol.ToolOperation, _ map[string]any) engine.Tool {
	tool, err := nodeModuleHost{node: node}.buildBrowserTool(inv, owner, moduleapi.BrowserToolConfig{
		Spec: moduleapi.ToolSpec{
			Name:       name,
			Operation:  operation,
			Parameters: browserToolSchema,
			Initial:    true,
		},
	}, func(ctx context.Context) error {
		if owner.Name != "" {
			return node.pluginEnabled(ctx, owner.Name)
		}
		return nil
	})
	if err != nil {
		panic(err)
	}
	return tool
}
