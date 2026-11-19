package store

import (
	"bytes"
	"crypto/sha256"
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
	for _, name := range append(core.DatabaseFiles("node.turso"), core.DatabaseFiles("knowledge.turso")...) {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			profile := t.TempDir()
			directory := filepath.Join(profile, "storage")
			if err := core.PrivateDirectory(directory); err != nil {
				t.Fatal(err)
			}
			path := filepath.Join(directory, name)
			if err := os.WriteFile(path, nil, 0o600); err != nil {
				t.Fatal(err)
			}
			if selected, _, err := nodeDatabasePath(t.Context(), directory); err == nil {
				t.Fatalf("incomplete storage selected %s", selected)
			}
			archive := filepath.Join(directory, "rust-archive")
			if err := core.PrivateDirectory(archive); err != nil {
				t.Fatal(err)
			}
			archived := filepath.Join(archive, "node.turso")
			if err := os.WriteFile(archived, []byte("preserved"), 0o600); err != nil {
				t.Fatal(err)
			}
			if opened, err := Open(t.Context(), profile, protocol.NodeID{31}); err == nil {
				_ = opened.Close()
				t.Fatal("incomplete active storage opened")
			} else if !strings.Contains(err.Error(), name) || strings.Count(err.Error(), "existing data was not changed") != 1 {
				t.Fatalf("unexpected refusal for %s: %v", name, err)
			}
			if info, err := os.Lstat(path); err != nil || info.Size() != 0 {
				t.Fatalf("incomplete storage changed: %v", err)
			}
			if body, err := os.ReadFile(archived); err != nil || string(body) != "preserved" {
				t.Fatalf("archive changed: %v", err)
			}
			if entries, err := os.ReadDir(directory); err != nil || len(entries) != 2 {
				t.Fatalf("failed startup changed active storage: %v, %v", entries, err)
			}
		})
	}
}

func TestArchiveOnlyProfileStartsFreshAndReusesNativeStorage(t *testing.T) {
	t.Parallel()
	profile, node, original := schemaFixture(t)
	const lookup = "SELECT coalesce(max(value),0) FROM counters WHERE namespace='sessions'"
	const insert = "INSERT INTO counters(namespace,value) VALUES('sessions',?1)"
	schemaExecute(t, original.Node, insert, 7)
	schemaExecute(t, original.Knowledge, "INSERT INTO collections(id,generation,scope,rebuild) VALUES('archived',1,X'7B7D',0)")
	if err := original.Close(); err != nil {
		t.Fatal(err)
	}
	directory := filepath.Join(profile, "storage")
	archive := filepath.Join(directory, "rust-archive")
	if err := core.PrivateDirectory(archive); err != nil {
		t.Fatal(err)
	}
	unchanged := map[string][sha256.Size]byte{}
	for _, base := range []string{"node.turso", "knowledge.turso"} {
		for i, member := range core.DatabaseFiles(base) {
			source, archived := filepath.Join(directory, member), filepath.Join(archive, member)
			if err := os.Rename(source, archived); i > 0 && errors.Is(err, os.ErrNotExist) {
				continue
			} else if err != nil {
				t.Fatal(err)
			}
			body, err := os.ReadFile(archived)
			if err != nil {
				t.Fatal(err)
			}
			unchanged[archived] = sha256.Sum256(body)
		}
	}
	archivedFiles := len(unchanged)
	for _, name := range []string{"node.sqlite", "node.sqlite-wal", "node.sqlite-shm", "knowledge.sqlite", "knowledge.sqlite-wal", "knowledge.sqlite-shm"} {
		path := filepath.Join(directory, name)
		body := []byte("preview file " + name)
		if err := os.WriteFile(path, body, 0o600); err != nil {
			t.Fatal(err)
		}
		unchanged[path] = sha256.Sum256(body)
	}
	for _, want := range []int64{0, 11} {
		active := reopenSchemaFixture(t, profile, node)
		if active.Node.Path() != filepath.Join(directory, "node.turso") || active.Knowledge.Path() != filepath.Join(directory, "knowledge.turso") {
			t.Fatal("startup selected nonstandard storage")
		}
		if got := schemaInteger(t, active.Node, lookup); got != want {
			t.Fatalf("active sessions watermark = %d, want %d (archived watermark is 7)", got, want)
		}
		if got := schemaInteger(t, active.Knowledge, "SELECT count(*) FROM collections WHERE id='archived'"); got != 0 {
			t.Fatal("archived knowledge was imported")
		}
		if want == 0 {
			schemaExecute(t, active.Node, insert, 11)
		}
		if err := active.Close(); err != nil {
			t.Fatal(err)
		}
		if entries, err := os.ReadDir(archive); err != nil || len(entries) != archivedFiles {
			t.Fatalf("archive members changed: %v, %v", entries, err)
		}
		for path, digest := range unchanged {
			if got, err := os.ReadFile(path); err != nil || sha256.Sum256(got) != digest {
				t.Fatalf("inactive file changed: %s, %v", path, err)
			}
		}
	}
}
