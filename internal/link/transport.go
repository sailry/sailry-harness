package link

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Response is the final outcome of an admitted command.
type Response struct {
	Output protocol.Output
	Fault  *protocol.Fault
}

// Admission separates acknowledgment from work that outlives its observer.
type Admission struct {
	Receipt    protocol.Receipt
	Completion <-chan Response
}

// Subscription observes one resource owned by the selected Node. Next honors its
// context; Close releases the observation and is safe to call more than once.
type Subscription interface {
	Next(context.Context) (protocol.Update, error)
	Close() error
}

// Handler is implemented by the execution Node, which owns the request ledger.
type Handler interface {
	Dispatch(context.Context, protocol.NodeID, protocol.Request) (Admission, error)
	Subscribe(context.Context, protocol.NodeID, protocol.Topic) (Subscription, error)
}

// Transport binds a controller to one Node without owning business state.
type Transport interface {
	Target() protocol.NodeID
	Dispatch(context.Context, protocol.Request) (Admission, error)
	Subscribe(context.Context, protocol.Topic) (Subscription, error)
}

// FaultError carries a protocol fault through an ordinary Go error boundary.
type FaultError struct{ Fault protocol.Fault }

func (err *FaultError) Error() string { return err.Fault.Message }

func (err *FaultError) Unwrap() error { return err.Fault }
