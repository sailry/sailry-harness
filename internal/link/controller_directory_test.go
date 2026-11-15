//go:build unix

package link

import (
	"encoding/json"
	"path/filepath"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestControllerDirectoryPreservesDirectPairsAndRemovalProvenance(t *testing.T) {
	t.Parallel()
	path := filepath.Join(t.TempDir(), "controller")
	_, store, err := openControllerStore(path)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = store.Close() }()
	ctx := t.Context()
	first, second, direct, imported, shared := protocol.NodeID{1}, protocol.NodeID{2}, protocol.NodeID{3}, protocol.NodeID{4}, protocol.NodeID{5}
	for _, source := range []protocol.NodeID{first, second} {
		if err := store.PairMember(ctx, source, DirectionHost, InvitationScope{Kind: protocol.InvitationMyDevice}); err != nil {
			t.Fatal(err)
		}
	}
	if err := store.PairPeer(ctx, direct, DirectionHost); err != nil {
		t.Fatal(err)
	}
	if err := store.Remember(ctx, EndpointAddress{ID: direct, Addrs: []json.RawMessage{json.RawMessage(`{"Ip":"127.0.0.1:4000"}`)}}); err != nil {
		t.Fatal(err)
	}
	values := []EndpointAddress{{ID: direct, Addrs: []json.RawMessage{json.RawMessage(`{"Relay":"https://relay.example.test/"}`)}}, {ID: imported}, {ID: shared}}
	if err := store.ApplyDirectory(ctx, direct, 1, values); err == nil {
		t.Fatal("ordinary pair imported a my-device directory")
	}
	if err := store.ApplyDirectory(ctx, first, 1, values); err != nil {
		t.Fatal(err)
	}
	if err := store.ApplyDirectory(ctx, second, 1, []EndpointAddress{{ID: shared}}); err != nil {
		t.Fatal(err)
	}
	addresses, err := store.Addresses(ctx)
	if err != nil {
		t.Fatal(err)
	}
	for _, address := range addresses {
		if address.ID == direct && len(address.Addrs) != 2 {
			t.Fatal("directory discarded direct address hints")
		}
	}
	if err := store.SetTrust(ctx, imported, false); err != nil {
		t.Fatal(err)
	}
	if err := store.ApplyDirectory(ctx, first, 2, values); err != nil {
		t.Fatal(err)
	}
	peers, err := store.Peers(ctx)
	if err != nil || slices.Contains(peers, imported) {
		t.Fatal("directory undid an explicit local removal")
	}
	if err := store.SetTrust(ctx, first, false); err != nil {
		t.Fatal(err)
	}
	peers, err = store.Peers(ctx)
	if err != nil || !slices.Contains(peers, direct) || !slices.Contains(peers, shared) {
		t.Fatal("removing one source removed a direct or another-source pair")
	}
	if err := store.ApplyDirectory(ctx, second, 2, []EndpointAddress{}); err != nil {
		t.Fatal(err)
	}
	if err := store.ApplyDirectory(ctx, second, 1, []EndpointAddress{{ID: shared}}); err != nil {
		t.Fatal(err)
	}
	peers, err = store.Peers(ctx)
	if err != nil || slices.Contains(peers, shared) || !slices.Contains(peers, direct) {
		t.Fatal("stale directory reintroduced a removed origin")
	}
	if err := store.Close(); err != nil {
		t.Fatal(err)
	}
	_, store, err = openControllerStore(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := store.PairMember(ctx, first, DirectionHost, InvitationScope{Kind: protocol.InvitationMyDevice}); err != nil {
		t.Fatal(err)
	}
	if err := store.ApplyDirectory(ctx, first, 3, values); err != nil {
		t.Fatal(err)
	}
	peers, err = store.Peers(ctx)
	if err != nil || slices.Contains(peers, imported) {
		t.Fatal("reopen forgot a local removal")
	}
	if err := store.PairPeer(ctx, imported, DirectionHost); err != nil {
		t.Fatal(err)
	}
	peers, err = store.Peers(ctx)
	if err != nil || !slices.Contains(peers, imported) {
		t.Fatal("explicit pairing could not restore removed device")
	}
}
