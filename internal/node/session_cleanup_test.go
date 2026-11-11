package node

import (
	"context"
	"database/sql"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"testing"
	"time"

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

	t.Run("a finishing upload keeps its file until its row commits", func(t *testing.T) {
		data := []byte("upload")
		spec := protocol.AttachmentSpec{Worktree: session.Worktree, Name: "upload.txt", MediaType: "text/plain", Size: uint64(len(data)), Revision: fmt.Sprintf("%x", blake3.Sum256(data))}
		upload := commandFixture(t, node, protocol.Command{Kind: "upload_attachment", Data: &spec}).Data.(protocol.AttachmentUpload)
		stream, err := node.Open(context.Background(), node.ID(), upload.Stream)
		if err != nil {
			t.Fatal(err)
		}
		defer stream.Close()
		if _, err = stream.Write(data); err == nil {
			err = stream.CloseWrite()
		}
		if err == nil {
			_, err = io.ReadFull(stream, make([]byte, 1))
		}
		if err != nil {
			t.Fatal(err)
		}
		// A finish takes the publication lock between its file and its row.
		unlock := node.agent.LockPublication()
		held := true
		defer func() {
			if held {
				unlock()
			}
		}()
		accepted, err := node.Dispatch(context.Background(), node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "finish_attachment_upload", Data: &protocol.CommandFinishAttachmentUploadPayload{Worktree: session.Worktree, Stream: upload.Stream}}))
		if err != nil {
			t.Fatal(err)
		}
		for deadline := time.Now().Add(10 * time.Second); ; time.Sleep(5 * time.Millisecond) {
			if entries, err := os.ReadDir(directory); err != nil {
				t.Fatal(err)
			} else if len(entries) == 4 {
				break
			}
			if time.Now().After(deadline) {
				t.Fatal("the upload was not published")
			}
		}
		collected := make(chan struct{})
		go func() {
			defer close(collected)
			node.collectAttachments(context.Background())
		}()
		select {
		case <-collected:
			t.Fatal("a collection ran between an upload's file and its row")
		case <-time.After(200 * time.Millisecond):
		}
		held = false
		unlock()
		result := <-accepted.Completion
		if result.Fault != nil {
			t.Fatal(result.Fault)
		}
		<-collected
		published := result.Output.Data.(protocol.Attachment)
		if !present(published.ID.String()) {
			t.Fatal("a collection removed the file of a finished upload")
		}
		commandFixture(t, node, protocol.Command{Kind: "discard_attachment", Data: &protocol.CommandDiscardAttachmentPayload{Worktree: session.Worktree, Attachment: published.ID}})
		if present(published.ID.String()) {
			t.Fatal("a discarded upload kept its file")
		}
	})
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
		// A removal just before shutdown still finishes its file cleanup.
		removed := sessionFixture(t, node)
		artifacts := filepath.Join(profile, "artifacts")
		for _, owner := range []string{removed.ID.String(), session.ID.String(), "notes"} {
			if err := os.MkdirAll(filepath.Join(artifacts, owner, protocol.NewArtifactID().String()), 0o700); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(artifacts, owner, "1.html"), []byte("<p>Report</p>"), 0o600); err != nil {
				t.Fatal(err)
			}
		}
		commandFixture(t, node, protocol.Command{Kind: "remove_session", Data: &protocol.CommandRemoveSessionPayload{Session: removed.ID, ExpectedRevision: removed.Revision}})
		unfinished := stored(protocol.NewAttachmentID().String())
		stopped = true
		if err := node.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
		if entries, err := os.ReadDir(artifacts); err != nil || len(entries) != 2 || entries[0].Name() == removed.ID.String() || entries[1].Name() == removed.ID.String() {
			t.Fatalf("removed session artifact directories: %v, %v", entries, err)
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
