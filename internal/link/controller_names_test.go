//go:build unix

package link

import (
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestControllerDirectoryRetainsOfflineHostNames(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	path := filepath.Join(t.TempDir(), "controller")
	_, store, err := openControllerStore(path)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = store.Close() })
	source, direct, imported := protocol.NodeID{1}, protocol.NodeID{2}, protocol.NodeID{3}
	if err := store.PairMember(ctx, source, DirectionHost, InvitationScope{Kind: protocol.InvitationMyDevice}); err != nil {
		t.Fatal(err)
	}
	if err := store.PairPeer(ctx, direct, DirectionHost); err != nil {
		t.Fatal(err)
	}
	if err := store.RememberPeerName(ctx, direct, "Direct host"); err != nil {
		t.Fatal(err)
	}
	addresses := []EndpointAddress{{ID: direct}, {ID: imported}}
	if err := store.ApplyDirectory(ctx, source, 1, addresses, map[protocol.NodeID]string{direct: "Directory alias", imported: "Offline host"}); err != nil {
		t.Fatal(err)
	}
	if err := store.ApplyDirectory(ctx, source, 2, addresses, map[protocol.NodeID]string{imported: "Renamed offline host"}); err != nil {
		t.Fatal(err)
	}
	if err := store.ApplyDirectory(ctx, source, 1, addresses, map[protocol.NodeID]string{imported: "Stale name"}); err != nil {
		t.Fatal(err)
	}
	if err := store.Close(); err != nil {
		t.Fatal(err)
	}
	_, store, err = openControllerStore(path)
	if err != nil {
		t.Fatal(err)
	}
	values, err := store.Relationships(ctx)
	if err != nil {
		t.Fatal(err)
	}
	names := map[protocol.NodeID]string{}
	for _, value := range values {
		names[value.ID] = value.Name
	}
	if names[direct] != "Direct host" || names[imported] != "Renamed offline host" {
		t.Fatalf("persisted offline names: %#v", names)
	}
	if err := store.SetTrust(ctx, imported, false); err != nil {
		t.Fatal(err)
	}
	if err := store.RememberPeerName(ctx, imported, "Removed host"); err == nil {
		t.Fatal("removed host accepted new display metadata")
	}
}

func TestRememberPeerNameRequiresTrustAndSkipsUnchangedMetadata(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	_, store, err := openControllerStore(filepath.Join(t.TempDir(), "controller"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = store.Close() })
	endpoint := newFixtureLink(t)
	endpoint.store = store
	peer := protocol.NodeID{2}
	if err := endpoint.RememberPeerName(ctx, peer, "Untrusted"); err == nil {
		t.Fatal("unpaired name was retained")
	}
	if err := endpoint.pairMember(ctx, peer, DirectionHost, InvitationScope{Kind: protocol.InvitationOtherUser}); err != nil {
		t.Fatal(err)
	}
	if err := endpoint.RememberPeerName(ctx, peer, "Known host"); err != nil {
		t.Fatal(err)
	}
	endpoint.mu.Lock()
	changed := make(chan struct{})
	endpoint.peerChanged = changed
	endpoint.mu.Unlock()
	if err := endpoint.RememberPeerName(ctx, peer, "Known host"); err != nil {
		t.Fatal(err)
	}
	select {
	case <-changed:
		t.Fatal("unchanged name restarted peer discovery")
	default:
	}
	if err := endpoint.RememberPeerName(ctx, peer, "bad\nname"); err == nil {
		t.Fatal("directory name accepted a control character")
	}
}
