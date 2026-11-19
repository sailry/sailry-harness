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

func TestStartupPreservesInactiveStorage(t *testing.T) {
	t.Parallel()
	for _, active := range []bool{false, true} {
		name := "new database leaves preserved original alone"
		if active {
			name = "active database leaves preserved original alone"
		}
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			profile := t.TempDir()
			storage := filepath.Join(profile, "storage")
			if err := core.PrivateDirectory(storage); err != nil {
				t.Fatal(err)
			}
			node := protocol.NodeID{1}
			if active {
				opened, err := Open(t.Context(), profile, node)
				if err != nil {
					t.Fatal(err)
				}
				if err := opened.Close(); err != nil {
					t.Fatal(err)
				}
			}
			files := map[string]string{
				"storage/rust-archive/node.turso":                          "preserved original database",
				"storage/rust-archive/knowledge.turso":                     "preserved original knowledge",
				"plugins/packages/" + strings.Repeat("a", 64) + "/main.js": "export default {}",
				"attachments/" + protocol.NewAttachmentID().String():       "attachment bytes",
			}
			for name, body := range files {
				path := filepath.Join(profile, filepath.FromSlash(name))
				if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
					t.Fatal(err)
				}
				if err := os.WriteFile(path, []byte(body), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			opened, err := Open(t.Context(), profile, node)
			if err != nil {
				t.Fatal(err)
			}
			if err := opened.Close(); err != nil {
				t.Fatal(err)
			}
			for name, body := range files {
				got, err := os.ReadFile(filepath.Join(profile, filepath.FromSlash(name)))
				if err != nil || !bytes.Equal(got, []byte(body)) {
					t.Fatalf("preserved file %s changed: %v", name, err)
				}
			}
			if _, err := os.Stat(filepath.Join(storage, "rust-archive-2")); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("startup moved storage to a new archive: %v", err)
			}
		})
	}
}
