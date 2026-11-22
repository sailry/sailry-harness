package store

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type unavailablePluginKey struct{}

func WithPluginAvailability(ctx context.Context, check func(protocol.PluginReference) error) context.Context {
	if check == nil {
		return ctx
	}
	return context.WithValue(ctx, unavailablePluginKey{}, check)
}

func checkPluginAvailability(ctx context.Context, reference protocol.PluginReference) error {
	if check, ok := ctx.Value(unavailablePluginKey{}).(func(protocol.PluginReference) error); ok {
		return check(reference)
	}
	return nil
}
