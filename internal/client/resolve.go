package client

import (
	"context"
	"errors"
	"time"

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
	// RequestAbsent: the request was not admitted. It had no effect.
	RequestAbsent
	// RequestUnknown: the Node cannot confirm the effect by this request ID.
	// Fault explains why.
	RequestUnknown
	// RequestReplayable: the forwarding Node can still ask the target by replaying
	// the original request. Keep it unchanged. Fault explains the uncertainty.
	RequestReplayable
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
		// Older Nodes used not_admitted for every missing row, even after pruning.
		if created, known := request.ID.CreatedAt(); known && time.Since(created) < protocol.RequestRetention {
			return Resolution{State: RequestAbsent}, nil
		}
		fallthrough
	case "unrecorded":
		return Resolution{State: RequestUnknown, Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "the Node has no retained record for the request; its outcome could not be confirmed"}}, nil
	case "admitted":
		return Resolution{State: RequestPending}, nil
	case "unknown":
		return Resolution{State: RequestUnknown, Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "the Node restarted while the request ran; its outcome is unknown"}}, nil
	case "replayable":
		input, inputErr := payload[protocol.CommandCreateSessionAtPayload](request.Command.Data)
		failure, err := payload[protocol.Fault](outcome.Data)
		if request.Command.Kind == "create_session_at" && inputErr == nil && input.Target != request.Target && err == nil && failure.Code == protocol.ErrorCodeOutcomeUnknown {
			return Resolution{State: RequestReplayable, Fault: &failure}, nil
		}
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
//   - It was not admitted, or it cannot be confirmed by this request ID:
//     the error satisfies Released (Unavailable: nothing changed;
//     OutcomeUnknown: check the target before trying again).
//   - The Node still runs it, cannot be asked, or supports exact replay of an
//     unresolved forward: the caller keeps the original request (Retained).
func (client *Client) Settle(ctx context.Context, request protocol.Request) (protocol.Output, error) {
	output, unknown := client.Execute(ctx, request)
	if unknown == nil || ctx.Err() != nil || !request.Command.Durable() || faultCode(unknown) != protocol.ErrorCodeOutcomeUnknown {
		return output, unknown
	}
	resolution, err := client.Resolve(ctx, request)
	if err != nil || resolution.State == RequestPending {
		return protocol.Output{}, unknown
	}
	return resolution.Result()
}

// Result is the answer a resolution gives its request, as Settle returns it:
// the stored output, the stored fault, an error that satisfies Released when
// the request was not admitted (Unavailable) or its outcome cannot be
// confirmed (OutcomeUnknown). Pending requests and unresolved forwards return
// an OutcomeUnknown that satisfies Retained.
func (resolution Resolution) Result() (protocol.Output, error) {
	switch resolution.State {
	case RequestApplied:
		return resolution.Output, nil
	case RequestFailed:
		return protocol.Output{}, &link.FaultError{Fault: *resolution.Fault}
	case RequestAbsent:
		return protocol.Output{}, &releasedError{FaultError: &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "the request was not admitted; nothing was changed"}}}
	case RequestUnknown:
		return protocol.Output{}, &releasedError{FaultError: &link.FaultError{Fault: *resolution.Fault}}
	case RequestReplayable:
		return protocol.Output{}, &link.FaultError{Fault: *resolution.Fault}
	default:
		return protocol.Output{}, &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "the Node is still running the request"}}
	}
}

// releasedError marks a failure after which the request must not be sent again.
type releasedError struct{ *link.FaultError }

func (err *releasedError) Unwrap() error { return err.FaultError }

// Released reports whether err ends recovery by this request ID. It does not
// imply that the request had no effect. Do not replay a released request.
func Released(err error) bool {
	var released *releasedError
	return errors.As(err, &released)
}

// Retained reports whether the request that failed with err must be kept and
// sent again unchanged to learn its outcome.
func Retained(err error) bool {
	return faultCode(err) == protocol.ErrorCodeOutcomeUnknown && !Released(err)
}
