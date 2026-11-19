package node

import (
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
)

func TestProfileRequiresOriginalKey(t *testing.T) {
	t.Parallel()
	path := filepath.Join(t.TempDir(), "profile")
	profile, err := OpenProfile(t.Context(), path)
	if err != nil {
		t.Fatal(err)
	}
	identity := profile.Identity.ID()
	database := profile.Store.Node.Path()
	if err := profile.Close(); err != nil {
		t.Fatal(err)
	}
	key := filepath.Join(path, "link.key")
	saved := filepath.Join(t.TempDir(), "link.key")
	if err := os.Rename(key, saved); err != nil {
		t.Fatal(err)
	}
	before, err := os.ReadFile(database)
	if err != nil {
		t.Fatal(err)
	}
	assertRejected := func(reason string) {
		t.Helper()
		opened, err := OpenProfile(t.Context(), path)
		if opened != nil {
			_ = opened.Close()
		}
		if err == nil || !strings.Contains(err.Error(), reason) || !strings.Contains(err.Error(), "restore the original link.key") {
			t.Fatalf("expected recovery instructions for %s: %v", reason, err)
		}
	}
	assertRejected("link.key is missing")
	if _, err := os.Lstat(key); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("startup created a replacement key: %v", err)
	}
	if after, err := os.ReadFile(database); err != nil || !bytes.Equal(before, after) {
		t.Fatalf("startup changed the database without its identity: %v", err)
	}
	other, err := link.OpenIdentity(path)
	if err != nil {
		t.Fatal(err)
	}
	assertRejected("does not match link.key")
	if unchanged, err := link.ReadIdentity(path); err != nil || unchanged.ID() != other.ID() {
		t.Fatalf("startup changed the mismatching key: %v", err)
	}
	if err := os.Remove(key); err != nil {
		t.Fatal(err)
	}
	if err := os.Rename(saved, key); err != nil {
		t.Fatal(err)
	}
	restored, err := OpenProfile(t.Context(), path)
	if err != nil {
		t.Fatal(err)
	}
	defer restored.Close()
	if restored.Identity.ID() != identity || restored.Store.NodeID != identity {
		t.Fatal("restoring the original key did not restore the database identity")
	}
}

func TestIncompleteStorageDoesNotCreateKey(t *testing.T) {
	t.Parallel()
	for _, sample := range []struct{ file, reason string }{
		{"node.turso", "Node database is empty"},
		{"node.db-log", "active storage still contains node.db-log"},
		{"knowledge.turso", "active storage still contains knowledge.turso"},
	} {
		t.Run(sample.file, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "profile")
			storage := filepath.Join(path, "storage")
			if err := os.MkdirAll(storage, 0o700); err != nil {
				t.Fatal(err)
			}
			file := filepath.Join(storage, sample.file)
			if err := os.WriteFile(file, nil, 0o600); err != nil {
				t.Fatal(err)
			}
			opened, err := OpenProfile(t.Context(), path)
			if opened != nil {
				_ = opened.Close()
			}
			if err == nil || !strings.Contains(err.Error(), sample.reason) {
				t.Fatalf("incomplete storage startup: %v", err)
			}
			if _, err := os.Lstat(filepath.Join(path, "link.key")); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("startup created an identity next to incomplete storage: %v", err)
			}
			if info, err := os.Stat(file); err != nil || info.Size() != 0 {
				t.Fatalf("startup changed incomplete storage: %v", err)
			}
		})
	}
}
