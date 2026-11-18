package git

import (
	"context"
	"os"
	"path/filepath"
	"slices"
	"testing"
	"time"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestLogFiltersLiteralPaths(t *testing.T) {
	root, repository, initial := fixture(t)
	worktree, err := repository.Worktree()
	if err != nil {
		t.Fatal(err)
	}
	write := func(path, text string) {
		t.Helper()
		target := filepath.Join(root, filepath.FromSlash(path))
		if err := os.MkdirAll(filepath.Dir(target), 0755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(target, []byte(text), 0644); err != nil {
			t.Fatal(err)
		}
		if _, err := worktree.Add(path); err != nil {
			t.Fatal(err)
		}
	}
	commit := func(message string, timestamp int64) plumbing.Hash {
		t.Helper()
		hash, err := worktree.Commit(message, &gogit.CommitOptions{
			Author: &object.Signature{Name: "Fixture", Email: "fixture@example.test", When: time.Unix(timestamp, 0)},
		})
		if err != nil {
			t.Fatal(err)
		}
		return hash
	}
	write("src/nested/file.txt", "first")
	add := commit("Add directory", 200)
	write("src-sibling/file.txt", "unrelated")
	commit("Add similar prefix", 300)
	write("src/nested/file.txt", "second")
	update := commit("Modify descendant", 400)
	if _, err := worktree.Remove("src/nested/file.txt"); err != nil {
		t.Fatal(err)
	}
	remove := commit("Remove descendant", 500)

	ctx := context.Background()
	for _, path := range []string{"src", "src/nested/file.txt"} {
		service := fixtureService()
		first, err := service.Log(ctx, root, protocol.CommandReadGitLogPayload{Path: &path, Limit: 2})
		if err != nil || first.Next == nil || len(first.Entries) != 2 ||
			first.Entries[0].ID != remove.String() || first.Entries[1].ID != update.String() {
			t.Fatalf("%s first page: %+v, %v", path, first, err)
		}
		second, err := service.Log(ctx, root, protocol.CommandReadGitLogPayload{Path: &path, Limit: 2, Cursor: first.Next})
		if err != nil || second.Next != nil || len(second.Entries) != 1 || second.Entries[0].ID != add.String() {
			t.Fatalf("%s next page: %+v, %v", path, second, err)
		}
	}
	// A merge with one unchanged parent should not attribute the other
	// branch's changes to the merge itself.
	removed, err := repository.CommitObject(remove)
	if err != nil {
		t.Fatal(err)
	}
	merged := *removed
	merged.Message = "Merge unchanged directory"
	merged.Committer.When = time.Unix(600, 0)
	merged.ParentHashes = []plumbing.Hash{remove, add}
	encoded := repository.Storer.NewEncodedObject()
	if err := merged.Encode(encoded); err != nil {
		t.Fatal(err)
	}
	merge, err := repository.Storer.SetEncodedObject(encoded)
	if err != nil {
		t.Fatal(err)
	}
	revision := merge.String()
	path := "src"
	page, err := fixtureService().Log(ctx, root, protocol.CommandReadGitLogPayload{Path: &path, Limit: 10, Revision: &revision})
	var ids []string
	for _, entry := range page.Entries {
		ids = append(ids, entry.ID)
	}
	if err != nil || !slices.Equal(ids, []string{remove.String(), update.String(), add.String()}) {
		t.Fatalf("merge history: %v, %v", ids, err)
	}

	path = "file.txt"
	page, err = fixtureService().Log(ctx, root, protocol.CommandReadGitLogPayload{Path: &path, Limit: 10})
	if err != nil || len(page.Entries) != 1 || page.Entries[0].ID != initial {
		t.Fatalf("root commit history: %+v, %v", page, err)
	}
}
