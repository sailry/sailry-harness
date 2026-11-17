package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Personal data shares the controller's single per-host reconciliation worker.
// Persisted executor groups and controller receipts survive every network hop.
func (node *Node) reconcilePersonalKV(ctx context.Context, remote link.Transport) error {
	// Settle an older source page before acknowledging newer executor writes.
	// Otherwise its first delivery could overwrite their acknowledged base.
	if err := node.deliverPersonalKV(ctx, remote, "kv_snapshot", nil); err != nil {
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeRevisionConflict {
			return err
		}
	}
	pendingErr := node.reconcilePersonalKVPending(ctx, remote)
	return errors.Join(pendingErr, node.publishPersonalKVSnapshot(ctx, remote))
}

func (node *Node) reconcilePersonalKVPending(ctx context.Context, remote link.Transport) error {
	if err := node.deliverPersonalKV(ctx, remote, "kv_ack", nil); err != nil {
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeRevisionConflict {
			return err
		}
	}
	var through uint64
	for {
		group, boundary, err := readPersonalKVGroup(ctx, remote)
		if err != nil {
			return err
		}
		if group == nil {
			return nil
		}
		if through == 0 {
			through = max(boundary, group.Sequence)
		}
		var receipt protocol.PersonalKVReceipt
		var changes []protocol.Event
		err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			var err error
			receipt, changes, err = store.ApplyPersonalKVGroup(ctx, db, node.ID(), remote.Target(), *group)
			if err != nil {
				return err
			}
			_, err = store.RecordEvents(ctx, db, node.ID(), changes)
			return err
		})
		if err != nil {
			return err
		}
		if len(changes) > 0 {
			node.pluginCommitted(changes, ctx, false)
			node.refreshKnowledgeStorage(protocol.Command{}, changes)
		}
		command := protocol.ConfigurationFollowCommand{Action: "kv_ack", KVReceipt: &receipt}
		if err := node.deliverPersonalKV(ctx, remote, "kv_ack", &command); err != nil {
			return err
		}
		// New local writes wait for the next pass so a busy executor still
		// receives source updates while its pending overlay stays visible.
		if receipt.Sequence >= through {
			return nil
		}
	}
}

func (node *Node) publishPersonalKVSnapshot(ctx context.Context, remote link.Transport) error {
	var generation *uint64
	var after *protocol.PersonalKVCursor
	for {
		var page protocol.PersonalKVSnapshot
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			page, err = store.ReadPersonalKVSnapshot(ctx, db, generation, after)
			return err
		}); err != nil {
			return err
		}
		command := protocol.ConfigurationFollowCommand{Action: "kv_snapshot", KVSnapshot: &page}
		if err := node.deliverPersonalKV(ctx, remote, "kv_snapshot", &command); err != nil {
			return err
		}
		if page.Next == nil {
			return nil
		}
		revision := page.Generation
		generation, after = &revision, page.Next
	}
}

func (node *Node) deliverPersonalKV(ctx context.Context, remote link.Transport, category string, command *protocol.ConfigurationFollowCommand) error {
	var request *protocol.Request
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		request, err = store.ConfigurationCommandDelivery(ctx, db, remote.Target(), category, command)
		return err
	}); err != nil || request == nil {
		return err
	}
	var deliveryErr error
	if category == "kv_ack" {
		deliveryErr = node.deliverConfigurationSnapshot(ctx, remote, *request)
		if deliveryErr != nil {
			// The root owns staged pages and the completion receipt. Keep it
			// even on a definite page error instead of replaying a whole ACK
			// under a newly allocated delivery identity.
			return deliveryErr
		}
	} else {
		_, deliveryErr = configurationRemote(ctx, remote, *request)
	}
	if deliveryErr != nil {
		var fault *protocol.Fault
		if !errors.As(deliveryErr, &fault) || fault.Code == protocol.ErrorCodeOutcomeUnknown || fault.Code == protocol.ErrorCodeUnavailable || fault.Code == protocol.ErrorCodeCancelled {
			return deliveryErr
		}
	}
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		return store.CompleteConfigurationFollowDelivery(ctx, db, remote.Target(), category, request.ID)
	}); err != nil {
		return err
	}
	return deliveryErr
}

func readPersonalKVGroup(ctx context.Context, remote link.Transport) (*protocol.PersonalKVGroup, uint64, error) {
	read := func(cursor *protocol.PersonalKVRead) (protocol.PersonalKVPending, error) {
		var pending protocol.PersonalKVPending
		output, err := configurationRemote(ctx, remote, protocol.NewRequest(remote.Target(), protocol.Command{
			Kind: "configuration_follow", Data: &protocol.ConfigurationFollowCommand{Action: "kv_pending", KVRead: cursor},
		}))
		if err == nil {
			err = decodePersonalKV(output.Data, &pending)
		}
		return pending, err
	}
	pending, err := read(nil)
	if err != nil || pending.Page == nil {
		return pending.Group, pending.Through, err
	}
	boundary := pending.Through
	header := *pending.Page
	if pending.Group != nil || header.Request == (protocol.RequestID{}) || header.Sequence == 0 || header.Bytes == 0 || header.Bytes > store.MaxPersonalKVTransferBytes {
		return nil, boundary, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "personal data transfer header is invalid"}
	}
	var body []byte
	for {
		page := pending.Page
		if page == nil || pending.Group != nil || page.Request != header.Request || page.Sequence != header.Sequence || page.Digest != header.Digest || page.Bytes != header.Bytes ||
			page.Offset != uint64(len(body)) || page.Offset >= page.Bytes || uint64(len(page.Data)) != min(uint64(protocol.ConfigurationPageBytes), page.Bytes-page.Offset) {
			return nil, boundary, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "personal data transfer page is invalid"}
		}
		body = append(body, page.Data...)
		if uint64(len(body)) == header.Bytes {
			var group protocol.PersonalKVGroup
			if err := json.Unmarshal(body, &group); err != nil {
				return nil, boundary, err
			}
			if group.Request != header.Request || group.Sequence != header.Sequence || group.Digest != header.Digest {
				return nil, boundary, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "personal data transfer differs from its group"}
			}
			return &group, boundary, nil
		}
		pending, err = read(&protocol.PersonalKVRead{Request: header.Request, Offset: uint64(len(body))})
		if err != nil {
			return nil, boundary, err
		}
	}
}

func decodePersonalKV(value any, target any) error {
	body, err := json.Marshal(value)
	if err != nil {
		return err
	}
	return json.Unmarshal(body, target)
}
