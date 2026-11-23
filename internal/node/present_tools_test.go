package node

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
)

// Presented files carry their worktree path, media type, size and revision,
// once each; a missing, escaping or directory path refuses the whole call.
func TestPresentFiles(t *testing.T) {
	t.Parallel()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	for name, body := range map[string]string{"report.docx": "docx", "out/page.html": "<p>page</p>", "data.unknownext": "x"} {
		filename := filepath.Join(root, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(filename), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filename, []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	files, err := presentFiles(t.Context(), root, []string{"./report.docx", "out/page.html", "report.docx", " data.unknownext "})
	if err != nil {
		t.Fatal(err)
	}
	confined, err := core.OpenFiles(root, core.RestrictedFiles)
	if err != nil {
		t.Fatal(err)
	}
	defer confined.Close()
	want := []struct {
		path, mime string
		size       uint64
	}{
		{"report.docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document", 4},
		{"out/page.html", "text/html", 11},
		{"data.unknownext", "application/octet-stream", 1},
	}
	if len(files) != len(want) {
		t.Fatalf("presented %+v", files)
	}
	for index, file := range files {
		revision, err := confined.SourceRevision(want[index].path)
		if err != nil {
			t.Fatal(err)
		}
		if file.Path != want[index].path || file.Mime != want[index].mime || file.Size == nil || *file.Size != want[index].size || file.Revision == nil || *file.Revision != revision {
			t.Fatalf("file %d is %+v, want %+v", index, file, want[index])
		}
	}
	for _, paths := range [][]string{{"report.docx", "missing.pdf"}, {"../outside.txt"}, {"out"}, {"/etc/hosts"}} {
		files, err := presentFiles(t.Context(), root, paths)
		if err == nil || files != nil {
			t.Fatalf("%v presented %+v, %v", paths, files, err)
		}
	}
}
