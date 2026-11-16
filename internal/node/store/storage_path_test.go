package store

import (
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestNativeStorageIgnoresPreviewDatabases(t *testing.T) {
	t.Parallel()
	for _, active := range []bool{false, true} {
		name := "new native storage"
		if active {
			name = "existing native storage"
		}
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			profile := t.TempDir()
			directory := filepath.Join(profile, "storage")
			if err := core.PrivateDirectory(directory); err != nil {
				t.Fatal(err)
			}
			node := protocol.NodeID{31}
			nativePath := filepath.Join(directory, "node.turso")
			var before os.FileInfo
			if active {
				storage, err := Open(t.Context(), profile, node)
				if err != nil {
					t.Fatal(err)
				}
				if err := storage.Close(); err != nil {
					t.Fatal(err)
				}
				before, err = os.Stat(nativePath)
				if err != nil {
					t.Fatal(err)
				}
			}
			preview := map[string][]byte{}
			for _, name := range []string{"node.sqlite", "node.sqlite-wal", "node.sqlite-shm", "knowledge.sqlite", "knowledge.sqlite-wal", "knowledge.sqlite-shm"} {
				path := filepath.Join(directory, name)
				body := []byte("preview file " + name)
				if err := os.WriteFile(path, body, 0o600); err != nil {
					t.Fatal(err)
				}
				preview[path] = body
			}
			storage, err := Open(t.Context(), profile, node)
			if err != nil {
				t.Fatal(err)
			}
			if storage.Node.Path() != nativePath || storage.Knowledge.Path() != filepath.Join(directory, "knowledge.turso") {
				_ = storage.Close()
				t.Fatalf("selected preview storage: %s, %s", storage.Node.Path(), storage.Knowledge.Path())
			}
			if err := storage.Close(); err != nil {
				t.Fatal(err)
			}
			after, err := os.Stat(nativePath)
			if err != nil || active && !os.SameFile(before, after) {
				t.Fatalf("native database was not preserved: %v", err)
			}
			for path, original := range preview {
				if body, err := os.ReadFile(path); err != nil || !bytes.Equal(body, original) {
					t.Fatalf("preview file changed: %s, %v", path, err)
				}
			}
			for _, name := range []string{"rust-archive", "node.sqlite-tshm", "knowledge.sqlite-tshm"} {
				if _, err := os.Lstat(filepath.Join(directory, name)); !errors.Is(err, os.ErrNotExist) {
					t.Fatalf("preview handling created %s: %v", name, err)
				}
			}
		})
	}
}

func TestNodeDatabasePathPreservesIncompleteStorage(t *testing.T) {
	t.Parallel()
	for _, name := range core.DatabaseFiles("node.turso") {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			directory := t.TempDir()
			path := filepath.Join(directory, name)
			if err := os.WriteFile(path, nil, 0o600); err != nil {
				t.Fatal(err)
			}
			if selected, err := nodeDatabasePath(t.Context(), directory); err == nil {
				t.Fatalf("incomplete storage selected %s", selected)
			}
			if info, err := os.Lstat(path); err != nil || info.Size() != 0 {
				t.Fatalf("incomplete storage changed: %v", err)
			}
		})
	}
}

func TestArchivedNativeStoragePreventsEmptyInitialization(t *testing.T) {
	t.Parallel()
	profile := t.TempDir()
	directory := filepath.Join(profile, "storage")
	archive := filepath.Join(directory, "rust-archive")
	if err := core.PrivateDirectory(archive); err != nil {
		t.Fatal(err)
	}
	original := []byte("preserved native database")
	path := filepath.Join(archive, "node.turso")
	if err := os.WriteFile(path, original, 0o600); err != nil {
		t.Fatal(err)
	}
	preview := filepath.Join(directory, "node.sqlite")
	if err := os.WriteFile(preview, []byte("preview database"), 0o600); err != nil {
		t.Fatal(err)
	}
	storage, err := Open(t.Context(), profile, protocol.NodeID{31})
	if storage != nil {
		_ = storage.Close()
	}
	if err == nil || !strings.Contains(err.Error(), "preserved") {
		t.Fatalf("archived native storage was ignored: %v", err)
	}
	if body, err := os.ReadFile(path); err != nil || !bytes.Equal(body, original) {
		t.Fatalf("archived native storage changed: %v", err)
	}
	if body, err := os.ReadFile(preview); err != nil || string(body) != "preview database" {
		t.Fatalf("preview storage changed: %v", err)
	}
	for _, name := range []string{"node.turso", "knowledge.turso", "backups"} {
		if _, err := os.Lstat(filepath.Join(directory, name)); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("archived native storage created %s: %v", name, err)
		}
	}
}
