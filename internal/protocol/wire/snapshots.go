package wire

import (
	"encoding/json"
	"math"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const PageBytes = protocol.MAX_FRAME_BYTES / 16

// MaxSnapshotBytes bounds what a receiver buffers for one paged snapshot: 256
// pages, twice the protocol's largest single payload (MAX_CHANGE_BYTES).
const MaxSnapshotBytes = 16 * protocol.MAX_FRAME_BYTES

type Delivery string

const (
	DeliveryCompleted Delivery = "completed"
	DeliveryUpdate    Delivery = "update"
)

func (delivery *Delivery) UnmarshalJSON(data []byte) error {
	var value string
	if err := json.Unmarshal(data, &value); err != nil {
		return err
	}
	if value != string(DeliveryCompleted) && value != string(DeliveryUpdate) {
		return invalid()
	}
	*delivery = Delivery(value)
	return nil
}

type SnapshotHeader struct {
	Delivery Delivery        `json:"delivery"`
	Node     protocol.NodeID `json:"node"`
	Cursor   uint64          `json:"cursor"`
	Bytes    uint64          `json:"bytes"`
}

func (header *SnapshotHeader) UnmarshalJSON(data []byte) error {
	type headerData SnapshotHeader
	var decoded headerData
	if err := protocol.DecodeObject(data, &decoded, []string{"delivery", "node", "cursor", "bytes"}, false); err != nil {
		return err
	}
	*header = SnapshotHeader(decoded)
	return nil
}

type SnapshotPage struct {
	Index uint64 `json:"index"`
	Data  string `json:"data"`
}

func (page *SnapshotPage) UnmarshalJSON(data []byte) error {
	type pageData SnapshotPage
	var decoded pageData
	if err := protocol.DecodeObject(data, &decoded, []string{"index", "data"}, false); err != nil {
		return err
	}
	*page = SnapshotPage(decoded)
	return nil
}

// WriteReply passes each frame of a reply to write: the reply itself, or the
// header and bounded pages of a snapshot. The caller bounds each frame, so a
// complete snapshot may take longer than one frame deadline.
func WriteReply(reply Reply, write func(Reply) error) error {
	snapshot, delivery := replySnapshot(reply)
	if snapshot == nil {
		return write(reply)
	}
	data, err := json.Marshal(snapshot)
	if err != nil || len(data) > MaxSnapshotBytes {
		return invalid()
	}
	defer clear(data)
	header := SnapshotHeader{delivery, snapshot.Node, snapshot.Cursor, uint64(len(data))}
	if err := write(Reply{Kind: ReplySnapshotStart, SnapshotStart: &header}); err != nil {
		return err
	}
	var index uint64
	for start := 0; start < len(data); index++ {
		end := min(start+PageBytes, len(data))
		for end < len(data) && !utf8.RuneStart(data[end]) {
			end--
		}
		page := SnapshotPage{index, string(data[start:end])}
		if err := write(Reply{Kind: ReplySnapshotPage, SnapshotPage: &page}); err != nil {
			return err
		}
		start = end
	}
	return nil
}

func replySnapshot(reply Reply) (*protocol.Snapshot, Delivery) {
	if reply.Kind == ReplyCompleted && reply.Completed != nil && reply.Completed.Output != nil && reply.Completed.Output.Kind == "snapshot" {
		return snapshotValue(reply.Completed.Output.Data), DeliveryCompleted
	}
	if reply.Kind == ReplyUpdate && reply.Update != nil && reply.Update.Kind == "snapshot" {
		return snapshotValue(reply.Update.Data), DeliveryUpdate
	}
	return nil, ""
}

func snapshotValue(value any) *protocol.Snapshot {
	switch snapshot := value.(type) {
	case *protocol.Snapshot:
		return snapshot
	case protocol.Snapshot:
		return &snapshot
	default:
		return nil
	}
}

type Assembly struct {
	pending *pendingSnapshot
}

type pendingSnapshot struct {
	header SnapshotHeader
	data   []byte
	index  uint64
}

func (assembly *Assembly) Accept(reply Reply) (*Reply, error) {
	switch reply.Kind {
	case ReplySnapshotStart:
		if assembly.pending != nil || reply.SnapshotStart == nil || reply.SnapshotStart.Bytes == 0 || reply.SnapshotStart.Bytes > MaxSnapshotBytes {
			return nil, invalid()
		}
		assembly.pending = &pendingSnapshot{header: *reply.SnapshotStart}
		return nil, nil
	case ReplySnapshotPage:
		pending, page := assembly.pending, reply.SnapshotPage
		if pending == nil || page == nil || len(page.Data) > math.MaxInt-len(pending.data) {
			return nil, invalid()
		}
		length := len(pending.data) + len(page.Data)
		if page.Index != pending.index || len(page.Data) == 0 || len(page.Data) > PageBytes || uint64(length) > pending.header.Bytes {
			return nil, invalid()
		}
		pending.data = append(pending.data, page.Data...)
		if pending.index == math.MaxUint64 {
			return nil, invalid()
		}
		pending.index++
		if uint64(length) != pending.header.Bytes {
			return nil, nil
		}
		assembly.pending = nil
		defer clear(pending.data)
		var snapshot protocol.Snapshot
		if err := protocol.DecodeAs(protocol.PeerData, pending.data, &snapshot); err != nil {
			return nil, invalid()
		}
		if snapshot.Node != pending.header.Node || snapshot.Cursor != pending.header.Cursor {
			return nil, invalid()
		}
		if pending.header.Delivery == DeliveryCompleted {
			output := protocol.Output{Kind: "snapshot", Data: &snapshot}
			return &Reply{Kind: ReplyCompleted, Completed: &Response{Output: &output}}, nil
		}
		update := protocol.Update{Kind: "snapshot", Data: &snapshot}
		return &Reply{Kind: ReplyUpdate, Update: &update}, nil
	default:
		if assembly.pending != nil {
			return nil, invalid()
		}
		return &reply, nil
	}
}
