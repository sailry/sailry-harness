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

func TestRustArchiveResources(t *testing.T) {
	t.Parallel()
	for _, current := range []bool{false, true} {
		name := "fresh storage"
		if current {
			name = "current storage"
		}
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			profile := t.TempDir()
			storage := filepath.Join(profile, "storage")
			if err := core.PrivateDirectory(storage); err != nil {
				t.Fatal(err)
			}
			open := func() {
				t.Helper()
				opened, err := Open(t.Context(), profile, protocol.NodeID{})
				if err != nil {
					t.Fatal(err)
				}
				if err := opened.Close(); err != nil {
					t.Fatal(err)
				}
			}
			if current {
				open()
			}
			oldDatabase := filepath.Join(storage, "node.turso")
			if err := os.WriteFile(oldDatabase, []byte("opaque database"), 0o600); err != nil {
				t.Fatal(err)
			}
			resources := map[string]string{
				"plugins/packages/" + strings.Repeat("a", 64) + "/main.js": "export default {}",
				"attachments/" + protocol.NewAttachmentID().String():       "attachment bytes",
			}
			modes := map[string]os.FileMode{}
			for name, body := range resources {
				path := filepath.Join(profile, filepath.FromSlash(name))
				if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
					t.Fatal(err)
				}
				if err := os.WriteFile(path, []byte(body), 0o700); err != nil {
					t.Fatal(err)
				}
				info, err := os.Stat(path)
				if err != nil {
					t.Fatal(err)
				}
				modes[name] = info.Mode()
			}
			check := func(base, name, expected string) {
				t.Helper()
				path := filepath.Join(base, filepath.FromSlash(name))
				body, err := os.ReadFile(path)
				if err != nil || string(body) != expected {
					t.Fatalf("resource %s changed: %q, %v", path, body, err)
				}
				info, err := os.Stat(path)
				if err != nil || info.Mode() != modes[name] {
					t.Fatalf("resource %s lost its permissions: %v, %v", path, info, err)
				}
			}
			open()
			if _, err := os.Lstat(oldDatabase); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("the old database stayed live: %v", err)
			}
			archive := filepath.Join(storage, RustArchive)
			if body, err := os.ReadFile(filepath.Join(archive, "node.turso")); err != nil || string(body) != "opaque database" {
				t.Fatalf("the archived database changed: %q, %v", body, err)
			}
			for name, body := range resources {
				check(archive, name, body)
				// Existing Go references keep their live files. Later changes
				// to those files must not change the kept archive's bytes.
				check(profile, name, body)
				if err := os.WriteFile(filepath.Join(profile, filepath.FromSlash(name)), []byte("current "+body), modes[name].Perm()); err != nil {
					t.Fatal(err)
				}
			}
			open()
			if _, err := os.Stat(filepath.Join(storage, RustArchive+"-2")); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("an ordinary restart archived resources again: %v", err)
			}
			for name, body := range resources {
				check(archive, name, body)
				check(profile, name, "current "+body)
			}
			// A later foreign database gets its own matching resource
			// snapshot without replacing either the live files or the first.
			if err := os.WriteFile(oldDatabase, []byte("later opaque database"), 0o600); err != nil {
				t.Fatal(err)
			}
			open()
			for name, body := range resources {
				check(archive, name, body)
				check(filepath.Join(storage, RustArchive+"-2"), name, "current "+body)
				check(profile, name, "current "+body)
			}
		})
	}
}
