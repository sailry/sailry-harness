package link

import (
	"context"
	"errors"
	"time"

	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

func (link *Link) slot(ctx context.Context, peer protocol.NodeID) (*outgoingSlot, error) {
	link.mu.Lock()
	if slot := link.outgoing[peer]; slot != nil {
		slot.users++
		link.mu.Unlock()
		return slot, nil
	}
	var candidates []*outgoingSlot
	if len(link.outgoing) >= maxConnections {
		for _, slot := range link.outgoing {
			if slot.users == 0 {
				slot.users++
				candidates = append(candidates, slot)
			}
		}
	}
	link.mu.Unlock()
	for _, slot := range candidates {
		slot.mu.Lock()
		connection := slot.connection
		slot.mu.Unlock()
		closed := connection == nil
		if connection != nil {
			scoped, cancel := link.operation(ctx, true)
			info, err := connection.Info(scoped)
			cancel()
			closed = err == nil && info.Closed != nil
		}
		evicted := false
		link.mu.Lock()
		slot.users--
		if closed && slot.users == 0 {
			for id, current := range link.outgoing {
				if current == slot {
					delete(link.outgoing, id)
					evicted = true
					break
				}
			}
		}
		link.mu.Unlock()
		if evicted {
			slot.cancel()
			slot.mu.Lock()
			connection, telemetry := slot.connection, slot.telemetry
			slot.connection, slot.telemetry = nil, nil
			slot.mu.Unlock()
			if telemetry != nil {
				telemetry.close()
				<-telemetry.done
			}
			if connection != nil {
				_ = connection.Close(0, "peer connection evicted")
			}
		}
	}
	link.mu.Lock()
	defer link.mu.Unlock()
	if slot := link.outgoing[peer]; slot != nil {
		slot.users++
		return slot, nil
	}
	if len(link.outgoing) >= maxConnections {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "Link connection capacity exhausted"}
	}
	lifetime, cancel := context.WithCancel(link.ctx)
	slot := &outgoingSlot{dial: make(chan struct{}, 1), ctx: lifetime, cancel: cancel, users: 1}
	slot.dial <- struct{}{}
	link.outgoing[peer] = slot
	return slot, nil
}

func (link *Link) releaseSlot(slot *outgoingSlot) {
	link.mu.Lock()
	slot.users--
	link.mu.Unlock()
}

func (link *Link) registered(peer protocol.NodeID, slot *outgoingSlot) error {
	if link.ctx.Err() != nil {
		return linkNetwork(link.ctx.Err())
	}
	if slot.ctx.Err() != nil {
		return &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "peer connection changed"}
	}
	link.mu.RLock()
	defer link.mu.RUnlock()
	if !link.trusted[peer] {
		return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "peer is not paired"}
	}
	if link.outgoing[peer] != slot {
		return &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "peer connection changed"}
	}
	return nil
}

func (link *Link) currentAddress(address EndpointAddress) EndpointAddress {
	link.mu.RLock()
	remembered := link.addresses[address.ID]
	link.mu.RUnlock()
	return mergeAddress(address, remembered)
}

func (link *Link) connection(ctx context.Context, address EndpointAddress, execution bool) (*iroh.Connection, bool, error) {
	if !link.isTrusted(address.ID) {
		return nil, false, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "peer is not paired"}
	}
	slot, err := link.slot(ctx, address.ID)
	if err != nil {
		return nil, false, err
	}
	defer link.releaseSlot(slot)
	select {
	case <-ctx.Done():
		return nil, false, linkNetwork(ctx.Err())
	case <-slot.ctx.Done():
		return nil, false, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "peer connection changed"}
	case <-slot.dial:
	}
	defer func() { slot.dial <- struct{}{} }()
	if err := link.registered(address.ID, slot); err != nil {
		return nil, false, err
	}
	slotContext, release := link.operation(ctx, false)
	stopSlot := context.AfterFunc(slot.ctx, release)
	defer func() { stopSlot(); release() }()
	target := link.currentAddress(address)
	slot.mu.Lock()
	cached, cachedAddress, cachedInfo := slot.connection, slot.address, slot.info
	slot.mu.Unlock()
	if cached != nil && addressIncludes(cachedAddress, target) {
		scoped, cancel := link.operation(slotContext, true)
		info, err := cached.Info(scoped)
		cancel()
		if err == nil && info.Closed == nil {
			if execution {
				if err := link.compatible(cachedInfo); err != nil {
					return nil, false, err
				}
			}
			return cached, cachedInfo.Zstd, nil
		}
	}
	scoped, cancel := link.operation(slotContext, true)
	connection, err := link.endpoint.Connect(scoped, target, wire.NodeALPN)
	cancel()
	if err != nil {
		return nil, false, linkNetwork(err)
	}
	accepted := false
	defer func() {
		if !accepted {
			_ = connection.Close(0, "peer connection rejected")
		}
	}()
	scoped, cancel = link.operation(slotContext, true)
	certified, err := connection.Info(scoped)
	cancel()
	if err != nil || certified.RemoteID != address.ID || certified.ALPN != wire.NodeALPN {
		return nil, false, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "peer identity could not be verified"}
	}
	info, err := link.inspectConnection(slotContext, connection)
	if err != nil {
		return nil, false, err
	}
	if execution {
		if err := link.compatible(info); err != nil {
			return nil, false, err
		}
	}
	link.trustUpdate.Lock()
	defer link.trustUpdate.Unlock()
	if err := link.registered(address.ID, slot); err != nil {
		return nil, false, err
	}
	current := link.currentAddress(address)
	if !addressIncludes(target, current) || !addressIncludes(current, target) {
		return nil, false, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "peer address changed"}
	}
	link.publishConnection(slot, target, connection, info, certified)
	accepted = true
	return connection, info.Zstd, nil
}

func (link *Link) UpdatePeer(ctx context.Context, address EndpointAddress) error {
	if !link.isTrusted(address.ID) {
		return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "peer is not paired"}
	}
	slot, err := link.slot(ctx, address.ID)
	if err != nil {
		return err
	}
	defer link.releaseSlot(slot)
	select {
	case <-ctx.Done():
		return linkNetwork(ctx.Err())
	case <-slot.ctx.Done():
		return &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "peer connection changed"}
	case <-slot.dial:
	}
	defer func() { slot.dial <- struct{}{} }()
	if err := link.registered(address.ID, slot); err != nil {
		return err
	}
	link.mu.RLock()
	previous, existed := link.addresses[address.ID]
	previous = cloneAddress(previous)
	link.mu.RUnlock()
	scoped, cancel := link.operation(ctx, false)
	stopSlot := context.AfterFunc(slot.ctx, cancel)
	defer func() { stopSlot(); cancel() }()
	dialing, expire := link.timeoutContext(scoped, ioTimeout)
	connection, err := link.endpoint.Connect(dialing, address, wire.NodeALPN)
	expire()
	if err != nil {
		return linkNetwork(err)
	}
	accepted := false
	defer func() {
		if !accepted {
			_ = connection.Close(0, "peer connection rejected")
		}
	}()
	checking, release := link.timeoutContext(scoped, ioTimeout)
	certified, err := connection.Info(checking)
	release()
	if err != nil || certified.RemoteID != address.ID || certified.ALPN != wire.NodeALPN {
		return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "peer identity could not be verified"}
	}
	info, err := link.inspectConnection(scoped, connection)
	if err != nil {
		return err
	}
	if err := link.compatible(info); err != nil {
		return err
	}
	link.trustUpdate.Lock()
	defer link.trustUpdate.Unlock()
	if err := link.registered(address.ID, slot); err != nil {
		return err
	}
	link.mu.RLock()
	current, present := link.addresses[address.ID]
	unchanged := existed == present && addressIncludes(previous, current) && addressIncludes(current, previous)
	link.mu.RUnlock()
	if !unchanged {
		return &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "peer address changed"}
	}
	if err := link.store.Remember(scoped, address); err != nil {
		if faultValue(err).Code == protocol.ErrorCodeOutcomeUnknown {
			return errors.Join(err, link.reconcilePeer(scoped, address.ID))
		}
		return err
	}
	link.mu.Lock()
	link.addresses[address.ID] = cloneAddress(address)
	link.mu.Unlock()
	link.publishConnection(slot, address, connection, info, certified)
	link.notifyPeers()
	accepted = true
	return nil
}

func (link *Link) publishConnection(slot *outgoingSlot, address EndpointAddress, connection *iroh.Connection, info PeerInfo, certified iroh.ConnectionInfo) {
	ticks := link.ticks
	if ticks == nil {
		ticks = connectionTicks
	}
	telemetry := newConnectionTelemetry(slot.ctx, selectedConnectionInfo(certified), connection.Info, connection.WaitPath, ticks)
	slot.mu.Lock()
	previous, previousTelemetry := slot.connection, slot.telemetry
	slot.connection, slot.address, slot.info, slot.telemetry = connection, cloneAddress(address), info, telemetry
	slot.mu.Unlock()
	if previousTelemetry != nil {
		previousTelemetry.close()
		<-previousTelemetry.done
	}
	if previous != nil && previous != connection {
		_ = previous.Close(0, "peer address changed")
	}
}

func (link *Link) compatible(info PeerInfo) error {
	local := uint16(protocol.VERSION)
	return checkCompatibility(&local, &link.softwareVersion, info.Protocol, info.SoftwareVersion)
}

func (link *Link) inspectConnection(ctx context.Context, connection *iroh.Connection) (PeerInfo, error) {
	started := time.Now()
	scoped, cancel := link.operation(ctx, true)
	stream, err := connection.OpenBi(scoped)
	cancel()
	if err != nil {
		return PeerInfo{}, linkNetwork(err)
	}
	defer stream.Close()
	if err := link.writeCall(ctx, stream, wire.Call{Kind: wire.CallInspect}, false); err != nil {
		return PeerInfo{}, err
	}
	if err := stream.CloseWrite(); err != nil {
		return PeerInfo{}, linkNetwork(err)
	}
	var reply wire.Reply
	if err := link.readFrame(ctx, stream, &reply, false); err != nil {
		return PeerInfo{}, err
	}
	if reply.Kind == wire.ReplyRejected {
		return PeerInfo{}, reply.Rejected
	}
	if reply.Kind != wire.ReplyEndpoint || reply.Endpoint == nil {
		return PeerInfo{}, invalidLink("endpoint inspection expected")
	}
	endpoint := reply.Endpoint
	return PeerInfo{
		Execution: endpoint.Execution, Name: endpoint.Name, Platform: endpoint.Platform,
		SoftwareVersion: endpoint.SoftwareVersion, Protocol: endpoint.Protocol, Zstd: endpoint.Zstd,
		Latency: time.Since(started),
	}, nil
}
