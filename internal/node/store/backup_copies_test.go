package store

import (
	"os"
	"path/filepath"
	"slices"
	"testing"
)

func TestEarlierUpgradeCopiesAreDropped(t *testing.T) {
	t.Parallel()
	backups := t.TempDir()
	for _, name := range []string{beforeUpgradePrefix + "old", beforeUpgradePrefix + "older", beforeUpgradePrefix + "new", "archive", ".upgrade-scratch"} {
		if err := os.MkdirAll(filepath.Join(backups, name), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(backups, name, "node.turso"), []byte(name), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	// A file that only shares the prefix is not a copy.
	if err := os.WriteFile(filepath.Join(backups, beforeUpgradePrefix+"note"), nil, 0o600); err != nil {
		t.Fatal(err)
	}
	dropEarlierUpgradeCopies(backups, beforeUpgradePrefix+"new")
	entries, err := os.ReadDir(backups)
	if err != nil {
		t.Fatal(err)
	}
	var names []string
	for _, entry := range entries {
		names = append(names, entry.Name())
	}
	want := []string{".upgrade-scratch", "archive", beforeUpgradePrefix + "new", beforeUpgradePrefix + "note"}
	if !slices.Equal(names, want) {
		t.Fatalf("entries = %q, want %q", names, want)
	}
	if body, err := os.ReadFile(filepath.Join(backups, beforeUpgradePrefix+"new", "node.turso")); err != nil || string(body) != beforeUpgradePrefix+"new" {
		t.Fatalf("kept copy = %q, %v", body, err)
	}
}
