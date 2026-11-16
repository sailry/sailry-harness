//go:build unix

package link

import (
	"encoding/json"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestControllerHostRemovalRetainsUnknownAfterLocalForgetAndReopen(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	path := filepath.Join(t.TempDir(), "controller")
	_, store, err := openControllerStore(path)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = store.Close() }()
	host := protocol.NodeID{2}
	if err := store.PairPeer(ctx, host, DirectionHost); err != nil {
		t.Fatal(err)
	}
	request := protocol.NewRequest(host, protocol.Command{Kind: "identity", Data: protocol.IdentityCommand{Kind: "revoke_device", Data: protocol.RevokeDevice{Device: protocol.NodeID{1}, ConfirmLast: true}}})
	first, err := store.PrepareHostRemoval(ctx, host, request)
	if err != nil {
		t.Fatal(err)
	}
	if err := store.RecordHostRemoval(ctx, host, first.Request.ID, "unknown", &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "reply unavailable"}); err != nil {
		t.Fatal(err)
	}
	if err := store.SetTrust(ctx, host, false); err != nil {
		t.Fatal(err)
	}
	if err := store.Close(); err != nil {
		t.Fatal(err)
	}
	_, store, err = openControllerStore(path)
	if err != nil {
		t.Fatal(err)
	}
	proposed := protocol.NewRequest(host, request.Command)
	retained, err := store.PrepareHostRemoval(ctx, host, proposed)
	if err != nil {
		t.Fatal(err)
	}
	before, _ := json.Marshal(first.Request)
	after, _ := json.Marshal(retained.Request)
	if string(before) != string(after) || retained.State != "unknown" {
		t.Fatal("local forget/reopen lost original remote request")
	}
	if err := store.PairPeer(ctx, host, DirectionHost); err != nil {
		t.Fatal(err)
	}
	newAttempt, err := store.PrepareHostRemoval(ctx, host, proposed)
	if err != nil {
		t.Fatal(err)
	}
	if newAttempt.Request.ID != proposed.ID || newAttempt.Request.ID == first.Request.ID {
		t.Fatal("new explicit pairing reused the previous removal effect")
	}
	history, err := store.HostRemovals(ctx)
	if err != nil || len(history) != 2 || history[0].Active || !history[1].Active {
		t.Fatalf("original unknown history was deleted: %+v %v", history, err)
	}
}
