//go:build unix

package link

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestControllerStorePersistsIdentityAndPairedAddresses(t *testing.T) {
	t.Parallel()
	path := filepath.Join(t.TempDir(), "controller")
	identity, store, err := openControllerStore(path)
	if err != nil {
		t.Fatal(err)
	}
	address := EndpointAddress{ID: protocol.NodeID{8}, Addrs: []json.RawMessage{
		json.RawMessage(`{"Ip":"127.0.0.1:4321"}`),
		json.RawMessage(`{"Relay":"https://relay.example/"}`),
	}}
	if err := store.Remember(t.Context(), address); err == nil {
		t.Fatal("unpaired address was persisted")
	}
	if err := store.SetTrust(t.Context(), address.ID, true); err != nil {
		t.Fatal(err)
	}
	if err := store.Remember(t.Context(), address); err != nil {
		t.Fatal(err)
	}
	if _, duplicate, err := openControllerStore(path); err == nil {
		_ = duplicate.Close()
		t.Fatal("one controller profile acquired two endpoint owners")
	}
	if err := store.Close(); err != nil {
		t.Fatal(err)
	}
	reopenedIdentity, reopened, err := openControllerStore(path)
	if err != nil {
		t.Fatal(err)
	}
	defer reopened.Close()
	if reopenedIdentity.ID() != identity.ID() {
		t.Fatal("controller identity changed across reopen")
	}
	peers, err := reopened.Peers(t.Context())
	if err != nil || !reflect.DeepEqual(peers, []protocol.NodeID{address.ID}) {
		t.Fatalf("persisted peers: %#v, %v", peers, err)
	}
	addresses, err := reopened.Addresses(t.Context())
	if err != nil || !reflect.DeepEqual(addresses, []EndpointAddress{address}) {
		t.Fatalf("persisted addresses: %#v, %v", addresses, err)
	}
	addresses[0].Addrs[0][0] = '!'
	unchanged, err := reopened.Addresses(t.Context())
	if err != nil || !reflect.DeepEqual(unchanged, []EndpointAddress{address}) {
		t.Fatal("address caller mutated controller-owned peer state")
	}
	if err := reopened.SetTrust(t.Context(), address.ID, false); err != nil {
		t.Fatal(err)
	}
	addresses, err = reopened.Addresses(t.Context())
	if err != nil || len(addresses) != 0 {
		t.Fatal("revocation retained its authenticated address")
	}
}

func TestControllerStoreRejectsNodeProfileAndInvalidFiles(t *testing.T) {
	t.Parallel()
	t.Run("actual_go_node_database", func(t *testing.T) {
		directory := filepath.Join(t.TempDir(), "controller")
		if err := os.Mkdir(directory, 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.Mkdir(filepath.Join(directory, "storage"), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(directory, "storage", "node.sqlite"), []byte("fixture"), 0o600); err != nil {
			t.Fatal(err)
		}
		if _, store, err := openControllerStore(directory); err == nil {
			_ = store.Close()
			t.Fatal("Node database was opened as a controller")
		}
	})
	for _, kind := range []string{"public_peers", "symlink_peers"} {
		t.Run(kind, func(t *testing.T) {
			directory := filepath.Join(t.TempDir(), "controller")
			_, store, err := openControllerStore(directory)
			if err != nil {
				t.Fatal(err)
			}
			if err := store.Close(); err != nil {
				t.Fatal(err)
			}
			peers := filepath.Join(directory, "peers.json")
			if kind == "public_peers" {
				if err := os.Chmod(peers, 0o644); err != nil {
					t.Fatal(err)
				}
			} else {
				target := filepath.Join(directory, "private.json")
				if err := os.Rename(peers, target); err != nil {
					t.Fatal(err)
				}
				if err := os.Symlink(target, peers); err != nil {
					t.Fatal(err)
				}
			}
			if _, store, err := openControllerStore(directory); err == nil {
				_ = store.Close()
				t.Fatal("invalid controller peer file was accepted")
			}
		})
	}
}

func TestControllerRecordRejectsBindingAndCapacity(t *testing.T) {
	t.Parallel()
	peer := protocol.NodeID{1}
	address := EndpointAddress{ID: protocol.NodeID{2}}
	for _, peers := range [][]controllerPeer{
		{{ID: peer}, {ID: peer}},
		{{ID: peer, Address: &address}},
		make([]controllerPeer, maxControllerPeers+1),
	} {
		encoded, err := json.Marshal(controllerRecord{Version: 1, Peers: peers})
		if err != nil {
			t.Fatal(err)
		}
		if _, err := decodeControllerRecord(encoded); err == nil {
			t.Fatal("invalid controller record was accepted")
		}
	}
	for _, encoded := range []string{
		`{"version":1,"version":1,"peers":[]}`,
		`{"version":2,"peers":[]}`,
		`{"version":1,"peers":null}`,
	} {
		if _, err := decodeControllerRecord([]byte(encoded)); err == nil {
			t.Fatalf("invalid controller record was accepted: %s", encoded)
		}
	}
	_, store, err := openControllerStore(filepath.Join(t.TempDir(), "controller"))
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	cancelled, cancel := context.WithCancel(t.Context())
	cancel()
	if err := store.SetTrust(cancelled, peer, true); err == nil {
		t.Fatal("cancelled controller write was accepted")
	}
}
