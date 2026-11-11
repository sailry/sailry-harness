package files

import (
	"context"
	"io"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func transferFixture(t *testing.T) (*Transfers, string, protocol.NodeID) {
	t.Helper()
	root, _ := filesFixture(t)
	profile, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	transfers := NewTransfers(profile)
	transfers.afterFunc = func(time.Duration, func()) func() bool { return func() bool { return true } }
	transfers.withTimeout = func(ctx context.Context, _ time.Duration) (context.Context, context.CancelFunc) {
		return context.WithCancel(ctx)
	}
	transfers.now = func() time.Time { return time.Date(2026, 11, 10, 0, 0, 0, 0, time.UTC) }
	t.Cleanup(func() { _ = transfers.Close() })
	var caller protocol.NodeID
	caller[0] = 1
	return transfers, root, caller
}
func TestTransferRegistryAndStreams(t *testing.T) {
	t.Parallel()
	ctx := context.Background()
	for _, scenario := range []string{"publish", "digest mismatch", "expiry", "active cancellation"} {
		t.Run(scenario, func(t *testing.T) {
			t.Parallel()
			transfers, root, caller := transferFixture(t)
			spec := protocol.FileUploadSpec{Worktree: protocol.NewWorktreeID(), Path: "received.txt", Size: 5, Revision: revision([]byte("hello"))}
			upload, err := transfers.UploadFile(ctx, caller, root, spec, core.ManualFiles)
			if err != nil {
				t.Fatal(err)
			}
			other := caller
			other[0]++
			if _, err = transfers.Open(ctx, other, upload.Stream); err == nil {
				t.Fatal("another caller opened the stream")
			}
			if scenario == "expiry" {
				transfers.mu.Lock()
				transfers.entries[upload.Stream].expires = transfers.now().Add(-time.Second)
				transfers.mu.Unlock()
				if _, err = transfers.Open(ctx, caller, upload.Stream); err == nil {
					t.Fatal("expired stream opened")
				}
				return
			}
			stream, err := transfers.Open(ctx, caller, upload.Stream)
			if err != nil {
				t.Fatal(err)
			}
			defer stream.Close()
			if _, err = transfers.Open(ctx, caller, upload.Stream); err == nil {
				t.Fatal("stream opened twice")
			}
			if scenario == "active cancellation" {
				if err = transfers.Cancel(caller, upload.Stream); err != nil {
					t.Fatal(err)
				}
				if _, err = stream.Write([]byte("hello")); err == nil {
					t.Fatal("cancelled stream accepted bytes")
				}
				return
			}
			payload := "hello"
			if scenario == "digest mismatch" {
				payload = "wrong"
			}
			if _, err = io.WriteString(stream, payload); err != nil {
				t.Fatal(err)
			}
			if err = stream.CloseWrite(); err != nil {
				t.Fatal(err)
			}
			ready, err := io.ReadAll(stream)
			if err != nil {
				t.Fatal(err)
			}
			finish := protocol.Command{Kind: "finish_file_upload", Data: &protocol.CommandFinishFileUploadPayload{Worktree: spec.Worktree, Path: spec.Path, Stream: upload.Stream}}
			output, err := transfers.Finish(ctx, caller, root, finish)
			if scenario == "digest mismatch" {
				if err == nil || len(ready) != 0 {
					t.Fatalf("mismatched upload published: %v %v", ready, err)
				}
				return
			}
			if err != nil || len(ready) != 1 || ready[0] != protocol.FILE_UPLOAD_READY || output.Kind != "file_written" {
				t.Fatalf("finish: %v %+v %v", ready, output, err)
			}
			data, err := os.ReadFile(filepath.Join(root, spec.Path))
			if err != nil || string(data) != "hello" {
				t.Fatalf("published bytes %q %v", data, err)
			}
			download, err := transfers.DownloadFile(ctx, caller, root, spec.Worktree, spec.Path, core.ManualFiles)
			if err != nil {
				t.Fatal(err)
			}
			received, err := transfers.Open(ctx, caller, download.Stream)
			if err != nil {
				t.Fatal(err)
			}
			defer received.Close()
			_ = received.CloseWrite()
			data, err = io.ReadAll(received)
			if err != nil || string(data) != "hello" || download.Revision != spec.Revision || download.Stamp == "" {
				t.Fatalf("download %q %+v %v", data, download, err)
			}
			if _, err = transfers.Finish(ctx, caller, root, finish); err == nil {
				t.Fatal("finished upload was replayed")
			}
		})
	}
	t.Run("SSH transfer scope", func(t *testing.T) {
		transfers, project, _ := transferFixture(t)
		profile := transfers.profile
		connection, err := core.ConnectionWorkspace(profile, protocol.ConnectionResource{Kind: "ssh", Data: protocol.NewSshID()})
		if err != nil {
			t.Fatal(err)
		}
		for _, row := range []struct {
			name, root, path string
			storage          bool
		}{
			{"project", project, "notes.txt", false},
			{"connection workspace", connection, "notes.txt", false},
			{"session workspace", core.SessionWorkspace(profile, protocol.NewSessionID()), "nested/notes.txt", false},
			{"profile", profile, "node.db", true},
			{"profile reaching a workspace", profile, "connections/ssh/notes.txt", true},
			{"workspace parent", filepath.Dir(connection), "workspace/notes.txt", true},
			{"workspace descendant", filepath.Join(connection, "nested"), "notes.txt", true},
			{"unregistered directory", filepath.Join(profile, "tmp"), "notes.txt", true},
		} {
			if got := transfers.storage(row.root, row.path); got != row.storage {
				t.Errorf("%s: Node storage = %v, want %v", row.name, got, row.storage)
			}
		}
	})
	t.Run("verified command inputs", func(t *testing.T) {
		transfers, root, caller := transferFixture(t)
		spec := protocol.AttachmentSpec{Worktree: protocol.NewWorktreeID(), Name: "display-only.txt", MediaType: "text/plain", Size: 5, Revision: revision([]byte("input"))}
		upload, err := transfers.UploadAttachment(ctx, caller, root, spec)
		if err != nil {
			t.Fatal(err)
		}
		stream, err := transfers.Open(ctx, caller, upload.Stream)
		if err != nil {
			t.Fatal(err)
		}
		if _, err = io.WriteString(stream, "input"); err != nil {
			t.Fatal(err)
		}
		if err = stream.CloseWrite(); err != nil {
			t.Fatal(err)
		}
		ready, err := io.ReadAll(stream)
		stream.Close()
		if err != nil || len(ready) != 1 || ready[0] != protocol.FILE_UPLOAD_READY {
			t.Fatalf("attachment readiness: %v %v", ready, err)
		}
		output, err := transfers.Finish(ctx, caller, root, protocol.Command{Kind: "finish_attachment_upload", Data: &protocol.CommandFinishAttachmentUploadPayload{Worktree: spec.Worktree, Stream: upload.Stream}})
		if err != nil {
			t.Fatal(err)
		}
		attachment := output.Data.(protocol.Attachment)
		directory, release, err := transfers.StageInputs(ctx, []protocol.Attachment{attachment})
		if err != nil {
			t.Fatal(err)
		}
		data, err := os.ReadFile(filepath.Join(directory, attachment.ID.String()))
		if err != nil || string(data) != "input" {
			t.Fatalf("staged input: %q %v", data, err)
		}
		if _, err = os.Stat(filepath.Join(directory, spec.Name)); !os.IsNotExist(err) {
			t.Fatalf("display name became a storage path: %v", err)
		}
		release()
		release()
		if _, err = os.Stat(directory); !os.IsNotExist(err) {
			t.Fatalf("command scratch was not released: %v", err)
		}
		data, err = transfers.AttachmentBytes(ctx, attachment)
		if err != nil || string(data) != "input" {
			t.Fatalf("immutable attachment changed after cleanup: %q %v", data, err)
		}
	})

	t.Run("injected idle expiry", func(t *testing.T) {
		transfers, _, _ := transferFixture(t)
		callbacks := make(chan func(), 1)
		transfers.afterFunc = func(_ time.Duration, f func()) func() bool { callbacks <- f; return func() bool { return false } }
		client, service := link.StreamPair()
		defer client.Close()
		defer service.Close()
		done := make(chan error, 1)
		go func() {
			done <- transfers.transferStep(ctx, service, func() error { var b [1]byte; _, err := service.Read(b[:]); return err })
		}()
		callback := <-callbacks
		callback()
		if err := <-done; err == nil {
			t.Fatal("idle expiry did not stop the stream")
		}
	})
}
