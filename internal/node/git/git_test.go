package git

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/config"
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

// fixtureService answers native Git without a process. Hunk staging asks Git
// for a path's filter driver and for the blob of the staged text; the fixture
// reports no driver and stores the text unconverted. Asked for the stored form
// of worktree files, it hashes their bytes: the fixture converts nothing.
func fixtureService() *Service {
	return New(Options{Run: func(_ context.Context, root string, args []string, input []byte) (CommandResult, error) {
		switch args[0] {
		case "check-attr":
			return CommandResult{Stdout: []byte(args[len(args)-1] + "\x00filter\x00unspecified\x00")}, nil
		case "hash-object":
			if args[1] == "--stdin-paths" {
				var out strings.Builder
				for _, path := range strings.Split(strings.TrimSuffix(string(input), "\n"), "\n") {
					body, err := os.ReadFile(filepath.Join(root, path))
					if err != nil {
						return CommandResult{ExitCode: 128}, nil
					}
					out.WriteString(plumbing.ComputeHash(plumbing.BlobObject, body).String() + "\n")
				}
				return CommandResult{Stdout: []byte(out.String())}, nil
			}
			r, err := open(root)
			if err != nil {
				return CommandResult{}, err
			}
			blob := r.Storer.NewEncodedObject()
			blob.SetType(plumbing.BlobObject)
			writer, err := blob.Writer()
			if err != nil {
				return CommandResult{}, err
			}
			if _, err := writer.Write(input); err != nil {
				return CommandResult{}, err
			}
			if err := writer.Close(); err != nil {
				return CommandResult{}, err
			}
			hash, err := r.Storer.SetEncodedObject(blob)
			return CommandResult{Stdout: []byte(hash.String() + "\n")}, err
		}
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
	t.Run("worktree_listing_marks_main_locked_and_stale", func(t *testing.T) {
		main, linked := t.TempDir(), t.TempDir()
		stale := filepath.Join(t.TempDir(), "removed")
		head := strings.Repeat("a", 40)
		listing := "worktree " + main + "\x00HEAD " + head + "\x00branch refs/heads/main\x00\x00" +
			"worktree " + stale + "\x00HEAD " + head + "\x00branch refs/heads/stale\x00prunable gitdir file points to non-existent location\x00\x00" +
			"worktree " + linked + "\x00HEAD " + head + "\x00detached\x00locked in use\x00\x00"
		result, err := ParseWorktrees([]byte(listing), protocol.RepositoryKindReady, false)
		if err != nil || len(result.Entries) != 3 || result.Truncated {
			t.Fatal(result, err)
		}
		entries := map[string]protocol.GitWorktree{}
		for _, entry := range result.Entries {
			entries[entry.Path] = entry
		}
		if entry := result.Entries[0]; entry.Path != main || !entry.Main || !entry.Available || entry.Locked || entry.Branch == nil || *entry.Branch != "main" {
			t.Fatalf("main worktree: %+v", entry)
		}
		if entry := entries[stale]; entry.Main || entry.Available || entry.Locked || entry.Branch == nil || *entry.Branch != "stale" {
			t.Fatalf("stale worktree: %+v", entry)
		}
		if entry := entries[linked]; entry.Main || !entry.Available || !entry.Locked || entry.Branch != nil || entry.Head == nil || *entry.Head != head {
			t.Fatalf("locked worktree: %+v", entry)
		}
	})
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
	// A history graph needs each commit's parents and the names at it: a
	// merge lists both, the root an empty list, and HEAD is named at the
	// commit checked out.
	t.Run("log_parents_and_decorations", func(t *testing.T) {
		root, r, first := fixture(t)
		s := fixtureService()
		w, err := r.Worktree()
		if err != nil {
			t.Fatal(err)
		}
		commit := func(message string, at int64, parents ...plumbing.Hash) plumbing.Hash {
			t.Helper()
			hash, err := w.Commit(message, &gogit.CommitOptions{AllowEmptyCommits: true, Parents: parents,
				Author: &object.Signature{Name: "Fixture", Email: "fixture@example.test", When: time.Unix(at, 0)}})
			if err != nil {
				t.Fatal(err)
			}
			return hash
		}
		base := plumbing.NewHash(first)
		side := commit("Side", 200, base)
		main := commit("Main", 300, base)
		merge := commit("Merge", 400, main, side)
		for name, hash := range map[plumbing.ReferenceName]plumbing.Hash{
			plumbing.NewBranchReferenceName("side"): side, plumbing.NewTagReferenceName("v1"): side,
			plumbing.NewRemoteReferenceName("origin", "main"): main} {
			if err := r.Storer.SetReference(plumbing.NewHashReference(name, hash)); err != nil {
				t.Fatal(err)
			}
		}
		log, err := s.Log(ctx, root, protocol.CommandReadGitLogPayload{Limit: 10})
		if err != nil || len(log.Entries) != 4 {
			t.Fatal(log, err)
		}
		entries := map[string]protocol.GitLogEntry{}
		for _, entry := range log.Entries {
			entries[entry.ID] = entry
		}
		if got := entries[merge.String()]; !slices.Equal(got.Parents, []string{main.String(), side.String()}) || !slices.Equal(got.References, []string{"HEAD", "heads/main"}) {
			t.Fatalf("merge: %+v", got)
		}
		if got := entries[side.String()]; !slices.Equal(got.Parents, []string{first}) || !slices.Equal(got.References, []string{"heads/side", "tags/v1"}) {
			t.Fatalf("side: %+v", got)
		}
		if got := entries[main.String()]; !slices.Equal(got.References, []string{"remotes/origin/main"}) {
			t.Fatalf("main: %+v", got)
		}
		if got := entries[first]; got.Parents == nil || len(got.Parents) != 0 {
			t.Fatalf("the root's parents are %#v, want an empty list", got.Parents)
		}
		// Detached, HEAD is the only name at its commit.
		if err := r.Storer.SetReference(plumbing.NewHashReference(plumbing.HEAD, main)); err != nil {
			t.Fatal(err)
		}
		log, err = s.Log(ctx, root, protocol.CommandReadGitLogPayload{Limit: 1})
		if err != nil || len(log.Entries) != 1 || !slices.Equal(log.Entries[0].References, []string{"HEAD", "remotes/origin/main"}) {
			t.Fatal(log, err)
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
		s := New(Options{Run: func(_ context.Context, _ string, args []string, _ []byte) (CommandResult, error) {
			// The status also asks for the stored form of its modified file.
			if args[0] == "hash-object" {
				return CommandResult{ExitCode: 128}, nil
			}
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
		if err := os.WriteFile(filepath.Join(root, "file.txt"), []byte("one\nstaged\nthree\n"), 0644); err != nil {
			t.Fatal(err)
		}
		w, err := r.Worktree()
		if err != nil {
			t.Fatal(err)
		}
		if _, err := w.Add("file.txt"); err != nil {
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
	t.Run("status_line_statistics_are_bounded", func(t *testing.T) {
		root, _, commit := fixture(t)
		// One byte over the limit: such a file is marked, not read and hashed.
		huge, err := os.Create(filepath.Join(root, "huge.bin"))
		if err != nil {
			t.Fatal(err)
		}
		if err := errors.Join(huge.Truncate(statLimit+1), huge.Close()); err != nil {
			t.Fatal(err)
		}
		for name, body := range map[string]string{"file.txt": "one\ntwo\nthree\nfour\n", "binary.dat": "a\x00b\n"} {
			if err := os.WriteFile(filepath.Join(root, name), []byte(body), 0644); err != nil {
				t.Fatal(err)
			}
		}
		body := "# branch.oid " + commit + "\x00# branch.head main\x001 .M N... 100644 100644 100644 abc def file.txt\x00? binary.dat\x00? huge.bin\x00"
		s := New(Options{Run: func(context.Context, string, []string, []byte) (CommandResult, error) {
			return CommandResult{Stdout: []byte(body)}, nil
		}})
		statistics := func() map[string][3]protocol.GitLineStats {
			t.Helper()
			s.Invalidate(root)
			status, err := s.Status(ctx, root)
			if err != nil || len(status.Entries) != 3 {
				t.Fatal(status, err)
			}
			got := map[string][3]protocol.GitLineStats{}
			for _, entry := range status.Entries {
				got[entry.Path] = [3]protocol.GitLineStats{entry.StagedDiff, entry.UnstagedDiff, entry.Diff}
			}
			return got
		}
		uncounted := protocol.GitLineStats{Truncated: true}
		if got, want := statistics(), map[string][3]protocol.GitLineStats{
			"file.txt":   {{}, {Additions: 1}, {Additions: 1}},
			"binary.dat": {{}, {}, {}},
			"huge.bin":   {{}, uncounted, uncounted},
		}; !reflect.DeepEqual(got, want) {
			t.Fatalf("statistics = %+v, want %+v", got, want)
		}
		// Out of time, a status still answers: the entries stay uncounted.
		s.stats = 0
		if got, want := statistics(), map[string][3]protocol.GitLineStats{
			"file.txt":   {{}, uncounted, uncounted},
			"binary.dat": {{}, uncounted, uncounted},
			"huge.bin":   {{}, uncounted, uncounted},
		}; !reflect.DeepEqual(got, want) {
			t.Fatalf("statistics without time = %+v, want %+v", got, want)
		}
		// A comparison cut short reports its coarse counts as uncounted.
		before, after := content{data: []byte("a\nb\n"), exists: true, hash: plumbing.NewHash("01")}, content{data: []byte("a\nc\n"), exists: true, hash: plumbing.NewHash("02")}
		if got := lineStats(before, after, nil, nil, time.Nanosecond); !got.Truncated {
			t.Fatalf("cut comparison = %+v", got)
		}
		if got := lineStats(before, after, nil, nil, time.Minute); got != (protocol.GitLineStats{Additions: 1, Deletions: 1}) {
			t.Fatalf("comparison = %+v", got)
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
	t.Run("transfers_with_a_remote_run_outside_the_root_lock", func(t *testing.T) {
		root, r, commit := fixture(t)
		var log []string
		s := New(Options{
			Run: func(_ context.Context, _ string, args []string, _ []byte) (CommandResult, error) {
				log = append(log, strings.Join(args, " "))
				return CommandResult{}, nil
			},
			Lock: func(string) func() {
				log = append(log, "lock")
				return func() { log = append(log, "unlock") }
			},
		})
		index, err := indexRevision(root, r)
		if err != nil {
			t.Fatal(err)
		}
		action := func(kind string, data any) error {
			return s.Action(ctx, root, protocol.CommandRunGitActionPayload{Action: protocol.GitAction{Kind: kind, Data: data}, ExpectedIndex: index, ExpectedHead: &commit, ExpectedBranch: ptr("main")})
		}
		for _, probe := range []struct {
			name string
			run  func() error
			want []string
		}{
			{"push", func() error { return action("push", protocol.GitActionPushPayload{}) }, []string{"push --progress"}},
			{"fetch", func() error { return action("fetch", protocol.GitActionFetchPayload{All: true}) }, []string{"fetch --progress --all"}},
			// The transfer of a pull is a fetch without the lock; git pull then integrates under it.
			{"pull", func() error { return action("pull", protocol.GitActionPullPayload{Rebase: true}) }, []string{"fetch --progress", "lock", "pull --progress --rebase --no-edit", "unlock"}},
			{"sync", func() error { return action("sync", protocol.GitActionSyncPayload{}) }, []string{"fetch --progress", "lock", "pull --progress --no-rebase --no-edit", "unlock", "push --progress"}},
			{"local action", func() error {
				return action("add_remote", protocol.GitActionAddRemotePayload{Name: "origin", Url: "https://example.invalid/repository.git"})
			}, []string{"lock", "remote add -- origin https://example.invalid/repository.git", "unlock"}},
			{"commit and sync", func() error {
				input := protocol.CommandCreateGitCommitPayload{Message: "Change", ExpectedIndex: index, ExpectedHead: &commit, ExpectedBranch: ptr("main")}
				input.Options.After = protocol.GitAfterCommitSync
				_, err := s.CreateCommit(ctx, root, input)
				return err
			}, []string{"lock", "commit --message Change", "unlock", "fetch --progress", "lock", "pull --progress --no-rebase --no-edit", "unlock", "push --progress"}},
		} {
			log = nil
			if err := probe.run(); err != nil || !reflect.DeepEqual(log, probe.want) {
				t.Fatalf("%s = %q %v, want %q", probe.name, log, err, probe.want)
			}
		}
		// A pull whose transfer fails does not reach the worktree.
		failing := New(Options{Run: func(_ context.Context, _ string, args []string, _ []byte) (CommandResult, error) {
			log = append(log, args[0])
			return CommandResult{ExitCode: 128, Stderr: []byte("fatal: Authentication failed")}, nil
		}})
		log = nil
		requireFault(t, failing.Action(ctx, root, protocol.CommandRunGitActionPayload{Action: protocol.GitAction{Kind: "pull", Data: protocol.GitActionPullPayload{}}, ExpectedIndex: index, ExpectedHead: &commit, ExpectedBranch: ptr("main")}), protocol.ErrorCodePermissionDenied)
		if !reflect.DeepEqual(log, []string{"fetch"}) {
			t.Fatalf("failed pull ran %q", log)
		}
	})
	t.Run("remote_branch_deletion_names_the_longest_configured_remote", func(t *testing.T) {
		root, r, commit := fixture(t)
		for _, remote := range []string{"origin", "origin/team"} {
			if _, err := r.CreateRemote(&config.RemoteConfig{Name: remote, URLs: []string{"https://example.invalid/" + remote + ".git"}}); err != nil {
				t.Fatal(err)
			}
		}
		var log []string
		s := New(Options{Run: func(_ context.Context, _ string, args []string, _ []byte) (CommandResult, error) {
			log = append(log, strings.Join(args, " "))
			return CommandResult{}, nil
		}})
		index, err := indexRevision(root, r)
		if err != nil {
			t.Fatal(err)
		}
		remove := func(branch string) error {
			return s.Action(ctx, root, protocol.CommandRunGitActionPayload{Action: protocol.GitAction{Kind: "delete_remote_branch", Data: protocol.GitActionDeleteRemoteBranchPayload{Branch: branch, Commit: commit}}, ExpectedIndex: index, ExpectedHead: &commit, ExpectedBranch: ptr("main")})
		}
		for branch, want := range map[string]string{
			"origin/topic":      "push --progress --force-with-lease=refs/heads/topic:" + commit + " origin :refs/heads/topic",
			"origin/team/topic": "push --progress --force-with-lease=refs/heads/topic:" + commit + " origin/team :refs/heads/topic",
		} {
			log = nil
			if err := remove(branch); err != nil || !reflect.DeepEqual(log, []string{want}) {
				t.Fatalf("%s = %q %v, want %q", branch, log, err, want)
			}
		}
		log = nil
		requireFault(t, remove("upstream/topic"), protocol.ErrorCodeNotConfigured)
		for _, branch := range []string{"--receive-pack=tool/topic", "/topic"} {
			if err := remove(branch); err == nil || len(log) != 0 {
				t.Fatalf("%q ran %q %v", branch, log, err)
			}
		}
	})
	t.Run("discard_refuses_paths_head_does_not_hold", func(t *testing.T) {
		root, r, commit := fixture(t)
		for name, body := range map[string]string{"staged.txt": "only copy\n", "untracked.txt": "draft\n", "file.txt": "edited\n"} {
			if err := os.WriteFile(filepath.Join(root, name), []byte(body), 0644); err != nil {
				t.Fatal(err)
			}
		}
		w, err := r.Worktree()
		if err != nil {
			t.Fatal(err)
		}
		if _, err := w.Add("staged.txt"); err != nil {
			t.Fatal(err)
		}
		index, err := indexRevision(root, r)
		if err != nil {
			t.Fatal(err)
		}
		var ran [][]string
		s := New(Options{Run: func(_ context.Context, _ string, args []string, _ []byte) (CommandResult, error) {
			ran = append(ran, args)
			return CommandResult{}, nil
		}})
		discard := func(paths ...string) error {
			return s.Action(ctx, root, protocol.CommandRunGitActionPayload{Action: protocol.GitAction{Kind: "discard", Data: protocol.GitActionDiscardPayload{Paths: paths}}, ExpectedIndex: index, ExpectedHead: &commit, ExpectedBranch: ptr("main")})
		}
		// One path HEAD does not hold refuses the whole selection before Git runs.
		for _, paths := range [][]string{{"staged.txt"}, {"untracked.txt"}, {"file.txt", "staged.txt"}} {
			requireFault(t, discard(paths...), protocol.ErrorCodeInvalidRequest)
		}
		if len(ran) != 0 {
			t.Fatalf("refused discard ran Git: %q", ran)
		}
		if err := discard("file.txt"); err != nil {
			t.Fatal(err)
		}
		want := []string{"restore", "--source=HEAD", "--staged", "--worktree", "--", ":(literal)file.txt"}
		if len(ran) != 1 || !reflect.DeepEqual(ran[0], want) {
			t.Fatalf("discard ran %q", ran)
		}
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
