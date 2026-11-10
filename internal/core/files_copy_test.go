package core

import (
	"context"
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

func TestConfinedCopyAndMove(t *testing.T) {
	t.Parallel()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	source := filepath.Join(root, "source")
	if err = os.MkdirAll(filepath.Join(source, ".git"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(source, "file.txt"), []byte("content"), 0o640); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(source, ".git", "config"), []byte("private"), 0o600); err != nil {
		t.Fatal(err)
	}
	files, err := OpenFiles(root, ManualFiles)
	if err != nil {
		t.Fatal(err)
	}
	defer files.Close()
	if err = files.CopyEntry(context.Background(), "source", "copied"); err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(filepath.Join(root, "copied", "file.txt"))
	if err != nil || string(data) != "content" {
		t.Fatalf("copy content %q %v", data, err)
	}
	if _, err = os.Stat(filepath.Join(root, "copied", ".git")); !os.IsNotExist(err) {
		t.Fatalf("Git metadata was copied: %v", err)
	}
	if err = files.CopyEntry(context.Background(), "source", "source/nested"); err == nil {
		t.Fatal("directory copied into itself")
	}
	if err = files.MoveEntryTo(context.Background(), files, "copied", "moved"); err != nil {
		t.Fatal(err)
	}
	if _, err = os.Stat(filepath.Join(root, "copied")); !os.IsNotExist(err) {
		t.Fatalf("source remains after rename: %v", err)
	}
	if runtime.GOOS != "windows" {
		outside := t.TempDir()
		if err = os.Symlink(outside, filepath.Join(root, "linked")); err != nil {
			t.Fatal(err)
		}
		if err = files.CopyEntry(context.Background(), "linked", "escaped"); err == nil {
			t.Fatal("symbolic-link copy was accepted")
		}
	}
}
