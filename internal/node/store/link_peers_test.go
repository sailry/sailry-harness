package store

import (
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPeerStoreRoundTrip(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	address := link.EndpointAddress{ID: protocol.NodeID{7}, Addrs: []json.RawMessage{json.RawMessage(`{"Ip":"127.0.0.1:4321"}`)}}
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if err := RememberLinkPeer(ctx, db, address); err == nil {
			return errors.New("untrusted peer address was retained")
		}
		if err := SetLinkTrust(ctx, db, address.ID, true); err != nil {
			return err
		}
		if err := RememberLinkPeer(ctx, db, address); err != nil {
			return err
		}
		for index := 0; index < 127; index++ {
			if err := SetLinkTrust(ctx, db, protocol.NodeID{9, byte(index)}, true); err != nil {
				return err
			}
		}
		if err := SetLinkTrust(ctx, db, protocol.NodeID{10}, true); err == nil {
			return errors.New("peer capacity was exceeded")
		}
		return SetLinkTrust(ctx, db, address.ID, true)
	})
	if err != nil {
		t.Fatal(err)
	}
	err = storage.Node.Read(ctx, func(db *sql.Conn) error {
		peers, err := LinkPeers(ctx, db)
		if err != nil || len(peers) != 128 || peers[0] != address.ID {
			return fmt.Errorf("paired identities: %d, %w", len(peers), err)
		}
		addresses, err := LinkAddresses(ctx, db)
		if err != nil || !reflect.DeepEqual(addresses, []link.EndpointAddress{address}) {
			return fmt.Errorf("persisted peer addresses: %v, %w", addresses, err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	err = storage.Node.Write(ctx, func(db *sql.Conn) error {
		if err := SetLinkTrust(ctx, db, address.ID, false); err != nil {
			return err
		}
		addresses, err := LinkAddresses(ctx, db)
		if err != nil || len(addresses) != 0 {
			return fmt.Errorf("revocation retained addresses: %d, %w", len(addresses), err)
		}
		selected := protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{"https://relay.example/"}}}
		revision, err := SaveNetworkSelection(ctx, db, 0, selected)
		if err != nil || revision != 1 {
			return fmt.Errorf("network revision: %d, %w", revision, err)
		}
		if _, err := SaveNetworkSelection(ctx, db, 0, protocol.NetworkSelection{Kind: "iroh"}); err == nil {
			return errors.New("stale network save succeeded")
		}
		revision, restored, err := ReadNetworkSelection(ctx, db)
		if err != nil || revision != 1 || restored == nil || restored.Kind != "custom" {
			return fmt.Errorf("retained network selection: %d, %w", revision, err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
