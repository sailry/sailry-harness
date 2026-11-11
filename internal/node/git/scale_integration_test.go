//go:build integration

package git

import (
	"bytes"
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// These scenarios use repository sizes on which status, change review, staging
// and branch listing once exceeded the read deadline. The service reports Busy
// when it runs out of time, so a result without a fault is the assertion; the
// logged durations are for comparison only.
func TestLargeRepositoryIntegration(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("Git executable is required")
	}
	ctx := context.Background()
	home := t.TempDir()
	env := []string{}
	for _, pair := range os.Environ() {
		key, _, _ := strings.Cut(pair, "=")
		if key != "HOME" && key != "USERPROFILE" && key != "XDG_CONFIG_HOME" {
			env = append(env, pair)
		}
	}
	env = append(env, "HOME="+home, "USERPROFILE="+home, "XDG_CONFIG_HOME="+filepath.Join(home, ".config"),
		"GIT_AUTHOR_NAME=Fixture", "GIT_AUTHOR_EMAIL=fixture@example.test", "GIT_COMMITTER_NAME=Fixture", "GIT_COMMITTER_EMAIL=fixture@example.test")
	run := func(t *testing.T, root string, input []byte, args ...string) string {
		t.Helper()
		command := exec.Command("git", args...)
		command.Dir, command.Env, command.Stdin = root, env, bytes.NewReader(input)
		out, err := command.CombinedOutput()
		if err != nil {
			t.Fatalf("git %v: %v\n%s", args, err, out)
		}
		return string(out)
	}
	repository := func(t *testing.T) string {
		t.Helper()
		root, err := filepath.EvalSymlinks(t.TempDir())
		if err != nil {
			t.Fatal(err)
		}
		run(t, root, nil, "init", "-q", "-b", "main")
		return root
	}
	s := New(Options{Environment: env})
	t.Run("status_changes_and_staging_with_20000_tracked_files", func(t *testing.T) {
		const tracked, changed, added = 20000, 500, 100
		root := repository(t)
		name := func(prefix string, index int) string { return fmt.Sprintf("d%d/%s%d.txt", index%200, prefix, index) }
		write := func(path string, body []byte) {
			t.Helper()
			target := filepath.Join(root, filepath.FromSlash(path))
			if err := os.MkdirAll(filepath.Dir(target), 0755); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(target, body, 0644); err != nil {
				t.Fatal(err)
			}
		}
		write(".gitignore", []byte("*.log\n"))
		for index := range tracked {
			write(name("f", index), bytes.Repeat([]byte("line of text\n"), 200))
		}
		run(t, root, nil, "add", "-A")
		run(t, root, nil, "commit", "-q", "-m", "base")
		for index := range changed {
			write(name("f", index), bytes.Repeat([]byte("changed line\n"), 210))
		}

		started := time.Now()
		status, err := s.Status(ctx, root)
		t.Logf("status with %d changed of %d tracked files: %s", changed, tracked, time.Since(started))
		if err != nil || len(status.Entries) != changed || status.Truncated {
			t.Fatal(len(status.Entries), status.Truncated, err)
		}
		want := protocol.GitLineStats{Additions: 210, Deletions: 200}
		for _, entry := range status.Entries {
			if entry.Unstaged == nil || entry.UnstagedDiff != want || entry.Diff != want || entry.StagedDiff != (protocol.GitLineStats{}) {
				t.Fatalf("%+v", entry)
			}
		}

		r, err := open(root)
		if err != nil {
			t.Fatal(err)
		}
		idx, err := r.Storer.Index()
		if err != nil {
			t.Fatal(err)
		}
		untouched := name("f", tracked-1)
		entry, err := idx.Entry(untouched)
		if err != nil {
			t.Fatal(err)
		}
		info, err := os.Lstat(filepath.Join(root, filepath.FromSlash(untouched)))
		if err != nil || !indexStat(entry, info) {
			t.Fatal("stat data recorded by Git does not match an untouched file", err)
		}
		target := protocol.GitChangeTarget{Kind: "worktree", Data: protocol.GitChangeTargetWorktreePayload{Scope: protocol.GitDiffScopeAll}}
		started = time.Now()
		page, err := s.Changes(ctx, root, protocol.CommandListGitChangesPayload{Target: &target, Limit: 500})
		t.Logf("change inventory: %s", time.Since(started))
		listed := 0
		for {
			if err != nil {
				t.Fatal(err)
			}
			listed += len(page.Entries)
			if page.Next == nil {
				break
			}
			page, err = s.Changes(ctx, root, protocol.CommandListGitChangesPayload{Cursor: page.Next, Limit: 500})
		}
		if listed != changed || !page.Exhausted {
			t.Fatal(listed, page.Exhausted)
		}

		paths := []string{}
		for index := range added {
			paths = append(paths, name("new", index))
			write(paths[index], []byte("new\n"))
		}
		revision, err := indexRevision(root, r)
		if err != nil {
			t.Fatal(err)
		}
		started = time.Now()
		_, err = s.UpdateIndex(ctx, root, protocol.CommandUpdateGitIndexPayload{Paths: paths, Operation: protocol.GitIndexChangeStage, ExpectedIndex: revision, ExpectedHead: status.Head})
		t.Logf("staging %d untracked files: %s", added, time.Since(started))
		if err != nil {
			t.Fatal(err)
		}
		if staged := strings.Count(run(t, root, nil, "diff", "--cached", "--name-only", "--diff-filter=A"), "\n"); staged != added {
			t.Fatal(staged)
		}
		write("d0/skipped.log", []byte("log\n"))
		revision, err = indexRevision(root, r)
		if err != nil {
			t.Fatal(err)
		}
		_, err = s.UpdateIndex(ctx, root, protocol.CommandUpdateGitIndexPayload{Paths: []string{"d0/skipped.log"}, Operation: protocol.GitIndexChangeStage, ExpectedIndex: revision, ExpectedHead: status.Head})
		requireFault(t, err, protocol.ErrorCodeInvalidRequest)
	})
	t.Run("ahead_behind_with_150000_commits", func(t *testing.T) {
		const commits = 150000
		root := repository(t)
		var stream bytes.Buffer
		for index := 1; index <= commits; index++ {
			fmt.Fprintf(&stream, "commit refs/heads/main\nmark :%d\ncommitter Fixture <fixture@example.test> %d +0000\ndata 2\nc\n", index, 1700000000+index)
			if index > 1 {
				fmt.Fprintf(&stream, "from :%d\n", index-1)
			}
			stream.WriteString("M 100644 inline f\ndata 2\nx\n\n")
		}
		run(t, root, stream.Bytes(), "fast-import", "--quiet")
		run(t, root, nil, "reset", "-q", "--hard", "main")
		run(t, root, nil, "config", "remote.origin.url", filepath.Join(root, "missing"))
		run(t, root, nil, "config", "remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*")
		run(t, root, nil, "config", "branch.main.remote", "origin")
		run(t, root, nil, "config", "branch.main.merge", "refs/heads/main")
		diverged := strings.TrimSpace(run(t, root, nil, "commit-tree", "HEAD~3^{tree}", "-p", "HEAD~3", "-m", "upstream"))
		for upstream, want := range map[string][2]uint64{"HEAD~1": {1, 0}, diverged: {3, 1}, "HEAD": {0, 0}} {
			run(t, root, nil, "update-ref", "refs/remotes/origin/main", upstream)
			started := time.Now()
			branches, err := s.Branches(ctx, root)
			t.Logf("branches with %d commits and upstream %s: %s", commits, upstream, time.Since(started))
			if err != nil || branches.Sync.Ahead != want[0] || branches.Sync.Behind != want[1] {
				t.Fatal(upstream, branches.Sync.Ahead, branches.Sync.Behind, err)
			}
		}
	})
}
