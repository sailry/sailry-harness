package client

import (
	"context"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// PrepareKnowledge pins the storage Node before an operation acquires its
// request identity. Callers retain this exact request for outcome recovery.
func (client *Client) PrepareKnowledge(ref protocol.KnowledgeRef, command protocol.Command) protocol.Request {
	target := ref.Node
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
