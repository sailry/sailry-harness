package link

import (
	"context"
	"errors"
	"sync"

	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

type Remote struct {
	link    *Link
	address EndpointAddress
}

func (remote *Remote) Target() protocol.NodeID { return remote.address.ID }

func (remote *Remote) Open(ctx context.Context, resource protocol.StreamID) (Stream, error) {
	connection, compressed, err := remote.link.connection(ctx, remote.address, true)
	if err != nil {
		return nil, err
	}
	scoped, cancel := remote.link.operation(ctx, true)
	stream, err := connection.OpenBi(scoped)
	cancel()
	if err != nil {
		return nil, linkNetwork(err)
	}
	accepted := false
	defer func() {
		if !accepted {
			_ = stream.Close()
		}
	}()
	if err := remote.link.writeCall(ctx, stream, wire.Call{Kind: wire.CallOpenStream, Stream: &resource}, compressed); err != nil {
		return nil, err
	}
	var reply wire.Reply
	if err := remote.link.readFrame(ctx, stream, &reply, compressed); err != nil {
		return nil, err
	}
	switch reply.Kind {
	case wire.ReplyStreamOpened:
		accepted = true
		return stream, nil
	case wire.ReplyRejected:
		return nil, reply.Rejected
	default:
		return nil, invalidLink("stream acknowledgement expected")
	}
}

func (remote *Remote) Dispatch(ctx context.Context, request protocol.Request) (Admission, error) {
	if request.Target != remote.Target() {
		return Admission{}, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "request belongs to another Node"}
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
	return remote.dispatchStream(ctx, request, stream, compressed)
}

func (remote *Remote) dispatchStream(ctx context.Context, request protocol.Request, stream contextStream, compressed bool) (Admission, error) {
	observing := false
	defer func() {
		if !observing {
			_ = stream.Close()
		}
	}()
	if err := remote.link.writeCall(ctx, stream, wire.Call{Kind: wire.CallCommand, Request: &request}, compressed); err != nil {
		var fault *protocol.Fault
		if errors.As(err, &fault) && fault.Code == protocol.ErrorCodeInvalidRequest {
			return Admission{}, err
		}
		return Admission{}, uncertainReceipt()
	}
	if err := stream.CloseWrite(); err != nil {
		return Admission{}, uncertainReceipt()
	}
	var reply wire.Reply
	if err := remote.link.readFrame(ctx, stream, &reply, compressed); err != nil {
		return Admission{}, uncertainReceipt()
	}
	if reply.Kind == wire.ReplyRejected && reply.Rejected != nil {
		return Admission{}, reply.Rejected
	}
	if reply.Kind != wire.ReplyReceipt || reply.Receipt == nil || reply.Receipt.ID != request.ID {
		return Admission{}, uncertainReceipt()
	}
	completed := make(chan Response, 1)
	if err := remote.link.spawn(func() {
		defer stream.Close()
		defer close(completed)
		reader := wire.NewReader(compressed)
		// A durable command outlives its caller. The Node runs any other
		// command under the caller's context, which reaches a remote Node
		// only as the stop this stream's close sends.
		owner, bound := remote.link.ctx, !request.Command.Durable()
		if bound {
			owner = ctx
		}
		scoped, cancel := remote.link.operation(owner, false)
		defer cancel()
		reply, err := reader.Reply(scopedReader{scoped, stream})
		if err != nil && bound && ctx.Err() != nil {
			completed <- Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "request was cancelled before its completion"}}
			return
		}
		if err != nil || reply.Kind != wire.ReplyCompleted || reply.Completed == nil {
			completed <- Response{Fault: unknownCompletion()}
			return
		}
		response := Response{Fault: reply.Completed.Fault}
		if reply.Completed.Output != nil {
			response.Output = *reply.Completed.Output
		}
		completed <- response
	}); err != nil {
		return Admission{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "request admitted but completion observation could not start"}
	}
	observing = true
	return Admission{Receipt: *reply.Receipt, Completion: completed}, nil
}

func (remote *Remote) Subscribe(ctx context.Context, topic protocol.Topic) (Subscription, error) {
	connection, compressed, err := remote.link.connection(ctx, remote.address, true)
	if err != nil {
		return nil, err
	}
	scoped, cancel := remote.link.operation(ctx, true)
	stream, err := connection.OpenBi(scoped)
	cancel()
	if err != nil {
		return nil, linkNetwork(err)
	}
	accepted := false
	defer func() {
		if !accepted {
			_ = stream.Close()
		}
	}()
	if err := remote.link.writeCall(ctx, stream, wire.Call{Kind: wire.CallSubscribe, Topic: &topic}, compressed); err != nil {
		return nil, err
	}
	if err := stream.CloseWrite(); err != nil {
		return nil, linkNetwork(err)
	}
	subscription := &remoteSubscription{link: remote.link, connection: connection, stream: stream, reader: wire.NewReader(compressed)}
	reply, err := subscription.reply(ctx)
	if err != nil {
		return nil, err
	}
	switch reply.Kind {
	case wire.ReplySubscribed:
		accepted = true
		return subscription, nil
	case wire.ReplyRejected:
		return nil, reply.Rejected
	default:
		return nil, invalidLink("subscription acceptance expected")
	}
}

type remoteSubscription struct {
	link       *Link
	connection *iroh.Connection
	stream     contextStream
	reader     *wire.Reader
	next       sync.Mutex
	closeOnce  sync.Once
	closeErr   error
}

func (subscription *remoteSubscription) Next(ctx context.Context) (protocol.Update, error) {
	subscription.next.Lock()
	defer subscription.next.Unlock()
	reply, err := subscription.reply(ctx)
	if err != nil {
		return protocol.Update{}, err
	}
	switch reply.Kind {
	case wire.ReplyUpdate:
		if reply.Update != nil {
			return *reply.Update, nil
		}
	case wire.ReplyRejected:
		return protocol.Update{}, reply.Rejected
	}
	return protocol.Update{}, invalidLink("subscription update expected")
}

func (subscription *remoteSubscription) reply(ctx context.Context) (wire.Reply, error) {
	scoped, cancel := subscription.link.operation(ctx, false)
	defer cancel()
	reply, err := subscription.reader.Reply(scopedReader{scoped, subscription.stream})
	if err != nil && subscription.connection != nil {
		infoContext, release := subscription.link.operation(subscription.link.ctx, true)
		info, infoErr := subscription.connection.Info(infoContext)
		release()
		if infoErr == nil && info.Closed != nil && info.Closed.Kind == "application" && info.Closed.Code == 1 {
			return wire.Reply{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "peer revoked access"}
		}
	}
	return reply, err
}

func (subscription *remoteSubscription) Close() error {
	subscription.closeOnce.Do(func() { subscription.closeErr = subscription.stream.Close() })
	return subscription.closeErr
}

func uncertainReceipt() error {
	return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "request may have been admitted; recover using the same request identifier"}
}

func unknownCompletion() *protocol.Fault {
	return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "completion unavailable; retry with the same request identifier"}
}

func faultValue(err error) protocol.Fault {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return *fault
	}
	var wrapped *FaultError
	if errors.As(err, &wrapped) {
		return wrapped.Fault
	}
	var value protocol.Fault
	if errors.As(err, &value) {
		return value
	}
	return protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Link connection is unavailable"}
}
