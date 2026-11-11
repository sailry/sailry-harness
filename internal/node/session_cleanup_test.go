package node

import (
	"context"
	"database/sql"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// Over 1 s: each Node start installs and verifies the bundled packages.
func TestStoredFileCollection(t *testing.T) {
	t.Parallel()
	profile := filepath.Join(t.TempDir(), "profile")
	node, err := startFixtureNode(context.Background(), profile)
	if err != nil {
		t.Fatal(err)
	}
	stopped := false
	t.Cleanup(func() {
		if !stopped {
			_ = node.Stop(context.Background())
		}
	})
	session := sessionFixture(t, node)
	directory := filepath.Join(profile, "attachments")
	if err := os.MkdirAll(filepath.Join(directory, protocol.NewAttachmentID().String()), 0o700); err != nil {
		t.Fatal(err)
	}
	stored := func(name string) string {
		t.Helper()
		if err := os.WriteFile(filepath.Join(directory, name), []byte("input"), 0o600); err != nil {
			t.Fatal(err)
		}
		return name
	}
	present := func(name string) bool {
		t.Helper()
		_, err := os.Lstat(filepath.Join(directory, name))
		if err != nil && !os.IsNotExist(err) {
			t.Fatal(err)
		}
		return err == nil
	}
	attachment := protocol.Attachment{ID: protocol.NewAttachmentID(), Spec: protocol.AttachmentSpec{Worktree: session.Worktree, Name: "input.txt", MediaType: "text/plain", Size: 5, Revision: fmt.Sprintf("%x", blake3.Sum256([]byte("input")))}}
	if err := node.profile.Store.Node.Write(context.Background(), func(db *sql.Conn) error {
		return store.InsertAttachment(context.Background(), db, node.ID(), attachment)
	}); err != nil {
		t.Fatal(err)
	}
	retained, foreign := stored(attachment.ID.String()), stored("notes.txt")
	entries, err := os.ReadDir(directory)
	if err != nil || len(entries) != 3 {
		t.Fatalf("fixture entries: %v, %v", entries, err)
	}

	t.Run("a discarded attachment loses its file at once", func(t *testing.T) {
		other := protocol.Attachment{ID: protocol.NewAttachmentID(), Spec: attachment.Spec}
		if err := node.profile.Store.Node.Write(context.Background(), func(db *sql.Conn) error {
			return store.InsertAttachment(context.Background(), db, node.ID(), other)
		}); err != nil {
			t.Fatal(err)
		}
		discarded, unfinished := stored(other.ID.String()), stored(protocol.NewAttachmentID().String())
		commandFixture(t, node, protocol.Command{Kind: "discard_attachment", Data: &protocol.CommandDiscardAttachmentPayload{Worktree: session.Worktree, Attachment: other.ID}})
		if present(discarded) || present(unfinished) {
			t.Fatal("a discarded attachment kept its file")
		}
		if !present(retained) || !present(foreign) {
			t.Fatal("discarding removed a file it does not own")
		}
	})
	t.Run("shutdown removes files without a row", func(t *testing.T) {
		unfinished := stored(protocol.NewAttachmentID().String())
		stopped = true
		if err := node.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
		if present(unfinished) {
			t.Fatal("shutdown kept an upload that has no row")
		}
		if !present(retained) || !present(foreign) {
			t.Fatal("shutdown removed a file it does not own")
		}
	})
	t.Run("start removes files without a row", func(t *testing.T) {
		unfinished := stored(protocol.NewAttachmentID().String())
		restarted, err := startFixtureNode(context.Background(), profile)
		if err != nil {
			t.Fatal(err)
		}
		defer func() {
			if err := restarted.Stop(context.Background()); err != nil {
				t.Error(err)
			}
		}()
		if present(unfinished) {
			t.Fatal("start kept an upload that has no row")
		}
		if !present(retained) || !present(foreign) {
			t.Fatal("start removed a file it does not own")
		}
		if entries, err := os.ReadDir(directory); err != nil || len(entries) != 3 {
			t.Fatalf("collection changed other entries: %v, %v", entries, err)
		}
	})
}
