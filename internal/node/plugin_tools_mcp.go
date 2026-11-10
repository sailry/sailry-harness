package node

import (
	"context"
	"errors"
	"fmt"
	"sync"

	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginMCPBinding struct {
	info          protocol.PluginInfo
	settings      store.ResolvedPluginSettings
	server        string
	configuration protocol.PluginMcpServer
	failure       error
}

func (node *Node) bindMCPTools(ctx context.Context, invocation *store.AgentInvocation, bindings []pluginMCPBinding) ([]engine.Tool, error) {
	type result struct {
		tools []engine.Tool
		err   error
	}
	results := make([]result, len(bindings))
	var workers sync.WaitGroup
	for index, binding := range bindings {
		if index >= 16 {
			results[index].err = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "MCP server limit exceeded"}
			continue
		}
		workers.Add(1)
		go func() {
			defer workers.Done()
			results[index].tools, results[index].err = node.connectMCPTools(ctx, invocation, binding)
		}()
	}
	workers.Wait()
	var tools []engine.Tool
	var terminal error
	diagnosticBytes := 0
	for index, result := range results {
		if result.err == nil && len(tools)+len(result.tools) > 64 {
			for _, tool := range result.tools {
				if tool.Close != nil {
					terminal = errors.Join(terminal, tool.Close())
				}
			}
			result.tools, result.err = nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "MCP tool limit exceeded"}
		}
		tools = append(tools, result.tools...)
		if result.err == nil {
			continue
		}
		if failure := terminalMCPBindingError(result.err); failure != nil {
			terminal = errors.Join(terminal, failure)
		}
		if diagnosticBytes < 16*1024 {
			binding := bindings[index]
			message := fmt.Sprintf("MCP server %s/%s is unavailable; do not claim to use it.", binding.info.Summary.Name, binding.server)
			diagnosticBytes += len(message)
			invocation.Instructions = append(invocation.Instructions, store.AgentInstruction{Text: message})
		}
	}
	return tools, terminal
}

func (node *Node) connectMCPTools(ctx context.Context, invocation *store.AgentInvocation, binding pluginMCPBinding) ([]engine.Tool, error) {
	if ctx.Err() != nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "MCP startup was cancelled"}
	}
	if binding.failure != nil {
		return nil, binding.failure
	}
	prepared, err := node.prepareMCPLaunch(ctx, binding.info, binding.settings, binding.server, binding.configuration)
	if err != nil {
		return nil, err
	}
	lease, err := node.plugins.mcp.Acquire(ctx, prepared)
	if err != nil {
		return nil, err
	}
	descriptors, err := lease.List(ctx)
	if err != nil {
		return nil, errors.Join(err, lease.Close())
	}
	var closeOnce sync.Once
	var closeErr error
	close := func() error { closeOnce.Do(func() { closeErr = lease.Close() }); return closeErr }
	var tools []engine.Tool
	for _, descriptor := range descriptors {
		declaration, accepted := selectedMCPTool(invocation, binding.info, binding.server, descriptor.Name)
		if !accepted {
			continue
		}
		tool, err := node.mcpTool(invocation, prepared.Package, binding.server, lease, descriptor, declaration, prepared.Options)
		if err != nil {
			return nil, errors.Join(err, close())
		}
		tool.Close = close
		tools = append(tools, tool)
	}
	if len(tools) == 0 {
		return nil, close()
	}
	return tools, nil
}

func terminalMCPBindingError(err error) error {
	if errors.Is(err, context.Canceled) {
		return &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "MCP startup was cancelled"}
	}
	if failure, ok := err.(*protocol.Fault); ok && (failure.Code == protocol.ErrorCodeCancelled || failure.Code == protocol.ErrorCodeOutcomeUnknown) {
		return failure
	}
	if joined, ok := err.(interface{ Unwrap() []error }); ok {
		for _, cause := range joined.Unwrap() {
			if failure := terminalMCPBindingError(cause); failure != nil {
				return failure
			}
		}
	} else if wrapped, ok := err.(interface{ Unwrap() error }); ok {
		return terminalMCPBindingError(wrapped.Unwrap())
	}
	return nil
}
