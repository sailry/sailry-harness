package node

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
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

func TestExternalFilePathProjection(t *testing.T) {
	t.Parallel()
	root := t.TempDir()
	scope := externalFileScope{root: root}
	name := "example.txt"
	abs := filepath.ToSlash(filepath.Join(root, name))
	result := scope.result(protocol.Output{Kind: "file_names", Data: protocol.FileNames{
		Paths: []string{name}, Diagnostics: []protocol.SearchDiagnostic{{Path: name}},
	}}).Data.(protocol.FileNames)
	if result.Paths[0] != abs || result.Diagnostics[0].Path != abs {
		t.Fatalf("discovery paths: %+v", result)
	}
	search := scope.result(protocol.Output{Kind: "search_results", Data: protocol.SearchResults{
		Paths: []string{name}, Counts: []protocol.SearchCount{{Path: name}}, Matches: []protocol.SearchMatch{{Path: name}},
	}}).Data.(protocol.SearchResults)
	if search.Paths[0] != abs || search.Counts[0].Path != abs || search.Matches[0].Path != abs {
		t.Fatalf("search paths: %+v", search)
	}
	const metadata = `{"path":"data.csv","integer":9007199254740993}`
	notebook := scope.result(protocol.Output{Kind: "notebook_page", Data: protocol.NotebookPage{Path: name, Metadata: json.RawMessage(metadata)}}).Data.(protocol.NotebookPage)
	if notebook.Path != abs || string(notebook.Metadata) != metadata {
		t.Fatalf("notebook path or metadata changed unexpectedly: %+v", notebook)
	}
	cursor := &protocol.DirectoryCursor{Revision: "0123456789abcdef", Name: name}
	page := scope.result(protocol.Output{Kind: "directory", Data: protocol.Directory{Path: ".", Next: cursor}}).Data.(protocol.Directory)
	if page.Path != filepath.ToSlash(root) || page.Next == nil || *page.Next != *cursor || localPath(root, root) != "" {
		t.Fatalf("root directory or cursor changed unexpectedly: %+v", page)
	}
}

func TestPatchPathRewritePreservesFilenameWhitespace(t *testing.T) {
	t.Parallel()
	const patch = "*** Begin Patch\r\n*** Add File:  leading and trailing \r\n+content\r\n*** End Patch\r\n"
	got, err := patchPaths(patch, func(path string) (string, error) {
		if path != " leading and trailing " {
			t.Fatalf("rewriting received %q", path)
		}
		return path, nil
	})
	if err != nil || got != patch {
		t.Fatalf("rewrite = %q, %v", got, err)
	}
}
