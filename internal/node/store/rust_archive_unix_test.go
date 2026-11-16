//go:build unix

package store

import (
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestStartupPreservesLinkedResourcesWhenStorageIsIncompatible(t *testing.T) {
	t.Parallel()
	for _, linked := range []string{"plugins/packages/private", "attachments"} {
		t.Run(linked, func(t *testing.T) {
			t.Parallel()
			profile := t.TempDir()
			storage := filepath.Join(profile, "storage")
			if err := core.PrivateDirectory(storage); err != nil {
				t.Fatal(err)
			}
			database := filepath.Join(storage, "node.sqlite")
			original := []byte("existing SQLite database")
			if err := os.WriteFile(database, original, 0o600); err != nil {
				t.Fatal(err)
			}
			outside := filepath.Join(t.TempDir(), "private")
			if err := os.WriteFile(outside, []byte("outside bytes"), 0o600); err != nil {
				t.Fatal(err)
			}
			link := filepath.Join(profile, filepath.FromSlash(linked))
			if err := os.MkdirAll(filepath.Dir(link), 0o700); err != nil {
				t.Fatal(err)
			}
			if err := os.Symlink(outside, link); err != nil {
				t.Fatal(err)
			}
			opened, err := Open(t.Context(), profile, protocol.NodeID{})
			if opened != nil {
				_ = opened.Close()
			}
			if err == nil {
				t.Fatal("incompatible storage was accepted")
			}
			if got, err := os.ReadFile(database); err != nil || !bytes.Equal(got, original) {
				t.Fatalf("existing database changed: %v", err)
			}
			if target, err := os.Readlink(link); err != nil || target != outside {
				t.Fatalf("linked resource changed: %q, %v", target, err)
			}
			if got, err := os.ReadFile(outside); err != nil || string(got) != "outside bytes" {
				t.Fatalf("outside resource changed: %v", err)
			}
			for _, name := range []string{"node.turso", "rust-archive"} {
				if _, err := os.Stat(filepath.Join(storage, name)); !errors.Is(err, os.ErrNotExist) {
					t.Fatalf("unexpected startup output %s: %v", name, err)
				}
			}
		})
	}
}
