package node

import (
	"context"
	"errors"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/localmodels/decision"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// decisionCapability is the decision runtime of this build. It runs on the
// pinned llama.cpp runtime packaged beside the executable and is unavailable
// without it; there is no fallback runtime or model.
func decisionCapability() localmodels.Capability {
	return localmodels.Capability{
		Load: func(resource localmodels.Resource, validating bool) (localmodels.Engine, error) {
			engine, err := decision.Load(resource.Definition, resource.Directory, validating)
			if err != nil {
				return localmodels.Engine{}, decisionFault(err)
			}
			return localmodels.Engine{Value: engine, Release: engine.Close}, nil
		},
		Available: decision.Available,
	}
}

// decisionFault reports a decision failure under its protocol code.
func decisionFault(err error) error {
	var failure *decision.Error
	if !errors.As(err, &failure) {
		return err
	}
	switch failure.Kind {
	case decision.Invalid:
		return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: failure.Message}
	case decision.Unavailable:
		return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: failure.Message}
	case decision.Cancelled:
		return &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: failure.Error()}
	}
	return &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: failure.Error()}
}

// decide answers typed questions with the requested model, or with the
// model the saved settings select, ahead of background inference.
func (node *Node) decide(ctx context.Context, request *protocol.DecisionRequest) (protocol.Output, error) {
	registry := node.localModels.registry
	if request.Model != nil && !localmodels.ValidID(*request.Model) {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid decision request"}
	}
	if err := decision.ValidateRequest(request); err != nil {
		return protocol.Output{}, err
	}
	if err := registry.Capabilities().Ensure(protocol.ModelsKindDecision); err != nil {
		return protocol.Output{}, err
	}
	settings := registry.Settings()
	choice := localmodels.Choice(&settings, protocol.ModelsKindDecision)
	if !choice.Enabled {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "local decision service is disabled"}
	}
	id := request.Model
	if id == nil {
		id = choice.Model
	}
	if id == nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "a decision model must be selected"}
	}
	resource, err := registry.Resource(protocol.ModelsKindDecision, *id, &choice)
	if err != nil {
		return protocol.Output{}, err
	}
	answered, err := localmodels.Execute(ctx, registry, resource, localmodels.Interactive, func(ctx context.Context, engine any) (*protocol.DecisionResponse, error) {
		loaded, matches := engine.(*decision.Engine)
		if !matches {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "decision engine required"}
		}
		answered, err := loaded.Decide(ctx, request)
		if err != nil {
			return nil, decisionFault(err)
		}
		return answered, nil
	})
	if err != nil {
		return protocol.Output{}, err
	}
	return protocol.Output{Kind: "decision", Data: *answered}, nil
}

// dispatchDecision runs one decision that stops with its caller and with
// the Node. Decisions are not durable.
func (node *Node) dispatchDecision(ctx context.Context, request protocol.Request) (link.Admission, error) {
	if err := node.begin(); err != nil {
		return link.Admission{}, err
	}
	completion := make(chan link.Response, 1)
	go func() {
		defer node.work.Done()
		defer close(completion)
		operation, cancel := context.WithCancel(ctx)
		defer cancel()
		defer context.AfterFunc(node.ctx, cancel)()
		completion <- response(node.decide(operation, request.Command.Data.(*protocol.DecisionRequest)))
	}()
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
}
