package node

import (
	"database/sql"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPairedAdministratorBrowsesHostHome(t *testing.T) {
	home, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	t.Setenv("HOME", home)
	t.Setenv("USERPROFILE", home)
	if err := os.Mkdir(filepath.Join(home, "project"), 0o700); err != nil {
		t.Fatal(err)
	}
	node := nodeFixture(t)
	administrator, other := protocol.NodeID{73}, protocol.NodeID{74}
	if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		if err := store.PairLinkPeer(t.Context(), db, administrator, link.DirectionController); err != nil {
			return err
		}
		return store.PairLinkPeer(t.Context(), db, other, link.DirectionController)
	}); err != nil {
		t.Fatal(err)
	}
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "browse_files", Data: protocol.CommandBrowseFilesPayload{}})
	response := dispatchFixture(t, node, administrator, request)
	if response.Fault != nil {
		t.Fatal(response.Fault)
	}
	listing := response.Output.Data.(protocol.FileBrowserListing)
	if listing.Directory.Path != home || len(listing.Directory.Entries) != 1 || listing.Directory.Entries[0].Name != "project" {
		t.Fatalf("remote home listing = %+v", listing)
	}
	if denied := dispatchFixture(t, node, other, request); denied.Fault == nil || denied.Fault.Code != protocol.ErrorCodePermissionDenied {
		t.Fatalf("non-administrator browsed host filesystem: %+v", denied)
	}
}
