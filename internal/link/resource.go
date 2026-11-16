package link

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

// Via opens a target through a directly paired entry Node. It never pairs the
// target or grants it the entry's identity.
func (link *Link) Via(entry EndpointAddress, target protocol.NodeID) *Remote {
	remote := link.Remote(entry)
	if target != entry.ID {
		remote.target = &target
	}
	return remote
}

// Resource carries captured knowledge context between directly paired Nodes.
// It never enables ordinary reverse commands.
func (remote *Remote) Resource(ctx context.Context, call protocol.ResourceCall) (Admission, error) {
	if remote.target != nil {
		return Admission{}, invalidLink("resource operation is not supported")
	}
	if err := call.Validate(); err != nil {
		return Admission{}, err
	}
	if !remote.link.directHost(ctx, remote.address.ID) && !remote.link.directController(ctx, remote.address.ID) {
		return Admission{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "resource target is not a directly paired Node"}
	}
	connection, compressed, err := remote.link.connection(ctx, remote.address, false)
	if err != nil {
		return Admission{}, err
	}
	scoped, cancel := remote.link.operation(ctx, true)
	stream, err := connection.OpenBi(scoped)
	cancel()
	if err != nil {
		return Admission{}, linkNetwork(err)
	}
	return remote.dispatchCall(ctx, call.Request, wire.Call{Kind: wire.CallResource, Resource: &call}, stream, compressed)
}
