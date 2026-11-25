package process

import (
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestCaptureWorkspaceBoundaryDoesNotPrepareGit(t *testing.T) {
	root := t.TempDir()
	if path, err := CaptureWorkspaceBoundary(root); err != nil || path != "" {
		t.Fatalf("plain workspace boundary: %q %v", path, err)
	}
	if _, err := os.Stat(filepath.Join(root, ".sailry")); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("capture created a managed directory: %v", err)
	}
	if err := os.MkdirAll(filepath.Join(root, ".git", "info"), 0o700); err != nil {
		t.Fatal(err)
	}
	exclude := filepath.Join(root, ".git", "info", "exclude")
	if err := os.WriteFile(exclude, []byte("original\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if path, err := CaptureWorkspaceBoundary(root); err != nil || path != "" {
		t.Fatalf("repository without checkouts: %q %v", path, err)
	}
	protected := filepath.Join(root, ".sailry", "worktrees")
	if err := os.MkdirAll(protected, 0o700); err != nil {
		t.Fatal(err)
	}
	if path, err := CaptureWorkspaceBoundary(root); err != nil || path != protected {
		t.Fatalf("existing checkout boundary: %q %v", path, err)
	}
	if data, err := os.ReadFile(exclude); err != nil || string(data) != "original\n" {
		t.Fatalf("capture changed Git metadata: %q %v", data, err)
	}
}
