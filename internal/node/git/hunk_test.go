package git

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func testManualHunks(t *testing.T) {
	ctx := context.Background()
	root, r, _ := fixture(t)
	lines := make([]string, 24)
	for index := range lines {
		lines[index] = fmt.Sprintf("line-%02d", index)
	}
	original := strings.Join(lines, "\n") + "\n"
	write := func(text string) {
		t.Helper()
		if err := os.WriteFile(filepath.Join(root, "file.txt"), []byte(text), 0644); err != nil {
			t.Fatal(err)
		}
	}
	write(original)
	w, err := r.Worktree()
	if err != nil {
		t.Fatal(err)
	}
	if _, err = w.Add("file.txt"); err != nil {
		t.Fatal(err)
	}
	commit, err := w.Commit("Separated hunk fixture", &gogit.CommitOptions{Author: &object.Signature{Name: "Fixture", Email: "fixture@example.test", When: time.Unix(101, 0)}})
	if err != nil {
		t.Fatal(err)
	}
	modified := strings.Replace(strings.Replace(original, "line-02", "first Ω", 1), "line-21", "second λ", 1)
	write(modified)
	s := fixtureService()
	capture := func() protocol.CommandApplyGitHunkPayload {
		t.Helper()
		patch, err := s.Diff(ctx, root, "file.txt", protocol.GitDiffScopeUnstaged)
		if err != nil {
			t.Fatal(err)
		}
		content, err := os.ReadFile(filepath.Join(root, "file.txt"))
		if err != nil {
			t.Fatal(err)
		}
		index, err := indexRevision(root, r)
		if err != nil {
			t.Fatal(err)
		}
		bodyHash, diffHash := blake3.Sum256(content), blake3.Sum256([]byte(patch.Text))
		return protocol.CommandApplyGitHunkPayload{Worktree: protocol.NewWorktreeID(), Path: "file.txt", Scope: protocol.GitDiffScopeUnstaged, Operation: protocol.GitHunkStage, DiffRevision: fmt.Sprintf("%x", diffHash), ExpectedRevision: fmt.Sprintf("%x", bodyHash), ExpectedHead: ptr(commit.String()), ExpectedBranch: ptr("main"), ExpectedIndex: index}
	}
	input := capture()
	result, err := s.ApplyHunk(ctx, root, input)
	if err != nil || result.Index == nil {
		t.Fatalf("stage result=%#v err=%v", result, err)
	}
	staged, err := indexContent(r, "file.txt", protocol.MAX_EDIT_BYTES)
	if err != nil || !strings.Contains(string(staged.data), "first Ω") || strings.Contains(string(staged.data), "second λ") {
		t.Fatalf("staged selection=%q err=%v", staged.data, err)
	}
	body, err := os.ReadFile(filepath.Join(root, "file.txt"))
	if err != nil || string(body) != modified {
		t.Fatal("hunk staging changed the working file")
	}
	_, err = s.ApplyHunk(ctx, root, input)
	requireFault(t, err, protocol.ErrorCodeRevisionConflict)
	reject := capture()
	reject.Operation = protocol.GitHunkReject
	result, err = s.ApplyHunk(ctx, root, reject)
	if err != nil || result.Index != nil || result.Revision == reject.ExpectedRevision {
		t.Fatalf("reject result=%#v err=%v", result, err)
	}
	want := strings.Replace(original, "line-02", "first Ω", 1)
	body, err = os.ReadFile(filepath.Join(root, "file.txt"))
	if err != nil || string(body) != want {
		t.Fatalf("reject lost unrelated staged change: %q err=%v", body, err)
	}
	write(modified)
	stale := capture()
	stale.DiffRevision = strings.Repeat("0", 64)
	_, err = s.ApplyHunk(ctx, root, stale)
	requireFault(t, err, protocol.ErrorCodeRevisionConflict)
	stale = capture()
	write("external replacement\n")
	_, err = s.ApplyHunk(ctx, root, stale)
	requireFault(t, err, protocol.ErrorCodeRevisionConflict)
	write("binary\x00file")
	binary := capture()
	_, err = s.ApplyHunk(ctx, root, binary)
	requireFault(t, err, protocol.ErrorCodeInvalidRequest)
}
