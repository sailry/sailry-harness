package client

import (
	"context"
	"encoding/hex"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func (client *Client) scopedKnowledge(ctx context.Context, call protocol.ScopedKnowledgeCall) (protocol.Output, error) {
	if err := call.Validate(); err != nil {
		return protocol.Output{}, err
	}
	original := call.Request
	if call.Original != nil {
		original = *call.Original
	}
	if err := protocol.ValidateKnowledgeExecution(original, client.Target()); err != nil {
		return protocol.Output{}, err
	}
	transport, ok := client.transport.(interface {
		ScopedKnowledge(context.Context, protocol.ScopedKnowledgeCall) (link.Admission, error)
	})
	if !ok {
		return protocol.Output{}, fault(protocol.ErrorCodeUnavailable, "scoped knowledge routing is unavailable")
	}
	accepted, err := transport.ScopedKnowledge(ctx, call)
	if err != nil {
		return protocol.Output{}, err
	}
	select {
	case response, open := <-accepted.Completion:
		if !open {
			return protocol.Output{}, unknownCompletion()
		}
		if response.Fault != nil {
			return protocol.Output{}, &link.FaultError{Fault: *response.Fault}
		}
		return response.Output, nil
	case <-ctx.Done():
		if !call.Request.Command.Durable() {
			return protocol.Output{}, fault(protocol.ErrorCodeCancelled, "request was cancelled before its completion")
		}
		return protocol.Output{}, unknownCompletion()
	}
}

// ExecuteScopedKnowledge sends the request to the Node that owns the plugin
// scope and the collection.
func (client *Client) ExecuteScopedKnowledge(ctx context.Context, request protocol.Request) (protocol.Output, error) {
	return client.scopedKnowledge(ctx, protocol.ScopedKnowledgeCall{Request: request})
}

func (client *Client) ScopedKnowledgeOutcome(ctx context.Context, request protocol.Request) (protocol.RequestOutcome, error) {
	body, err := json.Marshal(request)
	if err != nil || !request.Command.Durable() {
		return protocol.RequestOutcome{}, fault(protocol.ErrorCodeInvalidRequest, "invalid scoped knowledge outcome request")
	}
	digest := blake3.Sum256(body)
	inspection := protocol.NewRequest(request.Target, protocol.Command{Kind: "inspect_request", Data: protocol.CommandInspectRequestPayload{
		ID: request.ID, Digest: hex.EncodeToString(digest[:]),
	}})
	output, err := client.scopedKnowledge(ctx, protocol.ScopedKnowledgeCall{Request: inspection, Original: &request})
	if err != nil {
		return protocol.RequestOutcome{}, err
	}
	value, err := payload[protocol.OutputRequestOutcomePayload](output.Data)
	if err != nil || output.Kind != "request_outcome" || value.ID != request.ID {
		return protocol.RequestOutcome{}, fault(protocol.ErrorCodeInternal, "request outcome response does not match the query")
	}
	return value.Outcome, nil
}
