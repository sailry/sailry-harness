package hostctl

import (
	"context"
	"encoding/json"
	"io"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type controlReply struct {
	Version uint8           `json:"version"`
	Node    protocol.NodeID `json:"node"`
	Body    replyBody       `json:"body"`
}

type replyBody struct {
	Kind string `json:"kind"`
	Data any    `json:"data,omitempty"`
}

type helloFrame struct {
	Command string `json:"command"`
	Version uint8  `json:"version"`
}

type requestFrame struct {
	Command string           `json:"command"`
	Version uint8            `json:"version"`
	Request protocol.Request `json:"request"`
}

type subscribeFrame struct {
	Command string         `json:"command"`
	Version uint8          `json:"version"`
	Topic   protocol.Topic `json:"topic"`
}

func serveTransport(ctx context.Context, line []byte, writer io.Writer, transport link.Transport) error {
	respond := func(kind string, data any) error {
		return writeLine(writer, controlReply{Version: 1, Node: transport.Target(), Body: replyBody{kind, data}}, protocol.MAX_FRAME_BYTES)
	}
	fail := func(err error) error { return respond("failed", asFault(err)) }
	var command struct {
		Command string `json:"command"`
	}
	if json.Unmarshal(line, &command) != nil {
		return fail(&protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid Host transport frame"})
	}
	switch command.Command {
	case "node_hello":
		var frame helloFrame
		if err := protocol.DecodeObject(line, &frame, []string{"command", "version"}, true); err != nil || frame.Version != 1 {
			return fail(&protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "unsupported Host transport version"})
		}
		return respond("hello", nil)
	case "node_request":
		var frame requestFrame
		if err := protocol.DecodeObject(line, &frame, []string{"command", "version", "request"}, true); err != nil || frame.Version != 1 {
			return fail(&protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid Host transport request"})
		}
		admission, err := transport.Dispatch(ctx, frame.Request)
		if err != nil {
			return fail(err)
		}
		if err := respond("admitted", admission.Receipt); err != nil {
			return err
		}
		var result protocol.Result[protocol.Output, protocol.Fault]
		select {
		case <-ctx.Done():
			return ctx.Err()
		case completed, open := <-admission.Completion:
			if !open {
				result.Err = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Node completion is unconfirmed"}
			} else if completed.Fault != nil {
				result.Err = completed.Fault
			} else {
				result.Ok = &completed.Output
			}
		}
		return respond("completed", result)
	case "node_subscribe":
		var frame subscribeFrame
		if err := protocol.DecodeObject(line, &frame, []string{"command", "version", "topic"}, true); err != nil || frame.Version != 1 {
			return fail(&protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid Host transport subscription"})
		}
		subscription, err := transport.Subscribe(ctx, frame.Topic)
		if err != nil {
			return fail(err)
		}
		defer subscription.Close()
		if err := respond("subscribed", nil); err != nil {
			return err
		}
		for ctx.Err() == nil {
			update, err := subscription.Next(ctx)
			if err != nil {
				if ctx.Err() != nil {
					return ctx.Err()
				}
				return fail(err)
			}
			if err := respond("update", update); err != nil {
				return err
			}
		}
		return ctx.Err()
	}
	return fail(&protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "unsupported Host transport command"})
}
