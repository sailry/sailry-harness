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
	"lukechampine.com/blake3"
)

type configurationSnapshotTransport struct {
	link.Transport
	target       protocol.NodeID
	progress     protocol.ConfigurationSnapshotProgress
	pages        map[uint64][]byte
	requests     map[uint64]protocol.RequestID
	sizes        []int
	attempts     []uint64
	lostAt       *uint64
	disconnected bool
}

func (transport *configurationSnapshotTransport) Target() protocol.NodeID { return transport.target }

func (transport *configurationSnapshotTransport) Dispatch(_ context.Context, request protocol.Request) (link.Admission, error) {
	input := request.Command.Data.(*protocol.ConfigurationFollowCommand)
	if input.Page == nil {
		return link.Admission{}, errors.New("missing fixture page")
	}
	page := input.Page
	if transport.progress.Root != (protocol.RequestID{}) && transport.progress.Root != page.Root {
		return link.Admission{}, errors.New("delivery root changed")
	}
	transport.progress.Root = page.Root
	switch input.Action {
	case "snapshot_state":
	case "apply_page":
		encoded, err := json.Marshal(request)
		if err != nil {
			return link.Admission{}, err
		}
		transport.sizes = append(transport.sizes, len(encoded))
		if request.ID != protocol.ConfigurationPageRequestID(page.Root, page.Index) || request.Command.Durable() {
			return link.Admission{}, errors.New("unstable page request identity")
		}
		if previous, exists := transport.pages[page.Index]; exists && !bytes.Equal(previous, page.Data) {
			return link.Admission{}, errors.New("page content changed")
		}
		transport.pages[page.Index] = append([]byte(nil), page.Data...)
		transport.requests[page.Index] = request.ID
		transport.attempts = append(transport.attempts, page.Index)
		if page.Index != transport.progress.NextIndex {
			return link.Admission{}, errors.New("resumed at wrong page")
		}
		transport.progress.NextIndex++
		transport.progress.Complete = transport.progress.NextIndex*protocol.ConfigurationPageBytes >= page.Bytes
		if !transport.disconnected && transport.lostAt != nil && *transport.lostAt == page.Index {
			transport.disconnected = true
			return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "fixture reply was lost"}
		}
	default:
		return link.Admission{}, errors.New("unexpected fixture action")
	}
	completion := make(chan link.Response, 1)
	completion <- link.Response{Output: protocol.Output{Kind: "configuration_snapshot", Data: transport.progress}}
	close(completion)
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
}

func snapshotRequestFixture(t *testing.T, source, target protocol.NodeID, records int) protocol.Request {
	t.Helper()
	manifest := protocol.ConfigurationManifest{Source: source, Category: "providers", Revision: 1, Records: []protocol.ConfigurationFollowRecord{}}
	for index := range records {
		body, err := json.Marshal(map[string]string{"fixture": strings.Repeat("x", 960*1024)})
		if err != nil {
			t.Fatal(err)
		}
		manifest.Records = append(manifest.Records, protocol.ConfigurationFollowRecord{
			ID: fmt.Sprintf("record-%d", index), Revision: 1, Digest: strings.Repeat("0", 64), Body: body,
		})
	}
	return protocol.NewRequest(target, protocol.Command{
		Kind: "configuration_follow", Data: &protocol.ConfigurationFollowCommand{Action: "apply", Manifest: &manifest},
	})
}

func TestConfigurationSnapshotSenderBoundsFramesAndResumesExactDelivery(t *testing.T) {
	node := nodeFixture(t)
	target := protocol.NodeID{77}
	request := snapshotRequestFixture(t, node.ID(), target, 18)
	lostAt := uint64(1)
	transport := &configurationSnapshotTransport{
		target: target, pages: map[uint64][]byte{}, requests: map[uint64]protocol.RequestID{}, lostAt: &lostAt,
	}
	if err := node.deliverConfigurationSnapshot(t.Context(), transport, request); err == nil {
		t.Fatal("fixture did not interrupt the snapshot")
	}
	if len(transport.attempts) != 2 || transport.progress.NextIndex != 2 || transport.progress.Complete {
		t.Fatalf("interrupted sender progress: %+v", transport.progress)
	}
	firstIDs := map[uint64]protocol.RequestID{0: transport.requests[0], 1: transport.requests[1]}
	// Simulate rebuilding the frozen pending request after process restart.
	persisted, err := json.Marshal(request)
	if err != nil {
		t.Fatal(err)
	}
	var restored protocol.Request
	if err := json.Unmarshal(persisted, &restored); err != nil {
		t.Fatal(err)
	}
	if err := node.deliverConfigurationSnapshot(t.Context(), transport, restored); err != nil {
		t.Fatal(err)
	}
	if !transport.progress.Complete || transport.progress.Root != request.ID {
		t.Fatalf("resumed snapshot did not complete its original root: %+v", transport.progress)
	}
	if transport.requests[0] != firstIDs[0] || transport.requests[1] != firstIDs[1] || transport.attempts[2] != 2 {
		t.Fatal("resumption changed acknowledged request identities or replayed their pages")
	}
	for _, size := range transport.sizes {
		if size >= protocol.MAX_FRAME_BYTES {
			t.Fatalf("configuration page exceeded a frame: %d", size)
		}
	}
	var body []byte
	for index := uint64(0); index < transport.progress.NextIndex; index++ {
		body = append(body, transport.pages[index]...)
	}
	if len(body) <= protocol.MAX_FRAME_BYTES {
		t.Fatal("fixture did not exceed a single frame")
	}
	var actual protocol.ConfigurationManifest
	if err := json.Unmarshal(body, &actual); err != nil {
		t.Fatal(err)
	}
	expected := request.Command.Data.(*protocol.ConfigurationFollowCommand).Manifest
	if !reflect.DeepEqual(actual, *expected) {
		t.Fatal("snapshot changed during paging")
	}
	before := len(transport.attempts)
	if err := node.deliverConfigurationSnapshot(t.Context(), transport, restored); err != nil || len(transport.attempts) != before {
		t.Fatalf("completed receipt replayed page effects: %v", err)
	}
}

func TestConfigurationSnapshotSenderRecoversLostFinalReceipt(t *testing.T) {
	node := nodeFixture(t)
	target := protocol.NodeID{78}
	request := snapshotRequestFixture(t, node.ID(), target, 0)
	lostAt := uint64(0)
	transport := &configurationSnapshotTransport{
		target: target, pages: map[uint64][]byte{}, requests: map[uint64]protocol.RequestID{}, lostAt: &lostAt,
	}
	if err := node.deliverConfigurationSnapshot(t.Context(), transport, request); err == nil || !transport.progress.Complete {
		t.Fatalf("fixture did not lose the final receipt: %+v, %v", transport.progress, err)
	}
	if err := node.deliverConfigurationSnapshot(t.Context(), transport, request); err != nil || len(transport.attempts) != 1 {
		t.Fatalf("lost final response repeated an apply: attempts=%v error=%v", transport.attempts, err)
	}
}

func TestConfigurationSnapshotWriterStreamsExactJSON(t *testing.T) {
	request := snapshotRequestFixture(t, protocol.NodeID{71}, protocol.NodeID{72}, 2)
	manifest := *request.Command.Data.(*protocol.ConfigurationFollowCommand).Manifest
	var data bytes.Buffer
	if err := writeConfigurationSnapshot(&data, manifest); err != nil {
		t.Fatal(err)
	}
	var decoded protocol.ConfigurationManifest
	if err := json.Unmarshal(data.Bytes(), &decoded); err != nil || !reflect.DeepEqual(decoded, manifest) {
		t.Fatalf("streamed JSON changed content: %v", err)
	}
	if err := writeConfigurationSnapshot(snapshotFailedWriter{}, manifest); err != io.ErrClosedPipe {
		t.Fatalf("writer failure was lost: %v", err)
	}
}

type snapshotFailedWriter struct{}

func (snapshotFailedWriter) Write([]byte) (int, error) { return 0, io.ErrClosedPipe }

func TestConfigurationSnapshotPageDispatchRecordsEventsOnce(t *testing.T) {
	node := nodeFixture(t)
	source := protocol.NodeID{76}
	var user protocol.User
	if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		if err := store.PairLinkPeer(t.Context(), db, source, link.DirectionController); err != nil {
			return err
		}
		var err error
		user, err = store.CallerUser(t.Context(), db, node.ID(), source)
		if err != nil {
			return err
		}
		_, err = db.ExecContext(t.Context(), "UPDATE users SET controller=?2 WHERE id=?1", user.ID.String(), source[:])
		return err
	}); err != nil {
		t.Fatal(err)
	}
	manifest := protocol.ConfigurationManifest{Source: source, Category: "providers", Revision: 1, Records: []protocol.ConfigurationFollowRecord{}}
	var body bytes.Buffer
	if err := writeConfigurationSnapshot(&body, manifest); err != nil {
		t.Fatal(err)
	}
	sum := blake3.Sum256(body.Bytes())
	page := protocol.ConfigurationSnapshotPage{
		Root: protocol.NewRequestID(), Source: source, Category: "providers", Revision: 1,
		Bytes: uint64(body.Len()), Digest: hex.EncodeToString(sum[:]), Data: body.Bytes(),
	}
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "configuration_follow", Data: &protocol.ConfigurationFollowCommand{Action: "apply_page", Page: &page}})
	request.ID = protocol.ConfigurationPageRequestID(page.Root, 0)
	ctx := store.WithUser(t.Context(), user)
	countEvents := func() int {
		var count int
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			return db.QueryRowContext(ctx, "SELECT count(*) FROM events WHERE json_extract(body,'$.kind')='configuration_follow_changed'").Scan(&count)
		}); err != nil {
			t.Fatal(err)
		}
		return count
	}
	before := countEvents()
	result := dispatchFixture(t, node, source, request)
	if result.Fault != nil {
		t.Fatal(result.Fault)
	}
	var progress protocol.ConfigurationSnapshotProgress
	if err := decodeConfigurationOutput(result.Output, &progress); err != nil || !progress.Complete {
		t.Fatalf("page dispatch did not publish its final receipt: %+v %v", progress, err)
	}
	after := countEvents()
	if after != before+1 {
		t.Fatalf("empty snapshot published %d events", after-before)
	}
	result = dispatchFixture(t, node, source, request)
	if result.Fault != nil || countEvents() != after {
		t.Fatalf("repeated final request republished events: %+v", result)
	}
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var admitted int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM requests WHERE id=?1", request.ID.String()).Scan(&admitted); err != nil {
			return err
		}
		if admitted != 0 {
			return errors.New("cache page entered the permanent business-effect ledger")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
