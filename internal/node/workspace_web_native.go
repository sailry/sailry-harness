package node

import (
	"context"
	"errors"
	"net/http"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/web"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) nativeSearch(ctx context.Context, request protocol.Request, binding model.Binding, turn *protocol.TurnID, input protocol.SearchRequest) (protocol.SearchPage, error) {
	options := web.NativeSearch{Provider: binding.Provider, Model: binding.Config.Model, Input: input, Session: binding.Session, Request: request.ID, Prepare: func(ctx context.Context, body map[string]any) (*http.Request, error) {
		value, err := model.PrepareNativeSearch(ctx, binding, body)
		return value, searchModelFault(err)
	}, Fetch: node.web.fetch.Fetch}
	if binding.Secret != nil {
		options.Secrets = []protocol.Secret{*binding.Secret}
	}
	if turn != nil {
		options.Record = func(ctx context.Context, usage canonical.Usage) error {
			return nodeFault(node.agent.RecordSearchUsage(ctx, binding.Session, *turn, binding.Provider, binding.Config.Model, usage))
		}
	}
	return web.SearchNative(ctx, options)
}

func searchModelFault(err error) error {
	if err == nil {
		return nil
	}
	var failure *model.Failure
	if errors.As(err, &failure) {
		return model.Fault(err)
	}
	return err
}
