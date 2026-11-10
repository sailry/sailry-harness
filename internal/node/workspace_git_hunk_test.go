package node

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"testing"
	"time"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func testManualGitHunkAdmission(t *testing.T) {
	node := nodeFixture(t)
	session := sessionFixture(t, node)
	write := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "write_file", Data: protocol.CommandWriteFilePayload{Worktree: session.Worktree, Path: "source.txt", Text: "before\n"}}))
	if write.Fault != nil {
		t.Fatal(write.Fault)
	}
	root := ""
	for _, tree := range snapshotFixture(t, node).Worktrees {
		if tree.ID == session.Worktree {
			root = tree.Path
		}
	}
	if root == "" {
		t.Fatal("captured worktree missing")
	}
	r, err := gogit.PlainInitWithOptions(root, &gogit.PlainInitOptions{InitOptions: gogit.InitOptions{DefaultBranch: plumbing.NewBranchReferenceName("main")}})
	if err != nil {
		t.Fatal(err)
	}
	w, err := r.Worktree()
	if err != nil {
		t.Fatal(err)
	}
	if _, err = w.Add("source.txt"); err != nil {
		t.Fatal(err)
	}
	commit, err := w.Commit("Initial hunk fixture", &gogit.CommitOptions{Author: &object.Signature{Name: "Fixture", Email: "fixture@example.test", When: time.Unix(100, 0)}})
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(root, "source.txt"), []byte("after\n"), 0600); err != nil {
		t.Fatal(err)
	}
	diff := commandFixture(t, node, protocol.Command{Kind: "read_git_diff", Data: protocol.CommandReadGitDiffPayload{Worktree: session.Worktree, Path: "source.txt", Scope: protocol.GitDiffScopeUnstaged}}).Data.(protocol.GitDiff)
	content := commandFixture(t, node, protocol.Command{Kind: "read_file", Data: protocol.CommandReadFilePayload{Worktree: session.Worktree, Path: "source.txt"}}).Data.(protocol.FileContent)
	index, err := os.ReadFile(filepath.Join(root, ".git", "index"))
	if err != nil {
		t.Fatal(err)
	}
	indexHash, diffHash := blake3.Sum256(index), blake3.Sum256([]byte(diff.Text))
	head, branch := commit.String(), "main"
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "apply_git_hunk", Data: protocol.CommandApplyGitHunkPayload{Worktree: session.Worktree, Path: "source.txt", Scope: protocol.GitDiffScopeUnstaged, Operation: protocol.GitHunkStage, DiffRevision: fmt.Sprintf("%x", diffHash), ExpectedRevision: *content.Revision, ExpectedIndex: fmt.Sprintf("%x", indexHash), ExpectedHead: &head, ExpectedBranch: &branch}})
	first := dispatchFixture(t, node, node.ID(), request)
	if first.Fault != nil || first.Output.Kind != "git_hunk_applied" {
		t.Fatalf("manual hunk dispatch=%#v", first)
	}
	confirmed, err := os.ReadFile(filepath.Join(root, ".git", "index"))
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(root, "source.txt"), []byte("external\n"), 0600); err != nil {
		t.Fatal(err)
	}
	replayed := dispatchFixture(t, node, node.ID(), request)
	before, _ := json.Marshal(first)
	after, _ := json.Marshal(replayed)
	if !bytes.Equal(before, after) {
		t.Fatalf("manual hunk receipt changed: %s %s", before, after)
	}
	current, err := os.ReadFile(filepath.Join(root, ".git", "index"))
	if err != nil || !bytes.Equal(confirmed, current) {
		t.Fatal("manual hunk receipt replayed the index effect")
	}
	body, err := os.ReadFile(filepath.Join(root, "source.txt"))
	if err != nil || string(body) != "external\n" {
		t.Fatal("manual hunk receipt changed external content")
	}
}
