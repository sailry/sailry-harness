package node

import (
	"context"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) dispatchPort(ctx context.Context, caller protocol.NodeID, request protocol.Request) (link.Admission, error) {
	if request.Plugin != nil {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "port forwarding requires a controller"}
	}
	if err := node.begin(); err != nil {
		return link.Admission{}, err
	}
	defer node.work.Done()
	if err := ctx.Err(); err != nil {
		return link.Admission{}, err
	}
	var output protocol.Output
	var err error
	switch input := request.Command.Data.(type) {
	case *protocol.CommandOpenPortPayload:
		var stream protocol.StreamID
		stream, err = node.ports.Prepare(caller, input.Port)
		output = protocol.Output{Kind: "port_stream", Data: protocol.OutputPortStreamPayload{Stream: stream}}
	case *protocol.CommandCancelPortPayload:
		err = node.ports.Cancel(caller, input.Stream)
		output = protocol.Output{Kind: "port_cancelled"}
	}
	if err != nil {
		return link.Admission{}, err
	}
	completion := make(chan link.Response, 1)
	completion <- link.Response{Output: output}
	close(completion)
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: false}, Completion: completion}, nil
}
