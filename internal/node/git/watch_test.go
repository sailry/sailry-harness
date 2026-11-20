package git

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	gogit "github.com/go-git/go-git/v5"
)

func writeWatchRule(t testing.TB, root, name, content string) {
	t.Helper()
	path := filepath.Join(root, filepath.FromSlash(name))
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(content), 0600); err != nil {
		t.Fatal(err)
	}
}

func TestWatchFilterDoesNotVisitUnrelatedSubtrees(t *testing.T) {
	root, _, _ := fixture(t)
	writeWatchRule(t, root, "unrelated/hidden/.gitignore", "*.o\n")
	writeWatchRule(t, root, "active/.gitignore", "*.tmp\n")
	hidden := filepath.Join(root, "unrelated", "hidden")
	if err := os.Chmod(hidden, 0); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chmod(hidden, 0700) })
	if _, err := os.ReadDir(hidden); !errors.Is(err, os.ErrPermission) {
		t.Skip("fixture requires a directory this account cannot enumerate")
	}
	f, err := NewWatchFilter(root)
	if err != nil {
		t.Fatalf("opening a watch inspected an unrelated directory: %v", err)
	}
	if !f.Changed([]string{filepath.Join(root, ".git", "HEAD")}) || f.Changed([]string{filepath.Join(root, "active", "generated.tmp")}) {
		t.Fatal("unrelated inaccessible directory affected the active path")
	}
}

func TestWatchFilterMetadataAndIgnoreInvalidation(t *testing.T) {
	root, repository, _ := fixture(t)
	writeWatchRule(t, root, ".git/info/exclude", "private\n")
	writeWatchRule(t, root, ".gitignore", "*.tmp\nfile.txt\nblocked/\n")
	writeWatchRule(t, root, "src/.gitignore", "!keep.tmp\n/child\n")
	writeWatchRule(t, root, "blocked/.gitignore", "!keep.tmp\n")
	f, err := NewWatchFilter(root)
	if err != nil {
		t.Fatal(err)
	}
	check := func(name string, want bool) {
		t.Helper()
		if got := f.Changed([]string{filepath.Join(root, filepath.FromSlash(name))}); got != want {
			t.Fatalf("Changed(%q) = %v, want %v", name, got, want)
		}
	}
	check("src/keep.tmp", true)
	check("src/child", false)
	check("src/deeper/child", true)
	check("other/keep.tmp", false)
	check("private", false)
	check("blocked/keep.tmp", false)
	check("file.txt", true) // tracked despite the ignore pattern

	// Refs notify the repository, but do not reload an unchanged index or
	// rules. A mixed batch must invalidate both, even if the ref comes first.
	index, err := repository.Storer.Index()
	if err != nil {
		t.Fatal(err)
	}
	index.Entries = nil
	if err := repository.Storer.SetIndex(index); err != nil {
		t.Fatal(err)
	}
	check(".git/HEAD", true)
	check("file.txt", true)
	writeWatchRule(t, root, "src/.gitignore", "other\n")
	batch := []string{filepath.Join(root, ".git", "refs", "heads", "main"), filepath.Join(root, ".git", "index"), filepath.Join(root, "src", ".gitignore")}
	if !f.ReferencesChanged(batch) || !f.Changed(batch) {
		t.Fatal("mixed batch failed to notify references and repository")
	}
	check("file.txt", false)
	check("src/keep.tmp", false)
	check("src/other", false)
	writeWatchRule(t, root, "src/.gitignore", "")
	check("src", true) // a replacement directory may not name its ignore file
	check("src/other", true)
	writeWatchRule(t, root, ".git/info/exclude", "confidential\n")
	check(".git/info/exclude", true)
	check("private", true)
	check("confidential", false)
	writeWatchRule(t, root, ".gitignore", "coarse\n")
	if !f.Changed(nil) {
		t.Fatal("unknown batch was filtered out")
	}
	check("coarse", false)
}

// Compare the same fixed ref/index/content burst as unrelated worktree
// breadth grows. Fixture creation is outside the measured loop.
func BenchmarkWatchFilterMetadataBurst(b *testing.B) {
	for _, width := range []int{1, 96} {
		b.Run(fmt.Sprintf("siblings_%d", width), func(b *testing.B) {
			home := b.TempDir()
			b.Setenv("HOME", home)
			b.Setenv("USERPROFILE", home)
			root, err := filepath.EvalSymlinks(b.TempDir())
			if err != nil {
				b.Fatal(err)
			}
			if _, err := gogit.PlainInit(root, false); err != nil {
				b.Fatal(err)
			}
			writeWatchRule(b, root, "active/.gitignore", "*.tmp\n")
			for i := range width {
				writeWatchRule(b, root, fmt.Sprintf("unrelated/%03d/nested/.gitignore", i), "*.o\n")
			}
			filter, err := NewWatchFilter(root)
			if err != nil {
				b.Fatal(err)
			}
			ref := []string{filepath.Join(root, ".git", "refs", "heads", "main")}
			index := []string{filepath.Join(root, ".git", "index")}
			content := []string{filepath.Join(root, "active", "generated.tmp")}
			b.ReportAllocs()
			for b.Loop() {
				if !filter.Changed(ref) || !filter.Changed(index) || filter.Changed(content) {
					b.Fatal("incorrect metadata or ignored content notification")
				}
			}
		})
	}
}
