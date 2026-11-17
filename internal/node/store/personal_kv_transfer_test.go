package store

import (
	"bytes"
	"database/sql"
	"encoding/json"
	"fmt"
	"io"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

func personalKVBatch(t *testing.T, fixture personalKVFixture, value string, revision uint64) protocol.Request {
	t.Helper()
	operations := make([]protocol.PluginTransactionOperation, 64)
	for index := range operations {
		operations[index] = protocol.PluginTransactionOperation{Kind: "write", Data: protocol.PluginTransactionOperationWritePayload{
			Key: fmt.Sprintf("key-%02d", index), Value: json.RawMessage(value), ExpectedRevision: revision,
		}}
	}
	request := protocol.NewRequest(fixture.node, protocol.Command{Kind: "plugin_transaction", Data: &protocol.CommandPluginTransactionPayload{Operations: operations}})
	request.Plugin = &protocol.PluginContext{Package: fixture.reference}
	if err := wire.Write(io.Discard, wire.Envelope{Call: wire.Call{Kind: wire.CallCommand, Request: &request}}, true); err != nil {
		t.Fatalf("legal 64-operation input did not fit the Link frame: %v", err)
	}
	if err := fixture.store.Node.Write(fixture.ctx, func(db *sql.Conn) error {
		_, _, err := ExecutePluginStorageCommand(fixture.ctx, db, fixture.node, fixture.store.Profile, fixture.controller, request)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return request
}

func readPersonalKVTransfer(t *testing.T, fixture personalKVFixture) protocol.PersonalKVGroup {
	t.Helper()
	var cursor *protocol.PersonalKVRead
	var assembled []byte
	var request protocol.RequestID
	for {
		var pending protocol.PersonalKVPending
		if err := fixture.store.Node.Read(fixture.ctx, func(db *sql.Conn) error {
			var err error
			pending, err = ReadPersonalKVPending(fixture.ctx, db, fixture.node, fixture.controller, cursor)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		output := protocol.Output{Kind: "personal_kv_pending", Data: pending}
		if err := wire.Write(io.Discard, wire.Reply{Kind: wire.ReplyCompleted, Completed: &wire.Response{Output: &output}}, true); err != nil {
			t.Fatalf("pending page exceeded a real Link frame: %v", err)
		}
		if pending.Group != nil {
			return *pending.Group
		}
		page := pending.Page
		if page == nil || page.Offset != uint64(len(assembled)) || len(page.Data) > protocol.ConfigurationPageBytes {
			t.Fatalf("invalid outbox page: %+v", page)
		}
		if cursor == nil {
			request = page.Request
			if page.Bytes <= protocol.MAX_FRAME_BYTES || pending.Through != page.Sequence {
				t.Fatal("fixture did not exercise a group larger than one frame")
			}
		} else if page.Request != request {
			t.Fatal("pending continuation selected another transaction")
		}
		assembled = append(assembled, page.Data...)
		if uint64(len(assembled)) == page.Bytes {
			var group protocol.PersonalKVGroup
			if err := json.Unmarshal(assembled, &group); err != nil {
				t.Fatal(err)
			}
			if group.Request != request || group.Digest != page.Digest {
				t.Fatal("paging changed the original group identity")
			}
			return group
		}
		cursor = &protocol.PersonalKVRead{Request: request, Offset: uint64(len(assembled))}
	}
}

func acknowledgePersonalKVTransfer(t *testing.T, fixture *personalKVFixture, receipt protocol.PersonalKVReceipt, restart bool) {
	t.Helper()
	body, err := json.Marshal(receipt)
	if err != nil || len(body) <= protocol.MAX_FRAME_BYTES {
		t.Fatal("fixture did not exercise an acknowledgement larger than one frame")
	}
	pages := snapshotBytePages(t, protocol.ConfigurationManifest{
		Source: fixture.controller, Category: protocol.PersonalKVAcknowledgementCategory, Revision: receipt.Sequence,
	}, body)
	for index, page := range pages {
		request := protocol.Request{Version: protocol.VERSION, Target: fixture.node, ID: protocol.ConfigurationPageRequestID(page.Root, page.Index),
			Command: protocol.Command{Kind: "configuration_follow", Data: &protocol.ConfigurationFollowCommand{Action: "apply_page", Page: &page}}}
		if request.Command.Durable() {
			t.Fatal("acknowledgement page entered the business ledger")
		}
		if err := wire.Write(io.Discard, wire.Envelope{Call: wire.Call{Kind: wire.CallCommand, Request: &request}}, true); err != nil {
			t.Fatalf("acknowledgement page exceeded a real Link frame: %v", err)
		}
		progress, events, err := stageSnapshot(t, fixture.store, fixture.ctx, fixture.node, fixture.controller, page)
		if err != nil || progress.Complete != (index == len(pages)-1) || progress.NextIndex != uint64(index+1) {
			t.Fatalf("acknowledgement progress: %+v, %v", progress, err)
		}
		if !progress.Complete && len(events) != 0 {
			t.Fatal("partial acknowledgement published a personal-data change")
		}
		if index == 0 && restart {
			profile := fixture.store.Profile
			if err := fixture.store.Close(); err != nil {
				t.Fatal(err)
			}
			fixture.store, err = Open(fixture.ctx, profile, fixture.node)
			if err != nil {
				t.Fatal(err)
			}
			reopened := fixture.store
			t.Cleanup(func() { _ = reopened.Close() })
		}
		// A lost page reply, including the final reply, must be harmless.
		duplicate, changes, err := stageSnapshot(t, fixture.store, fixture.ctx, fixture.node, fixture.controller, page)
		if err != nil || duplicate != progress || len(changes) != 0 {
			t.Fatalf("page replay repeated an acknowledgement: %+v, %v", duplicate, err)
		}
		if !progress.Complete {
			if err := fixture.store.Node.Read(fixture.ctx, func(db *sql.Conn) error {
				user, err := CurrentUser(fixture.ctx, db)
				if err != nil {
					return err
				}
				var count int
				err = db.QueryRowContext(fixture.ctx, "SELECT count(*) FROM personal_kv_outbox WHERE user=?1 AND request=?2", user.ID.String(), receipt.Request.String()).Scan(&count)
				if err == nil && count != 1 {
					return fmt.Errorf("partial acknowledgement discarded the outbox")
				}
				return err
			}); err != nil {
				t.Fatal(err)
			}
		}
	}
	if err := fixture.store.Node.Read(fixture.ctx, func(db *sql.Conn) error {
		header := pages[0]
		header.Index, header.Data = 0, nil
		progress, err := ReadConfigurationSnapshot(fixture.ctx, db, fixture.node, fixture.controller, header)
		if err == nil && (!progress.Complete || progress.Root != pages[0].Root) {
			return fmt.Errorf("original delivery lost its completion receipt")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
}

func TestPersonalKVTransfersLargeGroupsAndConflictingReceipts(t *testing.T) {
	source := newPersonalKVFixture(t, protocol.NodeID{})
	replica := newPersonalKVFixture(t, source.node)
	large := `"` + strings.Repeat("x", 200*1024) + `"`
	request := personalKVBatch(t, replica, large, 0)
	group := readPersonalKVTransfer(t, replica)
	if group.Request != request.ID || len(group.Writes) != 64 {
		t.Fatal("paging replaced or split the original 64-operation write set")
	}
	apply := func(group protocol.PersonalKVGroup) protocol.PersonalKVReceipt {
		t.Helper()
		var receipt protocol.PersonalKVReceipt
		if err := source.store.Node.Write(source.ctx, func(db *sql.Conn) error {
			var err error
			receipt, _, err = ApplyPersonalKVGroup(source.ctx, db, source.node, replica.node, group)
			if err != nil {
				return err
			}
			duplicate, events, err := ApplyPersonalKVGroup(source.ctx, db, source.node, replica.node, group)
			if err == nil && (len(events) != 0 || !reflect.DeepEqual(duplicate, receipt)) {
				return fmt.Errorf("source replay repeated or changed its outcome")
			}
			return err
		}); err != nil {
			t.Fatal(err)
		}
		return receipt
	}
	receipt := apply(group)
	if !receipt.Applied {
		t.Fatal("large legal transaction was rejected")
	}
	acknowledgePersonalKVTransfer(t, &replica, receipt, true)
	if entry := replica.read(t, "key-00"); entry.Revision != 1 || !bytes.Equal(entry.Value, []byte(large)) {
		t.Fatal("successful acknowledgement changed the original local view")
	}
	personalKVBatch(t, replica, `"small overwrite"`, 1)
	personalKVBatch(t, source, large, 1)
	conflicting := readPersonalKVTransfer(t, replica)
	receipt = apply(conflicting)
	if receipt.Applied || len(receipt.Records) != 64 {
		t.Fatal("source CAS failed to reject the whole conflicting transaction")
	}
	acknowledgePersonalKVTransfer(t, &replica, receipt, false)
	if entry := replica.read(t, "key-00"); !bytes.Equal(entry.Value, []byte(large)) {
		t.Fatal("large conflict acknowledgement did not restore source content")
	}
	if entry := source.read(t, "key-00"); entry.Revision != 2 {
		t.Fatal("acknowledgement recovery repeated source effects")
	}
}
