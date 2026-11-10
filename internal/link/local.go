package link

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Local is constructed by the Node owner; caller identity never comes from wire data.
type Local struct {
	target  protocol.NodeID
	caller  protocol.NodeID
	handler Handler
}

func NewLocal(target, caller protocol.NodeID, handler Handler) *Local {
	return &Local{target: target, caller: caller, handler: handler}
}

func (local *Local) Target() protocol.NodeID { return local.target }

func (local *Local) WatchConnection(ctx context.Context, changed func(ConnectionInfo)) error {
	route := RouteLocal
	changed(ConnectionInfo{Route: &route})
	<-ctx.Done()
	changed(ConnectionInfo{})
	return nil
}

func (local *Local) Dispatch(ctx context.Context, request protocol.Request) (Admission, error) {
	return local.handler.Dispatch(ctx, local.caller, request)
}

func (local *Local) Subscribe(ctx context.Context, topic protocol.Topic) (Subscription, error) {
	return local.handler.Subscribe(ctx, local.caller, topic)
}

func (local *Local) Open(ctx context.Context, id protocol.StreamID) (Stream, error) {
	return local.handler.Open(ctx, local.caller, id)
}
