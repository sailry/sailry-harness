package link

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

func (local *Local) ScopedKnowledge(ctx context.Context, call protocol.ScopedKnowledgeCall) (Admission, error) {
	if err := call.Validate(); err != nil {
		return Admission{}, err
	}
	handler, ok := local.handler.(ScopedKnowledgeHandler)
	if !ok {
		return Admission{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "scoped knowledge routing is unavailable"}
	}
	return handler.ScopedKnowledge(ctx, local.caller, call)
}

func (remote *Remote) ScopedKnowledge(ctx context.Context, call protocol.ScopedKnowledgeCall) (Admission, error) {
	if err := call.Validate(); err != nil {
		return Admission{}, err
	}
	connection, compressed, err := remote.link.connection(ctx, remote.address, true)
	if err != nil {
		return Admission{}, err
	}
	scoped, cancel := remote.link.operation(ctx, true)
	stream, err := connection.OpenBi(scoped)
	cancel()
	if err != nil {
		return Admission{}, linkNetwork(err)
	}
	return remote.dispatchCall(ctx, call.Request, wire.Call{Kind: wire.CallScopedKnowledge, ScopedKnowledge: &call}, stream, compressed)
}
