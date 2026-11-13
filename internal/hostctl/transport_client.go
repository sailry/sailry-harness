package hostctl

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"net"
	"sync"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Connect attaches a Client to the running Node's owner-only control socket.
// The control channel carries commands and subscriptions, not binary streams.
// Closing a controller never shuts down the Node or replays admitted work.
func Connect(ctx context.Context, profile string) (link.Transport, error) {
	return connectTransport(ctx, func(ctx context.Context) (net.Conn, error) {
		return connectControl(ctx, profile)
	})
}

type controlTransport struct {
	node protocol.NodeID
	dial func(context.Context) (net.Conn, error)
}

type incomingControl struct {
	Version uint8           `json:"version"`
	Node    protocol.NodeID `json:"node"`
	Body    struct {
		Kind string          `json:"kind"`
		Data json.RawMessage `json:"data"`
	} `json:"body"`
}

func connectTransport(ctx context.Context, dial func(context.Context) (net.Conn, error)) (*controlTransport, error) {
	connection, err := dial(ctx)
	if err != nil {
		return nil, err
	}
	defer connection.Close()
	stop := context.AfterFunc(ctx, func() { _ = connection.Close() })
	defer stop()
	if err := writeLine(connection, helloFrame{Command: "node_hello", Version: 1}, protocol.MAX_FRAME_BYTES); err != nil {
		return nil, err
	}
	hello, err := readControl(bufio.NewReader(connection), protocol.NodeID{})
	if err != nil {
		return nil, err
	}
	if hello.Body.Kind != "hello" {
		return nil, errors.New("Host did not confirm the control transport")
	}
	return &controlTransport{node: hello.Node, dial: dial}, nil
}

func readControl(reader *bufio.Reader, node protocol.NodeID) (incomingControl, error) {
	var frame incomingControl
	line, err := readLine(reader, protocol.MAX_FRAME_BYTES)
	if err != nil {
		return frame, err
	}
	if err := json.Unmarshal(line, &frame); err != nil {
		return frame, err
	}
	if frame.Version != 1 || frame.Node == (protocol.NodeID{}) || node != (protocol.NodeID{}) && frame.Node != node {
		return frame, errors.New("Host control Node or version mismatch")
	}
	if frame.Body.Kind == "failed" {
		var fault protocol.Fault
		if err := protocol.DecodeAs(protocol.PeerData, frame.Body.Data, &fault); err != nil {
			return frame, err
		}
		return frame, &link.FaultError{Fault: fault}
	}
	return frame, nil
}

func (transport *controlTransport) Target() protocol.NodeID { return transport.node }

func (*controlTransport) Open(context.Context, protocol.StreamID) (link.Stream, error) {
	return nil, &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "Host control does not carry binary streams"}}
}

func (*controlTransport) WatchConnection(ctx context.Context, changed func(link.ConnectionInfo)) error {
	route := link.RouteLocal
	changed(link.ConnectionInfo{Route: &route})
	<-ctx.Done()
	changed(link.ConnectionInfo{})
	return nil
}

func (transport *controlTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	if request.Target != transport.node {
		return link.Admission{}, &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "request belongs to another Node"}}
	}
	connection, err := transport.dial(ctx)
	if err != nil {
		return link.Admission{}, err
	}
	stop := context.AfterFunc(ctx, func() { _ = connection.Close() })
	accepted := false
	defer func() {
		if !accepted {
			stop()
			_ = connection.Close()
		}
	}()
	unknown := func() error {
		return &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Host completion is unconfirmed; retain the same request identifier"}}
	}
	if err := writeLine(connection, requestFrame{Command: "node_request", Version: 1, Request: request}, protocol.MAX_FRAME_BYTES); err != nil {
		return link.Admission{}, unknown()
	}
	reader := bufio.NewReader(connection)
	frame, err := readControl(reader, transport.node)
	if err != nil {
		var fault *link.FaultError
		if errors.As(err, &fault) {
			return link.Admission{}, err
		}
		return link.Admission{}, unknown()
	}
	var receipt protocol.Receipt
	if frame.Body.Kind != "admitted" || protocol.DecodeAs(protocol.PeerData, frame.Body.Data, &receipt) != nil || receipt.ID != request.ID || receipt.Durable != request.Command.Durable() {
		return link.Admission{}, unknown()
	}
	completed := make(chan link.Response, 1)
	accepted = true
	go func() {
		defer close(completed)
		defer connection.Close()
		defer stop()
		frame, err := readControl(reader, transport.node)
		var result protocol.Result[protocol.Output, protocol.Fault]
		if err != nil || frame.Body.Kind != "completed" || protocol.DecodeAs(protocol.PeerData, frame.Body.Data, &result) != nil || (result.Ok == nil) == (result.Err == nil) {
			completed <- link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Host completion is unconfirmed; retain the same request identifier"}}
			return
		}
		response := link.Response{Fault: result.Err}
		if result.Ok != nil {
			response.Output = *result.Ok
		}
		completed <- response
	}()
	return link.Admission{Receipt: receipt, Completion: completed}, nil
}

func (transport *controlTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	connection, err := transport.dial(ctx)
	if err != nil {
		return nil, err
	}
	stop := context.AfterFunc(ctx, func() { _ = connection.Close() })
	defer stop()
	subscription := &controlSubscription{connection: connection, reader: bufio.NewReader(connection), node: transport.node}
	if err := writeLine(connection, subscribeFrame{Command: "node_subscribe", Version: 1, Topic: topic}, protocol.MAX_FRAME_BYTES); err != nil {
		_ = subscription.Close()
		return nil, err
	}
	frame, err := readControl(subscription.reader, transport.node)
	if err == nil && frame.Body.Kind != "subscribed" {
		err = errors.New("Host did not confirm the subscription")
	}
	if err == nil {
		err = ctx.Err()
	}
	if err != nil {
		_ = subscription.Close()
		return nil, err
	}
	// Client cancels the establishment context after Subscribe returns. The
	// observation is subsequently owned by Next's context and Close.
	return subscription, nil
}

type controlSubscription struct {
	connection net.Conn
	reader     *bufio.Reader
	node       protocol.NodeID
	read       sync.Mutex
	close      sync.Once
	err        error
}

func (subscription *controlSubscription) Next(ctx context.Context) (protocol.Update, error) {
	subscription.read.Lock()
	defer subscription.read.Unlock()
	stop := context.AfterFunc(ctx, func() { _ = subscription.Close() })
	defer stop()
	frame, err := readControl(subscription.reader, subscription.node)
	if ctx.Err() != nil {
		return protocol.Update{}, ctx.Err()
	}
	var update protocol.Update
	if err == nil {
		if frame.Body.Kind != "update" {
			err = errors.New("unexpected Host subscription response")
		} else {
			err = protocol.DecodeAs(protocol.PeerData, frame.Body.Data, &update)
		}
	}
	if err != nil {
		_ = subscription.Close()
	}
	return update, err
}

func (subscription *controlSubscription) Close() error {
	subscription.close.Do(func() { subscription.err = subscription.connection.Close() })
	return subscription.err
}
