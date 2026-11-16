package link

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ForTarget lets the local controller's resource router choose a directly
// connected storage Node without changing the prepared request or caller.
func (local *Local) ForTarget(target protocol.NodeID) Transport {
	if target == local.target {
		return local
	}
	return &localTarget{local: local, target: target}
}

// LocalResource restores a prepared resource mutation through the local
// controller entry. Its user and package are verified by that Node.
func (local *Local) LocalResource(ctx context.Context, call protocol.ResourceCall) (Admission, error) {
	handler, ok := local.handler.(ResourceHandler)
	if !ok {
		return Admission{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "local resource routing is unavailable"}
	}
	return handler.Resource(ctx, local.caller, call)
}

type localTarget struct {
	local  *Local
	target protocol.NodeID
}

func (target *localTarget) Target() protocol.NodeID { return target.target }
func (target *localTarget) Route() protocol.RouteKey {
	return protocol.RouteKey{Entry: target.local.target, Target: target.target}
}
func (target *localTarget) Dispatch(ctx context.Context, request protocol.Request) (Admission, error) {
	if request.Target != target.target {
		return Admission{}, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "request belongs to another Node"}
	}
	return target.local.Dispatch(ctx, request)
}
func (target *localTarget) WatchConnection(ctx context.Context, changed func(ConnectionInfo)) error {
	return target.local.WatchConnection(ctx, changed)
}
func (target *localTarget) Subscribe(context.Context, protocol.Topic) (Subscription, error) {
	return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "local resource route does not expose subscriptions"}
}
func (target *localTarget) Open(context.Context, protocol.StreamID) (Stream, error) {
	return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "local resource route does not expose streams"}
}
