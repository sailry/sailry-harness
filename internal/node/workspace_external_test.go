package node

import (
	"os"
	"path/filepath"
	"testing"
)

func TestCanonicalPathKeepsTheNamedLink(t *testing.T) {
	t.Parallel()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	target := filepath.Join(root, "target.txt")
	if err := os.WriteFile(target, []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	directory := filepath.Join(root, "real")
	if err := os.Mkdir(directory, 0o700); err != nil {
		t.Fatal(err)
	}
	for name, to := range map[string]string{"link.txt": target, "linked": directory} {
		if err := os.Symlink(to, filepath.Join(root, name)); err != nil {
			t.Skip("symbolic links unavailable:", err)
		}
	}
	for path, want := range map[string]string{
		// The named entry stays the link, so the file service rejects it.
		filepath.Join(root, "link.txt"): filepath.Join(root, "link.txt"),
		filepath.Join(root, "linked"):   filepath.Join(root, "linked"),
		// A link among the ancestors resolves, as /var and /tmp on macOS.
		filepath.Join(root, "linked", "new", "file.txt"): filepath.Join(directory, "new", "file.txt"),
		"relative/path": "relative/path",
	} {
		if got := canonicalPath(path); got != want {
			t.Errorf("canonicalPath(%q) = %q, want %q", path, got, want)
		}
	}
}
