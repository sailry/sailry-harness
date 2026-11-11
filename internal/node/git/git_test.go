package git

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"sync"
	"testing"
	"time"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func fixture(t *testing.T) (string, *gogit.Repository, string) {
	t.Helper()
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("USERPROFILE", home)
	t.Setenv("XDG_CONFIG_HOME", filepath.Join(home, ".config"))
	root, canonicalErr := filepath.EvalSymlinks(t.TempDir())
	if canonicalErr != nil {
		t.Fatal(canonicalErr)
	}
	r, err := gogit.PlainInitWithOptions(root, &gogit.PlainInitOptions{InitOptions: gogit.InitOptions{DefaultBranch: plumbing.NewBranchReferenceName("main")}})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "file.txt"), []byte("one\ntwo\nthree\n"), 0644); err != nil {
		t.Fatal(err)
	}
	w, err := r.Worktree()
	if err != nil {
		t.Fatal(err)
	}
	if _, err := w.Add("file.txt"); err != nil {
		t.Fatal(err)
	}
	hash, err := w.Commit("Initial commit", &gogit.CommitOptions{Author: &object.Signature{Name: "Fixture", Email: "fixture@example.test", When: time.Unix(100, 0)}})
	if err != nil {
		t.Fatal(err)
	}
	return root, r, hash.String()
}
func fixtureService() *Service {
	return New(Options{Run: func(context.Context, string, []string, []byte) (CommandResult, error) {
		return CommandResult{ExitCode: 0}, nil
	}})
}
func requireFault(t *testing.T, err error, code protocol.ErrorCode) {
	t.Helper()
	var f *protocol.Fault
	if !errors.As(err, &f) || f.Code != code {
		t.Fatalf("fault = %v, want %s", err, code)
	}
}

func TestGitReadMapping(t *testing.T) {
	ctx := context.Background()
	t.Run("status_native_fields_and_bound", func(t *testing.T) {
		body := "# branch.oid abcdef\x00# branch.head main\x001 .M N... 100644 100644 100644 abc def file with space.txt\x00? new.txt\x00u UU N... 100644 100644 100644 100644 a b c conflict.txt\x00"
		got, err := ParseStatus([]byte(body), false)
		if err != nil {
			t.Fatal(err)
		}
		if got.Kind != protocol.RepositoryKindReady || len(got.Entries) != 3 || got.Entries[1].Path != "file with space.txt" || got.Entries[1].Unstaged == nil || !got.Entries[2].Untracked || got.Entries[2].Unstaged != nil {
			t.Fatalf("%+v", got)
		}
		if !got.Entries[0].Conflicted {
			t.Fatal(got.Entries)
		}
		_, err = ParseStatus([]byte(body[:len(body)-1]), false)
		requireFault(t, err, protocol.ErrorCodeUnavailable)
	})
	t.Run("diff_log_show_blame", func(t *testing.T) {
		root, _, commit := fixture(t)
		s := fixtureService()
		if err := os.WriteFile(filepath.Join(root, "file.txt"), []byte("one\nchanged\nthree\n"), 0644); err != nil {
			t.Fatal(err)
		}
		diff, err := s.Diff(ctx, root, "file.txt", protocol.GitDiffScopeUnstaged)
		if err != nil || diff.Additions != 1 || diff.Deletions != 1 || !strings.Contains(diff.Text, "-two\n+changed\n") {
			t.Fatal(diff, err)
		}
		staged, err := s.Diff(ctx, root, "file.txt", protocol.GitDiffScopeStaged)
		if err != nil || staged.Text != "" {
			t.Fatal(staged, err)
		}
		log, err := s.Log(ctx, root, protocol.CommandReadGitLogPayload{Limit: 1})
		if err != nil || len(log.Entries) != 1 || log.Entries[0].ID != commit {
			t.Fatal(log, err)
		}
		show, err := s.Show(ctx, root, protocol.CommandReadGitShowPayload{Revision: commit, Path: ptr("file.txt"), Offset: 1, Limit: 1})
		if err != nil {
			t.Fatal(err)
		}
		file := data[protocol.GitFile](show.Data)
		if file.Text != "2: two\n" || file.Next == nil || *file.Next != 2 {
			t.Fatal(file)
		}
		blame, err := s.Blame(ctx, root, protocol.CommandReadGitBlamePayload{Path: "file.txt", Limit: 2})
		if err != nil || len(blame.Entries) != 2 || blame.Entries[1].OriginalLine != 2 || blame.Entries[0].Email != "fixture@example.test" {
			t.Fatal(blame, err)
		}
	})
	t.Run("changes_snapshot_pages_and_conflicts", func(t *testing.T) {
		root, _, _ := fixture(t)
		s := fixtureService()
		target := protocol.GitChangeTarget{Kind: "worktree", Data: protocol.GitChangeTargetWorktreePayload{Scope: protocol.GitDiffScopeAll}}
		if clean, err := s.Changes(ctx, root, protocol.CommandListGitChangesPayload{Target: &target, Limit: 10}); err != nil || len(clean.Entries) != 0 || !clean.Exhausted {
			t.Fatal("untouched tracked file listed", clean, err)
		}
		if err := os.WriteFile(filepath.Join(root, "file.txt"), []byte("one\nchanged\nthree\n"), 0644); err != nil {
			t.Fatal(err)
		}
		list, err := s.Changes(ctx, root, protocol.CommandListGitChangesPayload{Target: &target, Limit: 10})
		if err != nil || len(list.Entries) != 1 || !list.Exhausted {
			t.Fatal(list, err)
		}
		selected := list.Entries[0]
		change, err := s.Change(ctx, root, protocol.CommandReadGitChangePayload{Snapshot: list.Snapshot, Path: selected.Path, ExpectedMetadataRevision: selected.MetadataRevision, Limit: 2})
		if err != nil || len(change.Lines) != 2 || change.Next == nil {
			t.Fatal(change, err)
		}
		page, err := s.Context(ctx, root, protocol.CommandReadGitContextPayload{Snapshot: list.Snapshot, Path: selected.Path, ExpectedMetadataRevision: selected.MetadataRevision, ExpectedRevision: change.Revision, Side: protocol.GitContextSideNew, Offset: 1, Limit: 1})
		if err != nil || page.Text != "changed\n" {
			t.Fatal(page, err)
		}
		if err := os.WriteFile(filepath.Join(root, "file.txt"), []byte("external replacement\n"), 0644); err != nil {
			t.Fatal(err)
		}
		_, err = s.Change(ctx, root, protocol.CommandReadGitChangePayload{Snapshot: list.Snapshot, Path: selected.Path, ExpectedMetadataRevision: selected.MetadataRevision, Limit: 10})
		requireFault(t, err, protocol.ErrorCodeRevisionConflict)
	})
	t.Run("status_coalesced_and_modified_timestamp", func(t *testing.T) {
		root, _, commit := fixture(t)
		body := "# branch.oid " + commit + "\x00# branch.head main\x001 .M N... 100644 100644 100644 abc def file.txt\x00"
		started, release := make(chan struct{}), make(chan struct{})
		var calls int
		var mu sync.Mutex
		s := New(Options{Run: func(context.Context, string, []string, []byte) (CommandResult, error) {
			mu.Lock()
			calls++
			if calls == 1 {
				close(started)
			}
			mu.Unlock()
			<-release
			return CommandResult{Stdout: []byte(body)}, nil
		}})
		first := make(chan protocol.GitStatus, 1)
		go func() { v, _ := s.Status(ctx, root); first <- v }()
		<-started
		second := make(chan protocol.GitStatus, 1)
		go func() { v, _ := s.Status(ctx, root); second <- v }()
		close(release)
		a, b := <-first, <-second
		if len(a.Entries) != 1 || a.Entries[0].ModifiedMs == nil || !reflect.DeepEqual(a, b) {
			t.Fatal(a, b)
		}
		mu.Lock()
		defer mu.Unlock()
		if calls != 1 {
			t.Fatal(calls)
		}
	})
	t.Run("status_line_statistics_match_each_diff_scope", func(t *testing.T) {
		root, r, commit := fixture(t)
		index, err := indexRevision(root, r)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(root, "file.txt"), []byte("one\nstaged\nthree\n"), 0644); err != nil {
			t.Fatal(err)
		}
		if _, err := fixtureService().UpdateIndex(ctx, root, protocol.CommandUpdateGitIndexPayload{Paths: []string{"file.txt"}, Operation: protocol.GitIndexChangeStage, ExpectedIndex: index, ExpectedHead: &commit}); err != nil {
			t.Fatal(err)
		}
		for name, body := range map[string]string{"file.txt": "one\nstaged\nthree\nfour\nfive\n", "new.txt": "first\nsecond\n"} {
			if err := os.WriteFile(filepath.Join(root, name), []byte(body), 0644); err != nil {
				t.Fatal(err)
			}
		}
		body := "# branch.oid " + commit + "\x00# branch.head main\x001 MM N... 100644 100644 100644 abc def file.txt\x00? new.txt\x00"
		s := New(Options{Run: func(context.Context, string, []string, []byte) (CommandResult, error) {
			return CommandResult{Stdout: []byte(body)}, nil
		}})
		status, err := s.Status(ctx, root)
		if err != nil || len(status.Entries) != 2 {
			t.Fatal(status, err)
		}
		want := map[string][3]protocol.GitLineStats{
			"file.txt": {{Additions: 1, Deletions: 1}, {Additions: 2}, {Additions: 3, Deletions: 1}},
			"new.txt":  {{}, {Additions: 2}, {Additions: 2}},
		}
		for _, entry := range status.Entries {
			got := [3]protocol.GitLineStats{entry.StagedDiff, entry.UnstagedDiff, entry.Diff}
			if got != want[entry.Path] {
				t.Fatalf("%s statistics = %+v, want %+v", entry.Path, got, want[entry.Path])
			}
			for i, scope := range []protocol.GitDiffScope{protocol.GitDiffScopeStaged, protocol.GitDiffScopeUnstaged, protocol.GitDiffScopeAll} {
				diff, err := s.Diff(ctx, root, entry.Path, scope)
				if err != nil || got[i] != (protocol.GitLineStats{Additions: diff.Additions, Deletions: diff.Deletions, Truncated: diff.Truncated}) {
					t.Fatalf("%s %s statistics = %+v, diff = %+v %v", entry.Path, scope, got[i], diff, err)
				}
			}
		}
	})
	t.Run("ahead_behind_matches_reachable_sets", func(t *testing.T) {
		_, r, root := fixture(t)
		base, err := r.CommitObject(plumbing.NewHash(root))
		if err != nil {
			t.Fatal(err)
		}
		// A deterministic graph with merges, a second root and criss-cross histories.
		commits := []plumbing.Hash{base.Hash}
		parents := map[plumbing.Hash][]plumbing.Hash{}
		for i := 1; i <= 40; i++ {
			chosen := []plumbing.Hash{}
			if i != 7 {
				chosen = append(chosen, commits[(i*5+3)%len(commits)])
				if second := commits[(i*11+1)%len(commits)]; i%3 == 0 && second != chosen[0] {
					chosen = append(chosen, second)
				}
			}
			signature := object.Signature{Name: "Fixture", Email: "fixture@example.test", When: time.Unix(int64(100+i), 0)}
			obj := r.Storer.NewEncodedObject()
			if err := (&object.Commit{Author: signature, Committer: signature, Message: "commit", TreeHash: base.TreeHash, ParentHashes: chosen}).Encode(obj); err != nil {
				t.Fatal(err)
			}
			hash, err := r.Storer.SetEncodedObject(obj)
			if err != nil {
				t.Fatal(err)
			}
			commits, parents[hash] = append(commits, hash), chosen
		}
		reachable := func(tip plumbing.Hash) map[plumbing.Hash]bool {
			seen, pending := map[plumbing.Hash]bool{}, []plumbing.Hash{tip}
			for len(pending) > 0 {
				current := pending[len(pending)-1]
				pending = pending[:len(pending)-1]
				if !seen[current] {
					seen[current] = true
					pending = append(pending, parents[current]...)
				}
			}
			return seen
		}
		diverged := 0
		for _, a := range commits {
			for _, b := range commits {
				left, right := reachable(a), reachable(b)
				var ahead, behind uint64
				for hash := range left {
					if !right[hash] {
						ahead++
					}
				}
				for hash := range right {
					if !left[hash] {
						behind++
					}
				}
				if ahead > 0 && behind > 0 {
					diverged++
				}
				gotAhead, gotBehind, err := aheadBehind(ctx, r, a, b)
				if err != nil || gotAhead != ahead || gotBehind != behind {
					t.Fatalf("aheadBehind(%s, %s) = %d %d %v, want %d %d", a, b, gotAhead, gotBehind, err, ahead, behind)
				}
			}
		}
		if diverged == 0 {
			t.Fatal("fixture graph has no diverged pair")
		}
	})
}

func TestGitMutationMapping(t *testing.T) {
	ctx := context.Background()
	t.Run("manual hunk stages one selection and rejects stale sources", testManualHunks)
	t.Run("index_publish_preserves_unrelated_and_rejects_stale", func(t *testing.T) {
		root, r, commit := fixture(t)
		s := fixtureService()
		before, err := indexRevision(root, r)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(root, "new.txt"), []byte("new\n"), 0644); err != nil {
			t.Fatal(err)
		}
		got, err := s.UpdateIndex(ctx, root, protocol.CommandUpdateGitIndexPayload{Paths: []string{"new.txt"}, Operation: protocol.GitIndexChangeStage, ExpectedIndex: before, ExpectedHead: &commit})
		if err != nil || got.Revision == before {
			t.Fatal(got, err)
		}
		fresh, err := open(root)
		if err != nil {
			t.Fatal(err)
		}
		idx, err := fresh.Storer.Index()
		if err != nil {
			t.Fatal(err)
		}
		if _, err := idx.Entry("file.txt"); err != nil {
			t.Fatal("unrelated index entry lost", err)
		}
		if _, err := idx.Entry("new.txt"); err != nil {
			t.Fatal(err)
		}
		_, err = s.UpdateIndex(ctx, root, protocol.CommandUpdateGitIndexPayload{Paths: []string{"file.txt"}, Operation: protocol.GitIndexChangeUnstage, ExpectedIndex: before, ExpectedHead: &commit})
		requireFault(t, err, protocol.ErrorCodeRevisionConflict)
		if _, err := os.Lstat(filepath.Join(root, ".git", "index.lock")); !os.IsNotExist(err) {
			t.Fatal("owned lock leaked", err)
		}
	})
	t.Run("index_update_replaces_conflicting_entries_and_filters_once", func(t *testing.T) {
		root, r, _ := fixture(t)
		s := fixtureService()
		update := func(operation protocol.GitIndexChange, paths ...string) error {
			t.Helper()
			fresh, err := open(root)
			if err != nil {
				t.Fatal(err)
			}
			revision, err := indexRevision(root, fresh)
			if err != nil {
				t.Fatal(err)
			}
			current, err := head(fresh)
			if err != nil {
				t.Fatal(err)
			}
			_, err = s.UpdateIndex(ctx, root, protocol.CommandUpdateGitIndexPayload{Paths: paths, Operation: operation, ExpectedIndex: revision, ExpectedHead: current.Head})
			return err
		}
		write := func(name, body string) {
			t.Helper()
			if err := os.MkdirAll(filepath.Dir(filepath.Join(root, name)), 0755); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(root, name), []byte(body), 0644); err != nil {
				t.Fatal(err)
			}
		}
		requireIndex := func(want ...string) {
			t.Helper()
			fresh, err := open(root)
			if err != nil {
				t.Fatal(err)
			}
			idx, err := fresh.Storer.Index()
			if err != nil {
				t.Fatal(err)
			}
			got := []string{}
			for _, entry := range idx.Entries {
				got = append(got, entry.Mode.String()+" "+entry.Name)
			}
			sort.Strings(got)
			if !reflect.DeepEqual(got, want) {
				t.Fatalf("index = %q, want %q", got, want)
			}
		}
		// Several untracked paths share one ignore filter; an ignored one rejects the update.
		write(".gitignore", "*.log\n")
		write("dir/a.txt", "a\n")
		write("dir/b.txt", "b\n")
		write("dir/c.log", "c\n")
		requireFault(t, update(protocol.GitIndexChangeStage, "dir/a.txt", "dir/c.log"), protocol.ErrorCodeInvalidRequest)
		requireIndex("0100644 file.txt")
		if err := update(protocol.GitIndexChangeStage, "dir/a.txt", "dir/b.txt"); err != nil {
			t.Fatal(err)
		}
		requireIndex("0100644 dir/a.txt", "0100644 dir/b.txt", "0100644 file.txt")
		// HEAD has no such directory yet: the staged file still diffs and unstages.
		if diff, err := s.Diff(ctx, root, "dir/b.txt", protocol.GitDiffScopeStaged); err != nil || diff.Additions != 1 {
			t.Fatal(diff, err)
		}
		if err := update(protocol.GitIndexChangeUnstage, "dir/b.txt"); err != nil {
			t.Fatal(err)
		}
		requireIndex("0100644 dir/a.txt", "0100644 file.txt")
		w, err := r.Worktree()
		if err != nil {
			t.Fatal(err)
		}
		if _, err := w.Commit("Add directory", &gogit.CommitOptions{Author: &object.Signature{Name: "Fixture", Email: "fixture@example.test", When: time.Unix(200, 0)}}); err != nil {
			t.Fatal(err)
		}
		// A file staged over a directory displaces the entries below it.
		if err := os.RemoveAll(filepath.Join(root, "dir")); err != nil {
			t.Fatal(err)
		}
		write("dir", "file\n")
		if err := update(protocol.GitIndexChangeStage, "dir"); err != nil {
			t.Fatal(err)
		}
		requireIndex("0100644 dir", "0100644 file.txt")
		// HEAD holds a directory there, so unstaging the file only removes it.
		if err := update(protocol.GitIndexChangeUnstage, "dir"); err != nil {
			t.Fatal(err)
		}
		requireIndex("0100644 file.txt")
		if err := update(protocol.GitIndexChangeStage, "dir"); err != nil {
			t.Fatal(err)
		}
		// Restoring a file below it displaces the staged file.
		if err := update(protocol.GitIndexChangeUnstage, "dir/a.txt"); err != nil {
			t.Fatal(err)
		}
		requireIndex("0100644 dir/a.txt", "0100644 file.txt")
		// A file staged below a tracked file displaces that file, and unstaging the file restores it.
		if err := os.Remove(filepath.Join(root, "file.txt")); err != nil {
			t.Fatal(err)
		}
		write("file.txt/inner.txt", "inner\n")
		if err := update(protocol.GitIndexChangeStage, "file.txt/inner.txt"); err != nil {
			t.Fatal(err)
		}
		requireIndex("0100644 dir/a.txt", "0100644 file.txt/inner.txt")
		if err := update(protocol.GitIndexChangeUnstage, "file.txt"); err != nil {
			t.Fatal(err)
		}
		requireIndex("0100644 dir/a.txt", "0100644 file.txt")
	})
	t.Run("hunk_transform_uses_observed_source", func(t *testing.T) {
		root, r, commit := fixture(t)
		s := fixtureService()
		index, err := indexRevision(root, r)
		if err != nil {
			t.Fatal(err)
		}
		sum := blake3.Sum256([]byte("one\ntwo\nthree\n"))
		result, err := s.EditIndex(ctx, root, "file.txt", index, &commit, fmtHash(sum[:]), func(text string) (string, error) { return strings.Replace(text, "two", "staged", 1), nil })
		if err != nil {
			t.Fatal(err)
		}
		diff, err := s.Diff(ctx, root, "file.txt", protocol.GitDiffScopeStaged)
		if err != nil || !strings.Contains(diff.Text, "+staged") {
			t.Fatal(diff, err)
		}
		body, err := os.ReadFile(filepath.Join(root, "file.txt"))
		if err != nil || string(body) != "one\ntwo\nthree\n" {
			t.Fatal(string(body), err)
		}
		if err := os.WriteFile(filepath.Join(root, "file.txt"), []byte("external edit\n"), 0644); err != nil {
			t.Fatal(err)
		}
		_, err = s.EditIndex(ctx, root, "file.txt", result.Revision, &commit, fmtHash(sum[:]), func(text string) (string, error) { t.Fatal("stale source ran transform"); return text, nil })
		requireFault(t, err, protocol.ErrorCodeRevisionConflict)
	})
	t.Run("branch_create_no_native_process", func(t *testing.T) {
		root, r, commit := fixture(t)
		s := fixtureService()
		branch, err := s.CreateBranch(ctx, root, "feature", commit)
		if err != nil || branch.Name != "feature" {
			t.Fatal(branch, err)
		}
		ref, err := r.Reference(plumbing.NewBranchReferenceName("feature"), true)
		if err != nil || ref.Hash().String() != commit {
			t.Fatal(ref, err)
		}
		_, err = s.CreateBranch(ctx, root, "feature", commit)
		requireFault(t, err, protocol.ErrorCodeConflict)
	})
}

func TestCLIArgs(t *testing.T) {
	tests := []struct {
		name   string
		action protocol.GitAction
		want   []string
	}{
		{"fetch", protocol.GitAction{Kind: "fetch", Data: protocol.GitActionFetchPayload{Remote: ptr("origin"), Prune: true}}, []string{"fetch", "--prune", "origin"}},
		{"pull", protocol.GitAction{Kind: "pull", Data: protocol.GitActionPullPayload{Rebase: true}}, []string{"pull", "--rebase", "--no-edit"}},
		{"push", protocol.GitAction{Kind: "push", Data: protocol.GitActionPushPayload{Remote: ptr("origin"), Publish: true, Force: true}}, []string{"push", "--set-upstream", "--force-with-lease", "origin", "refs/heads/main:refs/heads/main"}},
		{"stash", protocol.GitAction{Kind: "stash", Data: protocol.GitActionStashPayload{Mode: protocol.GitStashModeAll}}, []string{"stash", "push", "--include-untracked"}},
		{"discard_literal", protocol.GitAction{Kind: "discard", Data: protocol.GitActionDiscardPayload{Paths: []string{"space name.txt", "[literal].txt"}}}, []string{"restore", "--source=HEAD", "--staged", "--worktree", "--", ":(literal)space name.txt", ":(literal)[literal].txt"}},
		{"continue", protocol.GitAction{Kind: "continue"}, []string{"rebase", "--continue"}},
		{"remote", protocol.GitAction{Kind: "add_remote", Data: protocol.GitActionAddRemotePayload{Name: "origin", Url: "ssh://git@example.test/repo"}}, []string{"remote", "add", "--", "origin", "ssh://git@example.test/repo"}},
		{"delete_remote_branch", protocol.GitAction{Kind: "delete_remote_branch", Data: protocol.GitActionDeleteRemoteBranchPayload{Branch: "origin/team/topic", Commit: strings.Repeat("a", 40)}}, []string{"push", "--force-with-lease=refs/heads/team/topic:" + strings.Repeat("a", 40), "origin", ":refs/heads/team/topic"}},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			got, err := CLIArgs(test.action, "main", "rebase", 0)
			if err != nil || !reflect.DeepEqual(got, test.want) {
				t.Fatal(got, err)
			}
		})
	}
	want := []string{"--no-optional-locks", "-c", "core.fsmonitor=false", "-c", "filter.lfs.clean=", "-c", "filter.lfs.process=", "-c", "filter.lfs.required=false", "status", "--porcelain=v2", "-z", "--branch", "--untracked-files=all", "--no-renames"}
	if got := StatusArgs([]string{"lfs"}); !reflect.DeepEqual(got, want) {
		t.Fatal(got)
	}
	if got := redact("https://user:password@example.test/repo?token=hidden password=secret"); strings.Contains(got, "password@") || strings.Contains(got, "hidden") || strings.Contains(got, "secret") {
		t.Fatal(got)
	}
	for _, branch := range []string{"--receive-pack=tool/topic", "/topic"} {
		action := protocol.GitAction{Kind: "delete_remote_branch", Data: protocol.GitActionDeleteRemoteBranchPayload{Branch: branch, Commit: strings.Repeat("a", 40)}}
		_, err := CLIArgs(action, "main", "", 0)
		requireFault(t, err, protocol.ErrorCodeInvalidRequest)
	}
	remote := protocol.GitAction{Kind: "delete_remote_branch", Data: protocol.GitActionDeleteRemoteBranchPayload{Branch: "origin/topic"}}
	if got := actionRemotes(remote); !reflect.DeepEqual(got, []string{"origin"}) {
		t.Fatal("remote branch deletion does not require its remote", got)
	}
	for stderr, code := range map[string]protocol.ErrorCode{
		"error: Your local changes to the following files would be overwritten by checkout:\n\tCargo.lock\n": protocol.ErrorCodeConflict,
		"error: Merge conflict in yarn.lock\n":                                     protocol.ErrorCodeConflict,
		"error: pathspec 'flake.lock' did not match any file(s) known to git\n":    protocol.ErrorCodeInvalidRequest,
		"fatal: Unable to create '/work/conflict/.git/index.lock': File exists.\n": protocol.ErrorCodeBusy,
	} {
		s := New(Options{Run: func(context.Context, string, []string, []byte) (CommandResult, error) {
			return CommandResult{Stderr: []byte(stderr), ExitCode: 1}, nil
		}})
		_, err := s.command(context.Background(), t.TempDir(), []string{"switch", "main"}, nil, false)
		requireFault(t, err, code)
	}
}

func TestManagedWorktreeExcluded(t *testing.T) {
	root, r, _ := fixture(t)
	s := fixtureService()
	exclude := filepath.Join(root, ".git", "info", "exclude")
	if err := os.MkdirAll(filepath.Dir(exclude), 0755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(exclude, []byte("# Keep\nprivate/"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(root, "build"), 0755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "build", "kept.txt"), []byte("kept\n"), 0644); err != nil {
		t.Fatal(err)
	}
	if w, err := r.Worktree(); err != nil {
		t.Fatal(err)
	} else if _, err := w.Add("build/kept.txt"); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, ".gitignore"), []byte("ignored.txt\nbuild*\n"), 0644); err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 2; i++ {
		path, err := s.PrepareManaged(root)
		if err != nil || path != filepath.Join(root, ".sailry", "worktrees") {
			t.Fatal(path, err)
		}
	}
	body, err := os.ReadFile(exclude)
	if err != nil || string(body) != "# Keep\nprivate/\n.sailry/\n" {
		t.Fatal(string(body), err)
	}
	filter, err := NewWatchFilter(root)
	if err != nil {
		t.Fatal(err)
	}
	if filter.Changed([]string{filepath.Join(root, "ignored.txt")}) || filter.Changed([]string{filepath.Join(root, ".sailry", "worktrees", "child", "file")}) || filter.Changed([]string{filepath.Join(root, ".git", "index.lock")}) || !filter.Changed([]string{filepath.Join(root, "file.txt")}) || !filter.ReferencesChanged([]string{filepath.Join(root, ".git", "refs", "heads", "main")}) {
		t.Fatal("watch filtering mismatch")
	}
	// Ignored paths matter only when they are tracked or hold tracked files.
	for name, want := range map[string]bool{"build": true, "build/kept.txt": true, "build/out.o": false, "build.log": false, "build/kept.txt.tmp": false, "buil": true} {
		if got := filter.Changed([]string{filepath.Join(root, filepath.FromSlash(name))}); got != want {
			t.Fatalf("Changed(%s) = %v, want %v", name, got, want)
		}
	}
}
