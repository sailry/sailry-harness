package client

import (
	"context"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// RemoveHost makes one best-effort self-revocation at an outbound host before
// the caller removes local trust. The original request/outcome remains in the
// peer store. Remote unavailability never prevents the requested local removal.
// There is no automatic retry after removal; a later explicit pairing starts a
// new attempt while retaining the earlier diagnostic record.
func RemoveHost(ctx context.Context, endpoint *link.Link, address link.EndpointAddress, local func(context.Context) error) (link.HostRemovalAttempt, error) {
	var attempt link.HostRemovalAttempt
	if endpoint.Direction(address.ID).AllowsOutbound() {
		var err error
		attempt, err = endpoint.PrepareHostRemoval(ctx, address.ID)
		if err == nil {
			remote := New(endpoint.Remote(address))
			attempt = notifyHostRemoval(ctx, remote, attempt, func(state string, failure *protocol.Fault) error {
				return endpoint.RecordHostRemoval(context.WithoutCancel(ctx), address.ID, attempt.Request.ID, state, failure)
			})
			_ = remote.Close()
		} else {
			attempt.State = "failed"
			attempt.Fault = &protocol.Fault{Code: faultCode(err), Message: err.Error()}
		}
	}
	return attempt, local(context.WithoutCancel(ctx))
}

func notifyHostRemoval(ctx context.Context, remote *Client, attempt link.HostRemovalAttempt, record func(string, *protocol.Fault) error) link.HostRemovalAttempt {
	finish := func(state string, err error) link.HostRemovalAttempt {
		attempt.State, attempt.Fault = state, nil
		if err != nil {
			attempt.Fault = &protocol.Fault{Code: faultCode(err), Message: err.Error()}
		}
		if saved := record(state, attempt.Fault); saved != nil {
			attempt.State = "unknown"
			attempt.Fault = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "host removal outcome could not be retained"}
		}
		return attempt
	}
	if attempt.State == "completed" || attempt.State == "failed" {
		return attempt
	}
	if attempt.State == "unknown" {
		resolution, err := remote.Resolve(ctx, attempt.Request)
		if err != nil {
			return finish("unknown", err)
		}
		switch resolution.State {
		case RequestApplied:
			return finish("completed", nil)
		case RequestFailed:
			return finish("failed", resolution.Fault)
		case RequestPending, RequestUnknown:
			_, err := resolution.Result()
			return finish("unknown", err)
		case RequestAbsent: // Only this exact original request may be sent.
		}
	}
	if err := record("unknown", nil); err != nil {
		return finish("unknown", err)
	}
	_, err := remote.Settle(ctx, attempt.Request)
	if err == nil {
		return finish("completed", nil)
	}
	if faultCode(err) == protocol.ErrorCodeOutcomeUnknown || ctx.Err() != nil {
		return finish("unknown", err)
	}
	return finish("failed", err)
}
