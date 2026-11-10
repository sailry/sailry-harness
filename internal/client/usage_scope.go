package client

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type usageScopeKey struct{}

type usageScope struct {
	context protocol.PluginContext
	resolve bool
}

func WithUsageScope(ctx context.Context, target protocol.NodeID, scope protocol.PluginContext) context.Context {
	return withUsageScope(ctx, target, scope, false)
}

func WithMatchingUsageScope(ctx context.Context, target protocol.NodeID, scope protocol.PluginContext) context.Context {
	return withUsageScope(ctx, target, scope, true)
}

func withUsageScope(ctx context.Context, target protocol.NodeID, scope protocol.PluginContext, resolve bool) context.Context {
	previous, _ := ctx.Value(usageScopeKey{}).(map[protocol.NodeID]usageScope)
	scopes := make(map[protocol.NodeID]usageScope, len(previous)+1)
	for node, captured := range previous {
		scopes[node] = captured
	}
	scopes[target] = usageScope{context: copyValue(scope), resolve: resolve}
	return context.WithValue(ctx, usageScopeKey{}, scopes)
}

func usageScopeFor(ctx context.Context, target protocol.NodeID) (usageScope, bool) {
	scopes, _ := ctx.Value(usageScopeKey{}).(map[protocol.NodeID]usageScope)
	scope, exists := scopes[target]
	return scope, exists
}
