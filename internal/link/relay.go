package link

import (
	"context"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

func (link *Link) identity() protocol.NodeID {
	return link.nodeID
}

func (link *Link) relay(ctx context.Context, peer protocol.NodeID, incoming contextStream, envelope wire.Envelope, compressed bool) error {
	target := *envelope.Target
	if !link.ownShell(ctx, peer) || !link.directHost(ctx, target) {
		return link.reject(ctx, incoming, protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "resource route is unavailable"})
	}
	switch envelope.Call.Kind {
	case wire.CallCommand, wire.CallSubscribe, wire.CallOpenStream:
	default:
		return link.reject(ctx, incoming, protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "unsupported routed operation"})
	}
	if envelope.Call.Kind == wire.CallCommand && !routedRequestTarget(envelope, target) {
		return link.reject(ctx, incoming, protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "request target does not match its route"})
	}
	link.mu.RLock()
	address, exists := link.addresses[target]
	link.mu.RUnlock()
	if !exists {
		return link.reject(ctx, incoming, protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "target address is unavailable"})
	}
	scoped, release := observeStream(ctx, incoming)
	defer release()
	connection, targetCompressed, err := link.connection(scoped, address, true)
	if err != nil {
		return link.reject(ctx, incoming, faultValue(err))
	}
	open, cancel := link.operation(scoped, true)
	outgoing, err := connection.OpenBi(open)
	cancel()
	if err != nil {
		return link.reject(ctx, incoming, faultValue(linkNetwork(err)))
	}
	defer outgoing.Close()
	forward := link.envelope(envelope.Call)
	forward.RawData = envelope.RawData
	// The target is now reached directly. A second relay hop is never encoded.
	writing, finishWrite := link.operation(scoped, true)
	err = wire.Write(scopedWriter{writing, outgoing}, forward, targetCompressed)
	finishWrite()
	if err != nil {
		return err
	}
	if envelope.Call.Kind != wire.CallOpenStream {
		if err := outgoing.CloseWrite(); err != nil {
			return linkNetwork(err)
		}
	}
	if envelope.Call.Kind == wire.CallOpenStream {
		frame, err := link.readRelayFrame(scoped, outgoing, wire.NewReader(targetCompressed))
		if err != nil {
			return err
		}
		defer clear(frame)
		var reply struct {
			Kind wire.ReplyKind `json:"kind"`
		}
		if json.Unmarshal(frame, &reply) != nil {
			return invalidLink("stream acknowledgement expected")
		}
		if err := link.writeRelayFrame(scoped, incoming, frame, false); err != nil {
			return err
		}
		if reply.Kind != wire.ReplyStreamOpened {
			return nil
		}
		return copyDuplex(scoped, incoming, outgoing)
	}
	return link.relayReplies(scoped, incoming, outgoing, compressed, targetCompressed)
}

func routedRequestTarget(envelope wire.Envelope, target protocol.NodeID) bool {
	if envelope.Call.Request != nil {
		return envelope.Call.Request.Target == target
	}
	if envelope.Undecoded == nil {
		return false
	}
	var request struct {
		Target protocol.NodeID `json:"target"`
	}
	return json.Unmarshal(envelope.RawData, &request) == nil && request.Target == target
}

// relayReplies handles individual transport frames, never assembling a remote
// snapshot. A missing completion remains uncertain at the original requester.
func (link *Link) relayReplies(ctx context.Context, incoming, outgoing contextStream, compressed, targetCompressed bool) error {
	reader := wire.NewReader(targetCompressed)
	var snapshot *wire.SnapshotHeader
	var remaining uint64
	var index uint64
	for {
		if err := ctx.Err(); err != nil {
			return linkNetwork(err)
		}
		frame, err := link.readRelayFrame(ctx, outgoing, reader)
		if err != nil {
			return err
		}
		var header struct {
			Kind wire.ReplyKind  `json:"kind"`
			Data json.RawMessage `json:"data"`
		}
		if err := json.Unmarshal(frame, &header); err != nil {
			clear(frame)
			return invalidLink("reply frame expected")
		}
		done := header.Kind == wire.ReplyCompleted || header.Kind == wire.ReplyRejected
		switch header.Kind {
		case wire.ReplySnapshotStart:
			var start wire.SnapshotHeader
			if err := json.Unmarshal(header.Data, &start); err != nil || snapshot != nil || start.Bytes == 0 || start.Bytes > wire.MaxSnapshotBytes {
				clear(frame)
				return invalidLink("invalid snapshot header")
			}
			snapshot, remaining, index = &start, start.Bytes, 0
		case wire.ReplySnapshotPage:
			var page wire.SnapshotPage
			if err := json.Unmarshal(header.Data, &page); err != nil || snapshot == nil ||
				page.Index != index || len(page.Data) == 0 || len(page.Data) > wire.PageBytes ||
				uint64(len(page.Data)) > remaining {
				clear(frame)
				return invalidLink("invalid snapshot page")
			}
			index++
			remaining -= uint64(len(page.Data))
			if remaining == 0 {
				done = snapshot.Delivery == wire.DeliveryCompleted
				snapshot = nil
			}
		default:
			if snapshot != nil {
				clear(frame)
				return invalidLink("snapshot page expected")
			}
		}
		first := header.Kind == wire.ReplyReceipt || header.Kind == wire.ReplySubscribed || header.Kind == wire.ReplyRejected
		if err := link.writeRelayFrame(ctx, incoming, frame, compressed && !first); err != nil {
			clear(frame)
			return err
		}
		clear(frame)
		if done {
			return linkNetwork(incoming.CloseWrite())
		}
	}
}

func (link *Link) readRelayFrame(ctx context.Context, stream contextStream, reader *wire.Reader) (json.RawMessage, error) {
	// A task completion or subscription may legitimately be idle. Match the
	// direct Remote reader: cancellation bounds observation, not an idle timer.
	scoped, cancel := link.operation(ctx, false)
	defer cancel()
	return reader.Raw(scopedReader{scoped, stream})
}

func (link *Link) writeRelayFrame(ctx context.Context, stream contextStream, frame json.RawMessage, compressed bool) error {
	scoped, cancel := link.operation(ctx, true)
	defer cancel()
	return wire.Write(scopedWriter{scoped, stream}, frame, compressed)
}
