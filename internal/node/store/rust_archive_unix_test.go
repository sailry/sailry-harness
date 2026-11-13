//go:build unix

package store

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRustArchiveRefusesLinkedResources(t *testing.T) {
	t.Parallel()
	for _, linked := range []string{"plugins/packages/private", "attachments"} {
		t.Run(linked, func(t *testing.T) {
			t.Parallel()
			profile := t.TempDir()
			storage := filepath.Join(profile, "storage")
			if err := core.PrivateDirectory(storage); err != nil {
				t.Fatal(err)
			}
			database := filepath.Join(storage, "node.turso")
			if err := os.WriteFile(database, []byte("opaque database"), 0o600); err != nil {
				t.Fatal(err)
			}
			packages := filepath.Join(profile, "plugins", "packages", strings.Repeat("a", 64))
			if err := os.MkdirAll(packages, 0o700); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(packages, "main.js"), []byte("resource"), 0o600); err != nil {
				t.Fatal(err)
			}
			outside := filepath.Join(t.TempDir(), "private")
			if err := os.WriteFile(outside, []byte("outside bytes"), 0o600); err != nil {
				t.Fatal(err)
			}
			if err := os.Symlink(outside, filepath.Join(profile, filepath.FromSlash(linked))); err != nil {
				t.Fatal(err)
			}
			opened, err := Open(t.Context(), profile, protocol.NodeID{})
			if opened != nil {
				_ = opened.Close()
			}
			if err == nil {
				t.Fatal("a linked resource was copied into the archive")
			}
			if body, err := os.ReadFile(database); err != nil || string(body) != "opaque database" {
				t.Fatalf("a failed resource copy moved its database: %q, %v", body, err)
			}
			if _, err := os.Stat(filepath.Join(storage, RustArchive, "node.turso")); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("the database was archived before its resources: %v", err)
			}
			if _, err := os.Stat(filepath.Join(storage, "node.sqlite")); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("a failed archive created current storage: %v", err)
			}
			if _, err := os.Lstat(filepath.Join(storage, RustArchive, filepath.FromSlash(linked))); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("the archive published an outside resource: %v", err)
			}
			if linked == "attachments" {
				// The package copy completed before the later resource failed.
				// A repaired retry keeps that snapshot and publishes a new set.
				if err := os.Remove(filepath.Join(profile, linked)); err != nil {
					t.Fatal(err)
				}
				if err := os.WriteFile(filepath.Join(packages, "main.js"), []byte("current resource"), 0o600); err != nil {
					t.Fatal(err)
				}
				opened, err := Open(t.Context(), profile, protocol.NodeID{})
				if err != nil {
					t.Fatalf("a repaired archive did not open: %v", err)
				}
				if err := opened.Close(); err != nil {
					t.Fatal(err)
				}
				for archive, expected := range map[string]string{RustArchive: "resource", RustArchive + "-2": "current resource"} {
					path := filepath.Join(storage, archive, "plugins", "packages", strings.Repeat("a", 64), "main.js")
					if body, err := os.ReadFile(path); err != nil || string(body) != expected {
						t.Fatalf("the retried archive replaced resources: %q, %v", body, err)
					}
				}
				if body, err := os.ReadFile(filepath.Join(storage, RustArchive+"-2", "node.turso")); err != nil || string(body) != "opaque database" {
					t.Fatalf("the repaired set lost its database: %q, %v", body, err)
				}
			}
		})
	}
}
