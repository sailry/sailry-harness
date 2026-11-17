package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
	"lukechampine.com/blake3"
)

func TestPersonalKVAcknowledgementSenderResumesOriginalDelivery(t *testing.T) {
	node := nodeFixture(t)
	target := protocol.NodeID{81}
	receipt := protocol.PersonalKVReceipt{Executor: target, Request: protocol.NewRequestID(), Sequence: 1, Digest: strings.Repeat("d", 64), Records: []protocol.PersonalKVRecord{}}
	for index := range 64 {
		receipt.Records = append(receipt.Records, protocol.PersonalKVRecord{Plugin: "personal-fixture", Key: fmt.Sprintf("key-%02d", index), Revision: 1,
			ValueBytes: []byte(`"` + strings.Repeat("x", 200*1024) + `"`), Present: true})
	}
	body, err := json.Marshal(receipt)
	if err != nil || len(body) <= protocol.MAX_FRAME_BYTES {
		t.Fatal("fixture acknowledgement did not exceed one Link frame")
	}
	t.Run("read complete original group", func(t *testing.T) {
		group := protocol.PersonalKVGroup{Controller: node.ID(), Executor: target, Request: receipt.Request, Sequence: receipt.Sequence,
			Package: protocol.PluginReference{Name: "personal-fixture"}, Writes: []protocol.PersonalKVWrite{}}
		for _, record := range receipt.Records {
			group.Writes = append(group.Writes, protocol.PersonalKVWrite{Key: record.Key, ValueBytes: record.ValueBytes, Present: true})
		}
		encoded, _ := json.Marshal(group)
		digest := blake3.Sum256(encoded)
		group.Digest = hex.EncodeToString(digest[:])
		encoded, _ = json.Marshal(group)
		transport := &personalKVPageTransport{target: target, body: encoded, group: group}
		restored, through, err := readPersonalKVGroup(t.Context(), transport)
		if err != nil || restored == nil || !reflect.DeepEqual(*restored, group) || through != group.Sequence || transport.pages < 2 {
			t.Fatalf("paged group changed its identity or write set: through=%d pages=%d error=%v", through, transport.pages, err)
		}
	})
	for _, lost := range []uint64{1, uint64(len(body) / protocol.ConfigurationPageBytes)} {
		t.Run(fmt.Sprintf("lost-page-%d", lost), func(t *testing.T) {
			transport := &configurationSnapshotTransport{
				target: target, pages: map[uint64][]byte{}, requests: map[uint64]protocol.RequestID{}, lostAt: &lost,
			}
			command := protocol.ConfigurationFollowCommand{Action: "kv_ack", KVReceipt: &receipt}
			if err := node.deliverPersonalKV(t.Context(), transport, "kv_ack", &command); err == nil {
				t.Fatal("fixture did not lose an acknowledgement page reply")
			}
			var root protocol.RequestID
			if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
				var body []byte
				if err := db.QueryRowContext(t.Context(), "SELECT request FROM configuration_follow_deliveries WHERE host=?1 AND category='kv_ack'", target[:]).Scan(&body); err != nil {
					return err
				}
				var request protocol.Request
				if err := json.Unmarshal(body, &request); err != nil {
					return err
				}
				root = request.ID
				return nil
			}); err != nil {
				t.Fatal(err)
			}
			if root != transport.progress.Root {
				t.Fatal("page transfer replaced the original acknowledgement request")
			}
			if err := node.deliverPersonalKV(t.Context(), transport, "kv_ack", nil); err != nil {
				t.Fatal(err)
			}
			if !transport.progress.Complete || transport.progress.Root != root {
				t.Fatal("recovery lost the original delivery completion")
			}
			var assembled bytes.Buffer
			for index := uint64(0); index < transport.progress.NextIndex; index++ {
				if transport.requests[index] != protocol.ConfigurationPageRequestID(root, index) {
					t.Fatal("recovery used another page identity")
				}
				assembled.Write(transport.pages[index])
			}
			var restored protocol.PersonalKVReceipt
			if err := json.Unmarshal(assembled.Bytes(), &restored); err != nil || !reflect.DeepEqual(restored, receipt) {
				t.Fatalf("paging changed the original receipt: %v", err)
			}
		})
	}
}

type personalKVPageTransport struct {
	link.Transport
	target protocol.NodeID
	body   []byte
	group  protocol.PersonalKVGroup
	pages  int
}

func (transport *personalKVPageTransport) Target() protocol.NodeID { return transport.target }

func (transport *personalKVPageTransport) Dispatch(_ context.Context, request protocol.Request) (link.Admission, error) {
	input := request.Command.Data.(*protocol.ConfigurationFollowCommand)
	if input.Action != "kv_pending" {
		return link.Admission{}, errors.New("unexpected personal-data read")
	}
	var offset uint64
	if input.KVRead != nil {
		if input.KVRead.Request != transport.group.Request {
			return link.Admission{}, errors.New("continuation changed the group identity")
		}
		offset = input.KVRead.Offset
	}
	end := min(offset+protocol.ConfigurationPageBytes, uint64(len(transport.body)))
	pending := protocol.PersonalKVPending{Through: transport.group.Sequence, Page: &protocol.PersonalKVPage{
		Request: transport.group.Request, Sequence: transport.group.Sequence, Digest: transport.group.Digest,
		Bytes: uint64(len(transport.body)), Offset: offset, Data: transport.body[offset:end],
	}}
	output := protocol.Output{Kind: "personal_kv_pending", Data: pending}
	if err := wire.Write(io.Discard, wire.Reply{Kind: wire.ReplyCompleted, Completed: &wire.Response{Output: &output}}, true); err != nil {
		return link.Admission{}, err
	}
	transport.pages++
	completion := make(chan link.Response, 1)
	completion <- link.Response{Output: output}
	close(completion)
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
}

type personalKVOrderTransport struct {
	link.Transport
	target       protocol.NodeID
	old          protocol.RequestID
	group        *protocol.PersonalKVGroup
	base         string
	visible      string
	refuseOld    bool
	reads        int
	acknowledged int
	ack          configurationSnapshotTransport
}

func (transport *personalKVOrderTransport) Target() protocol.NodeID { return transport.target }

func (transport *personalKVOrderTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	input := request.Command.Data.(*protocol.ConfigurationFollowCommand)
	var output protocol.Output
	switch input.Action {
	case "kv_pending":
		transport.reads++
		pending := protocol.PersonalKVPending{Group: transport.group}
		if transport.group != nil {
			pending.Through = transport.group.Sequence
		}
		output = protocol.Output{Kind: "personal_kv_pending", Data: pending}
	case "kv_snapshot":
		if request.ID != transport.old || transport.refuseOld {
			return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture snapshot is not delivered"}
		}
		transport.base = ""
		if transport.group == nil {
			transport.visible = transport.base
		}
		output = protocol.Output{Kind: "configuration_follow", Data: protocol.ConfigurationFollowStatus{}}
	case "snapshot_state", "apply_page":
		accepted, err := transport.ack.Dispatch(ctx, request)
		if err == nil && transport.ack.progress.Complete && transport.acknowledged == 0 {
			var body bytes.Buffer
			for index := uint64(0); index < transport.ack.progress.NextIndex; index++ {
				body.Write(transport.ack.pages[index])
			}
			var receipt protocol.PersonalKVReceipt
			if err := json.Unmarshal(body.Bytes(), &receipt); err != nil {
				return link.Admission{}, err
			}
			transport.base = string(receipt.Records[0].ValueJSON())
			transport.visible, transport.group = transport.base, nil
			transport.acknowledged++
		}
		return accepted, err
	default:
		return link.Admission{}, errors.New("unexpected personal-data fixture action")
	}
	completion := make(chan link.Response, 1)
	completion <- link.Response{Output: output}
	close(completion)
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
}

func TestPersonalKVRecoversOlderSnapshotBeforeNewAcknowledgement(t *testing.T) {
	node := nodeFixture(t)
	target := protocol.NodeID{82}
	oldSnapshot := protocol.ConfigurationFollowCommand{Action: "kv_snapshot", KVSnapshot: &protocol.PersonalKVSnapshot{Records: []protocol.PersonalKVRecord{}}}
	var original *protocol.Request
	if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		var err error
		original, err = store.ConfigurationCommandDelivery(t.Context(), db, target, "kv_snapshot", &oldSnapshot)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	group := protocol.PersonalKVGroup{Controller: node.ID(), Executor: target, Request: protocol.NewRequestID(), Sequence: 1,
		Package: protocol.PluginReference{Name: "personal-fixture"}, Writes: []protocol.PersonalKVWrite{{Key: "note", Present: true, ValueBytes: []byte(`"new write"`)}}}
	body, err := json.Marshal(group)
	if err != nil {
		t.Fatal(err)
	}
	digest := blake3.Sum256(body)
	group.Digest = hex.EncodeToString(digest[:])
	transport := &personalKVOrderTransport{
		target: target, old: original.ID, group: &group, visible: `"new write"`, refuseOld: true,
		ack: configurationSnapshotTransport{target: target, pages: map[uint64][]byte{}, requests: map[uint64]protocol.RequestID{}},
	}
	if err := node.reconcilePersonalKV(t.Context(), transport); err == nil || transport.reads != 0 || transport.acknowledged != 0 {
		t.Fatal("an undelivered old snapshot allowed new acknowledgements")
	}
	transport.refuseOld = false
	// The later fresh snapshot remains offline. The settled acknowledgement
	// must remain visible even without that final convergence pass.
	if err := node.reconcilePersonalKV(t.Context(), transport); err == nil {
		t.Fatal("fixture did not interrupt the fresh snapshot")
	}
	if transport.acknowledged != 1 || transport.group != nil || transport.visible != `"new write"` {
		t.Fatalf("older snapshot rolled back the acknowledged write: %+v", transport)
	}
}
