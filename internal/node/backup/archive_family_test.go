package backup

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestNativeDatabaseFamilyArchive(t *testing.T) {
	t.Parallel()
	for _, nodeName := range []string{"node.turso"} {
		t.Run(nodeName, func(t *testing.T) {
			profile, files := fixtureProfile(t)
			for _, name := range []string{nodeName, databaseNames[1]} {
				for _, member := range core.DatabaseFiles(name)[1:] {
					files["storage/"+member] = []byte("native " + member)
				}
			}
			snapshot := fixtureSnapshot(t, profile, files, fixtureRoots)
			var archive bytes.Buffer
			if err := snapshot.archive(t.Context(), &archive); err != nil {
				t.Fatal(err)
			}
			restored := t.TempDir()
			manifest, err := Unpack(t.Context(), &archive, restored)
			if err != nil {
				t.Fatal(err)
			}
			for path, want := range files {
				got, err := os.ReadFile(filepath.Join(restored, filepath.FromSlash(path)))
				if err != nil || !bytes.Equal(got, want) {
					t.Fatalf("restored %s = %q, %v", path, got, err)
				}
			}
			// Opening a restored family can rewrite logs. Refresh includes updates and
			// removals while keeping unrelated content unchanged.
			removed := "storage/" + nodeName + "-tshm"
			if err := os.Remove(filepath.Join(restored, filepath.FromSlash(removed))); err != nil {
				t.Fatal(err)
			}
			updated := "storage/knowledge.db-log"
			content := []byte("updated native MVCC log")
			if err := os.WriteFile(filepath.Join(restored, filepath.FromSlash(updated)), content, 0o600); err != nil {
				t.Fatal(err)
			}
			if err := manifest.RefreshDatabases(t.Context(), restored); err != nil {
				t.Fatal(err)
			}
			if slices.ContainsFunc(manifest.Files, func(file File) bool { return file.Path == removed }) {
				t.Fatal("removed native log remains in the manifest")
			}
			index := slices.IndexFunc(manifest.Files, func(file File) bool { return file.Path == updated })
			sum := sha256.Sum256(content)
			if index < 0 || manifest.Files[index].SHA256 != hex.EncodeToString(sum[:]) || manifest.Files[index].Size != uint64(len(content)) {
				t.Fatal("rewritten native log was not refreshed")
			}
		})
	}
}

func TestFreezeRejectsNonRegularDatabaseMember(t *testing.T) {
	t.Parallel()
	for _, member := range core.DatabaseFiles("node.turso") {
		t.Run(member, func(t *testing.T) {
			snapshot, err := NewSnapshot(t.TempDir(), protocol.NodeID{})
			if err != nil {
				t.Fatal(err)
			}
			defer snapshot.Close()
			err = snapshot.Freeze(t.Context(), "node.turso", func(_ context.Context, target string) error {
				if member != "node.turso" {
					if err := os.WriteFile(target, []byte("database"), 0o600); err != nil {
						return err
					}
				}
				return os.Mkdir(filepath.Join(filepath.Dir(target), member), 0o700)
			})
			if code(err) != protocol.ErrorCodeInvalidRequest {
				t.Fatalf("non-regular database member accepted: %v", err)
			}
		})
	}
}

func TestInvalidDatabaseFamilyArchive(t *testing.T) {
	t.Parallel()
	for _, name := range []string{"node.sqlite", "knowledge.sqlite", "node.sqlite-wal", "knowledge.sqlite-shm"} {
		t.Run(name, func(t *testing.T) {
			digest := strings.Repeat("0", 64)
			manifest := Manifest{Version: 1, OriginalProfile: t.TempDir(), Files: []File{
				{Path: "link.key", SHA256: digest, Mode: 0o600},
				{Path: "storage/node.turso", SHA256: digest, Mode: 0o600},
				{Path: "storage/" + name, SHA256: digest, Mode: 0o600},
			}}
			err := manifest.Validate()
			if code(err) != protocol.ErrorCodeInvalidRequest {
				t.Fatalf("invalid database family archive = %v", err)
			}
			body, err := json.Marshal(manifest)
			if err != nil {
				t.Fatal(err)
			}
			var buffer bytes.Buffer
			compressed := gzip.NewWriter(&buffer)
			archive := tar.NewWriter(compressed)
			if err := archive.WriteHeader(&tar.Header{Name: manifestName, Typeflag: tar.TypeReg, Mode: 0o600, Size: int64(len(body))}); err != nil {
				t.Fatal(err)
			}
			if _, err := archive.Write(body); err != nil {
				t.Fatal(err)
			}
			if err := errors.Join(archive.Close(), compressed.Close()); err != nil {
				t.Fatal(err)
			}
			destination := t.TempDir()
			if _, err := Unpack(t.Context(), &buffer, destination); code(err) != protocol.ErrorCodeInvalidRequest {
				t.Fatalf("unpacked invalid database family archive = %v", err)
			}
			entries, err := os.ReadDir(destination)
			if err != nil || len(entries) != 0 {
				t.Fatalf("invalid database family archive wrote files: %v, %v", entries, err)
			}
		})
	}
}
