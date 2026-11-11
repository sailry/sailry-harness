package client

import (
	"context"
	"errors"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Settled is what became of a durable request whose completion was not observed.
type Settled uint8

const (
	// RequestPending: the Node is still running the request. Keep it and ask again.
	RequestPending Settled = iota
	// RequestApplied: the request succeeded. Output holds its stored result.
	RequestApplied
	// RequestFailed: the request failed. Fault holds its stored fault.
	RequestFailed
	// RequestAbsent: the request never reached the Node. Nothing changed.
	RequestAbsent
	// RequestUnknown: the Node cannot tell what the request did, and never will. It
	// stopped while the request ran, or the operation itself recorded that
	// its effect could not be confirmed. Fault says which.
	RequestUnknown
)

// Resolution answers "what became of this request".
type Resolution struct {
	State  Settled
	Output protocol.Output
	Fault  *protocol.Fault
}

// Resolve asks the Node what became of a durable request. It never runs the
// request. An error means the question itself went unanswered (the Node is
// unreachable, for one), so the request is as uncertain as before.
func (client *Client) Resolve(ctx context.Context, request protocol.Request) (Resolution, error) {
	outcome, err := client.Outcome(ctx, request)
	if err != nil {
		return Resolution{}, err
	}
	switch outcome.Kind {
	case "not_admitted":
		return Resolution{State: RequestAbsent}, nil
	case "admitted":
		return Resolution{State: RequestPending}, nil
	case "unknown":
		return Resolution{State: RequestUnknown, Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "the Node restarted while the request ran; its outcome is unknown"}}, nil
	case "completed":
		result, err := payload[protocol.Result[protocol.Output, protocol.Fault]](outcome.Data)
		switch {
		case err != nil || (result.Ok == nil) == (result.Err == nil):
			return Resolution{}, fault(protocol.ErrorCodeInternal, "request outcome response does not match the query")
		case result.Ok != nil:
			return Resolution{State: RequestApplied, Output: *result.Ok}, nil
		case result.Err.Code == protocol.ErrorCodeOutcomeUnknown:
			return Resolution{State: RequestUnknown, Fault: result.Err}, nil
		default:
			return Resolution{State: RequestFailed, Fault: result.Err}, nil
		}
	}
	return Resolution{}, fault(protocol.ErrorCodeInternal, "request outcome response does not match the query")
}

// Settle executes a durable request and, when its outcome comes back unknown
// (the reply was lost, or the Node answered a duplicate it will not run
// again), asks the Node what became of it. The request is never sent twice.
//
//   - It was applied: the stored output is returned as if it had been observed.
//   - It failed: the stored fault is returned.
//   - It never reached the Node, or the Node can never tell what it did: the
//     error satisfies Released. The caller drops the request, tells the user
//     once (Unavailable: nothing changed; OutcomeUnknown: check the target),
//     and builds a new request for the next attempt.
//   - The Node still runs it, or cannot be asked: the unknown outcome stands
//     (Retained). The caller keeps the request; Settle with the same request
//     later resolves it.
func (client *Client) Settle(ctx context.Context, request protocol.Request) (protocol.Output, error) {
	output, unknown := client.Execute(ctx, request)
	if unknown == nil || ctx.Err() != nil || !request.Command.Durable() || faultCode(unknown) != protocol.ErrorCodeOutcomeUnknown {
		return output, unknown
	}
	resolution, err := client.Resolve(ctx, request)
	if err != nil {
		return protocol.Output{}, unknown
	}
	switch resolution.State {
	case RequestApplied:
		return resolution.Output, nil
	case RequestFailed:
		return protocol.Output{}, &link.FaultError{Fault: *resolution.Fault}
	case RequestAbsent:
		return protocol.Output{}, &releasedError{FaultError: &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "the request did not reach the Node; nothing was changed"}}}
	case RequestUnknown:
		return protocol.Output{}, &releasedError{FaultError: &link.FaultError{Fault: *resolution.Fault}}
	default:
		return protocol.Output{}, unknown
	}
}

// releasedError marks a failure after which the request must not be sent again.
type releasedError struct{ *link.FaultError }

func (err *releasedError) Unwrap() error { return err.FaultError }

// Released reports whether err ends its request: the Node holds nothing more
// for that identifier, so sending it again can only repeat err. The next
// attempt must be a new request. Every error of Settle ends its request,
// except an OutcomeUnknown for which Released is false: that one is still
// running or the Node could not be asked, and the same request, settled again,
// resolves it.
func Released(err error) bool {
	var released *releasedError
	return errors.As(err, &released)
}

// Retained reports whether the request that failed with err must be kept and
// sent again unchanged to learn its outcome.
func Retained(err error) bool {
	return faultCode(err) == protocol.ErrorCodeOutcomeUnknown && !Released(err)
}
