package client

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ExecuteHostCall retains the entry's authenticated identity while selecting
// only the prepared handler's actual execution Node.
func (client *Client) ExecuteHostCall(ctx context.Context, request protocol.Request) (protocol.Output, error) {
	if request.Command.Kind != "call_host_handler" {
		return protocol.Output{}, fault(protocol.ErrorCodeInvalidRequest, "host handler request is required")
	}
	return client.ExecuteKnowledge(ctx, request)
}

// HostCallOutcome inspects the original durable handler request without
// preparing or replaying a second invocation.
func (client *Client) HostCallOutcome(ctx context.Context, request protocol.Request) (protocol.RequestOutcome, error) {
	if request.Command.Kind != "call_host_handler" {
		return protocol.RequestOutcome{}, fault(protocol.ErrorCodeInvalidRequest, "host handler request is required")
	}
	return client.resourceOutcome(ctx, request)
}
