package client

import (
	"context"
	"encoding/hex"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// PrepareKnowledge pins the storage Node before an operation acquires its
// request identity. Callers retain this exact request for outcome recovery.
func (client *Client) PrepareKnowledge(ref protocol.KnowledgeRef, command protocol.Command) protocol.Request {
	target := ref.Node
	if target == (protocol.NodeID{}) {
		target = protocol.KnowledgeTarget(command)
	}
	if target == (protocol.NodeID{}) {
		target = client.Target()
	}
	return protocol.NewRequest(target, copyValue(command))
}

// ExecuteKnowledge uses the same entry identity as this Client while selecting
// the collection's real Node. It never rewrites an already prepared request.
func (client *Client) ExecuteKnowledge(ctx context.Context, request protocol.Request) (protocol.Output, error) {
	if request.Target == client.Target() {
		return client.Execute(ctx, request)
	}
	router, ok := client.transport.(interface {
		ForTarget(protocol.NodeID) link.Transport
	})
	if !ok {
		return protocol.Output{}, fault(protocol.ErrorCodeUnavailable, "entry does not expose a route to this knowledge Node")
	}
	selected := New(router.ForTarget(request.Target))
	defer selected.Close()
	return selected.Execute(ctx, request)
}

func (client *Client) Knowledge(ctx context.Context, ref protocol.KnowledgeRef, command protocol.Command) (protocol.Output, error) {
	return client.ExecuteKnowledge(ctx, client.PrepareKnowledge(ref, command))
}

// KnowledgeOutcome follows the original entry and target without retrying a
// mutation under a new identity.
func (client *Client) KnowledgeOutcome(ctx context.Context, request protocol.Request) (protocol.RequestOutcome, error) {
	if request.Target == client.Target() {
		return client.Outcome(ctx, request)
	}
	if local, ok := client.transport.(interface {
		LocalResource(context.Context, protocol.ResourceCall) (link.Admission, error)
	}); ok {
		if request.Version != protocol.VERSION || !request.Command.Durable() || !protocol.KnowledgeCommand(request.Command.Kind) {
			return protocol.RequestOutcome{}, fault(protocol.ErrorCodeInvalidRequest, "invalid knowledge outcome request")
		}
		body, err := json.Marshal(request)
		if err != nil {
			return protocol.RequestOutcome{}, fault(protocol.ErrorCodeInvalidRequest, "request could not be encoded")
		}
		digest := blake3.Sum256(body)
		inspection := protocol.NewRequest(request.Target, protocol.Command{Kind: "inspect_request", Data: protocol.CommandInspectRequestPayload{
			ID: request.ID, Digest: hex.EncodeToString(digest[:]),
		}})
		accepted, err := local.LocalResource(ctx, protocol.ResourceCall{Request: inspection, Original: &request})
		if err != nil {
			return protocol.RequestOutcome{}, err
		}
		select {
		case response, open := <-accepted.Completion:
			if !open {
				return protocol.RequestOutcome{}, unknownCompletion()
			}
			if response.Fault != nil {
				return protocol.RequestOutcome{}, response.Fault
			}
			result, err := payload[protocol.OutputRequestOutcomePayload](response.Output.Data)
			if response.Output.Kind != "request_outcome" || err != nil || result.ID != request.ID {
				return protocol.RequestOutcome{}, fault(protocol.ErrorCodeInternal, "request outcome response does not match the query")
			}
			return result.Outcome, nil
		case <-ctx.Done():
			return protocol.RequestOutcome{}, ctx.Err()
		}
	}
	router, ok := client.transport.(interface {
		ForTarget(protocol.NodeID) link.Transport
	})
	if !ok {
		return protocol.RequestOutcome{}, fault(protocol.ErrorCodeUnavailable, "entry does not expose a route to this knowledge Node")
	}
	selected := New(router.ForTarget(request.Target))
	defer selected.Close()
	return selected.Outcome(ctx, request)
}
