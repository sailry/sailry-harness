package link

import (
	"context"
	"errors"
	"sync"
	"sync/atomic"

	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

func (link *Link) drive() {
	defer link.workers.Done()
	for range maxConnections {
		link.workers.Add(1)
		go link.acceptWorker()
	}
}

func (link *Link) acceptWorker() {
	defer link.workers.Done()
	for {
		if link.ctx.Err() != nil {
			return
		}
		connection, err := link.endpoint.Accept(link.ctx)
		if err != nil {
			// A closed endpoint accepts nothing more, so its workers end.
			if link.ctx.Err() != nil || errors.Is(err, iroh.ErrClosed) {
				return
			}
			continue
		}
		scoped, cancel := link.operation(link.ctx, true)
		info, err := connection.Info(scoped)
		cancel()
		if err != nil {
			_ = connection.Close(0, "connection unavailable")
			continue
		}
		link.mu.Lock()
		link.incoming[connection] = info.RemoteID
		link.mu.Unlock()
		switch {
		case info.ALPN == wire.PairALPN:
			link.servePairing(connection, info.RemoteID)
		case info.ALPN == wire.NodeALPN && link.isTrusted(info.RemoteID):
			link.serveConnection(connection, info.RemoteID)
		default:
			_ = connection.Close(1, "peer not paired")
		}
		link.mu.Lock()
		delete(link.incoming, connection)
		link.mu.Unlock()
	}
}

func (link *Link) driveWork() {
	defer link.workers.Done()
	finished := make(chan struct{}, maxTasks)
	active := 0
	for {
		var available <-chan func()
		if active < maxTasks {
			available = link.work
		}
		select {
		case <-link.ctx.Done():
			link.workMu.Lock()
			link.workStopped = true
			for {
				select {
				case work := <-link.work:
					work()
				default:
					link.workMu.Unlock()
					return
				}
			}
		case <-finished:
			active--
		case work := <-available:
			active++
			link.workers.Add(1)
			go func() {
				defer link.workers.Done()
				defer func() { finished <- struct{}{} }()
				work()
			}()
		}
	}
}

func (link *Link) spawn(work func()) error {
	link.workMu.Lock()
	defer link.workMu.Unlock()
	if link.workStopped || link.ctx.Err() != nil {
		return linkNetwork(link.ctx.Err())
	}
	select {
	case link.work <- work:
		return nil
	default:
		return &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "Link task capacity exhausted"}
	}
}

func (link *Link) serveConnection(connection *iroh.Connection, peer protocol.NodeID) {
	ctx, cancel := context.WithCancel(link.ctx)
	defer cancel()
	streams := make(chan struct{}, maxStreams)
	subscriptions := make(chan struct{}, maxStreams-8)
	var compression atomic.Bool
	var pending sync.WaitGroup
	defer func() {
		if link.isTrusted(peer) {
			_ = connection.Close(0, "Link closed")
		} else {
			_ = connection.Close(1, "peer revoked")
		}
		cancel()
		pending.Wait()
	}()
	for {
		select {
		case <-ctx.Done():
			return
		case streams <- struct{}{}:
		}
		stream, err := connection.AcceptBi(ctx)
		if err != nil {
			<-streams
			return
		}
		pending.Add(1)
		go func() {
			defer pending.Done()
			defer func() { <-streams }()
			_ = link.serveRequest(ctx, peer, stream, subscriptions, &compression)
		}()
	}
}

func (link *Link) serveRequest(ctx context.Context, peer protocol.NodeID, stream contextStream, subscriptions chan struct{}, compression *atomic.Bool) error {
	defer stream.Close()
	compressed := compression.Load()
	var envelope wire.Envelope
	if err := link.readFrame(ctx, stream, &envelope, compressed); err != nil {
		return err
	}
	if !link.isTrusted(peer) {
		return link.reject(ctx, stream, protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "peer is no longer paired"})
	}
	if envelope.Call.Kind != wire.CallInspect {
		host := uint16(protocol.VERSION)
		if err := checkCompatibility(envelope.Protocol, envelope.SoftwareVersion, &host, &link.softwareVersion); err != nil {
			return link.reject(ctx, stream, faultValue(err))
		}
		if link.handler == nil {
			return link.reject(ctx, stream, protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "controller has no execution service"})
		}
	}
	switch envelope.Call.Kind {
	case wire.CallInspect:
		compression.Store(envelope.Zstd)
		endpoint := link.endpointReply()
		if err := link.writeFrame(ctx, stream, wire.Reply{Kind: wire.ReplyEndpoint, Endpoint: &endpoint}); err != nil {
			return err
		}
	case wire.CallOpenStream:
		if envelope.Call.Stream == nil {
			return invalidLink("stream resource expected")
		}
		scoped, cancel := link.operation(ctx, true)
		service, err := link.handler.Open(scoped, peer, *envelope.Call.Stream)
		cancel()
		if err != nil {
			return link.reject(ctx, stream, faultValue(err))
		}
		defer service.Close()
		if err := link.writeFrame(ctx, stream, wire.Reply{Kind: wire.ReplyStreamOpened}); err != nil {
			return err
		}
		return copyDuplex(ctx, stream, service)
	case wire.CallCommand:
		if envelope.Call.Request == nil {
			return invalidLink("command request expected")
		}
		request := *envelope.Call.Request
		dispatchContext := link.ctx
		release := func() {}
		if !request.Command.Durable() {
			dispatchContext, release = observeStream(ctx, stream)
		}
		defer release()
		admission, err := link.handler.Dispatch(dispatchContext, peer, request)
		cancelled := dispatchContext.Err() != nil
		if err != nil {
			if cancelled {
				return nil
			}
			return link.reject(ctx, stream, faultValue(err))
		}
		if err := link.writeFrame(ctx, stream, wire.Reply{Kind: wire.ReplyReceipt, Receipt: &admission.Receipt}); err != nil {
			return err
		}
		var response Response
		select {
		case <-ctx.Done():
			return linkNetwork(ctx.Err())
		case completed, open := <-admission.Completion:
			response = completed
			if !open {
				response.Fault = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Node completion is unavailable"}
			}
		}
		result := wire.Response{Output: &response.Output, Fault: response.Fault}
		if result.Fault != nil {
			result.Output = nil
		}
		if err := link.writeReply(ctx, stream, wire.Reply{Kind: wire.ReplyCompleted, Completed: &result}, compressed); err != nil {
			return err
		}
	case wire.CallSubscribe:
		if envelope.Call.Topic == nil {
			return invalidLink("subscription topic expected")
		}
		select {
		case subscriptions <- struct{}{}:
			defer func() { <-subscriptions }()
		default:
			return link.reject(ctx, stream, protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "subscription capacity exhausted"})
		}
		scoped, release := observeStream(ctx, stream)
		defer release()
		subscription, err := link.handler.Subscribe(scoped, peer, *envelope.Call.Topic)
		if err != nil {
			if scoped.Err() != nil {
				return nil
			}
			return link.reject(ctx, stream, faultValue(err))
		}
		defer subscription.Close()
		if err := link.writeFrame(ctx, stream, wire.Reply{Kind: wire.ReplySubscribed}); err != nil {
			return err
		}
		for {
			update, err := subscription.Next(scoped)
			if scoped.Err() != nil {
				return nil
			}
			if err != nil {
				fault := faultValue(err)
				if err := link.writeReply(ctx, stream, wire.Reply{Kind: wire.ReplyRejected, Rejected: &fault}, compressed); err != nil {
					return err
				}
				break
			}
			if !link.isTrusted(peer) {
				return nil
			}
			if err := link.writeReply(ctx, stream, wire.Reply{Kind: wire.ReplyUpdate, Update: &update}, compressed); err != nil {
				return err
			}
		}
	default:
		return invalidLink("Link call expected")
	}
	return linkNetwork(stream.CloseWrite())
}

func observeStream(parent context.Context, stream contextStream) (context.Context, func()) {
	ctx, cancel := context.WithCancel(parent)
	finished := make(chan struct{})
	go func() {
		defer close(finished)
		_, _ = stream.WaitStopped(ctx)
		if ctx.Err() == nil {
			cancel()
		}
	}()
	return ctx, func() {
		cancel()
		<-finished
	}
}

func (link *Link) reject(ctx context.Context, stream contextStream, fault protocol.Fault) error {
	if err := link.writeFrame(ctx, stream, wire.Reply{Kind: wire.ReplyRejected, Rejected: &fault}); err != nil {
		return err
	}
	return linkNetwork(stream.CloseWrite())
}
