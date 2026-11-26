package node

import (
	"context"
	"errors"
	"net/http"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/web"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) nativeSearch(ctx context.Context, request protocol.Request, binding model.Binding, turn *protocol.TurnID, input protocol.SearchRequest) (protocol.SearchPage, error) {
	cooldownKey := model.CredentialCooldownKey(binding)
	if err := node.cooldowns.Wait(ctx, nil, cooldownKey); err != nil {
		return protocol.SearchPage{}, searchModelFault(err)
	}
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
	page, err := web.SearchNative(ctx, options)
	if err != nil {
		if until, ok := web.RateLimitRetryAt(err); ok {
			node.cooldowns.Observe(cooldownKey, until, time.Now())
		}
		return protocol.SearchPage{}, err
	}
	return page, nil
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
