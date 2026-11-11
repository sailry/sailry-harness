//go:build integration

package git

import (
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"testing"
	"time"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// These probes execute only inside isolated temporary repositories. Run on each
// supported OS; cross-compilation does not prove native process or Git behavior.
func TestNativeGitIntegration(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("Git executable is required")
	}
	ctx := context.Background()
	service := func(t *testing.T) *Service {
		home := t.TempDir()
		env := []string{}
		for _, pair := range os.Environ() {
			key, _, _ := strings.Cut(pair, "=")
			if key != "HOME" && key != "USERPROFILE" && key != "XDG_CONFIG_HOME" {
				env = append(env, pair)
			}
		}
		env = append(env, "HOME="+home, "USERPROFILE="+home, "XDG_CONFIG_HOME="+filepath.Join(home, ".config"))
		return New(Options{Environment: env})
	}
	t.Run("native_status_diff_and_filtered_config", func(t *testing.T) {
		root, r, commit := fixture(t)
		config, err := r.Config()
		if err != nil {
			t.Fatal(err)
		}
		config.Raw.Section("filter").Subsection("fixture").SetOption("clean", "a-filter-that-must-never-run")
		if err := r.SetConfig(config); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(root, ".gitattributes"), []byte("*.txt filter=fixture\n"), 0644); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(root, "file.txt"), []byte("one\nchanged\nthree\n"), 0644); err != nil {
			t.Fatal(err)
		}
		s := service(t)
		status, err := s.Status(ctx, root)
		if err != nil || status.Head == nil || *status.Head != commit {
			t.Fatal(status, err)
		}
		found := false
		for _, entry := range status.Entries {
			if entry.Path == "file.txt" {
				found = true
				if entry.Diff.Additions != 1 || entry.Diff.Deletions != 1 || entry.ModifiedMs == nil {
					t.Fatal(entry)
				}
			}
		}
		if !found {
			t.Fatal(status)
		}
		actual, err := s.Diff(ctx, root, "file.txt", protocol.GitDiffScopeUnstaged)
		if err != nil {
			t.Fatal(err)
		}
		capture, err := s.command(ctx, root, []string{"-c", "filter.fixture.clean=", "-c", "filter.fixture.required=false", "diff", "--no-ext-diff", "--no-color", "--no-renames", "--", "file.txt"}, nil, true)
		if err != nil {
			t.Fatal(err)
		}
		if !reflect.DeepEqual(normalizedHunks(actual.Text), normalizedHunks(string(capture.Stdout))) {
			t.Fatalf("go-git patch:\n%s\nnative patch:\n%s", actual.Text, capture.Stdout)
		}
	})
	t.Run("discard_keeps_staged_new_file", func(t *testing.T) {
		root, _, commit := fixture(t)
		s := service(t)
		for name, body := range map[string]string{"staged.txt": "only copy\n", "file.txt": "edited\n"} {
			if err := os.WriteFile(filepath.Join(root, name), []byte(body), 0644); err != nil {
				t.Fatal(err)
			}
		}
		if _, err := s.command(ctx, root, []string{"add", "--", "staged.txt"}, nil, false); err != nil {
			t.Fatal(err)
		}
		discard := func(paths ...string) error {
			r, err := open(root)
			if err != nil {
				t.Fatal(err)
			}
			index, err := indexRevision(root, r)
			if err != nil {
				t.Fatal(err)
			}
			return s.Action(ctx, root, protocol.CommandRunGitActionPayload{Action: protocol.GitAction{Kind: "discard", Data: protocol.GitActionDiscardPayload{Paths: paths}}, ExpectedIndex: index, ExpectedHead: &commit, ExpectedBranch: ptr("main")})
		}
		requireFault(t, discard("file.txt", "staged.txt"), protocol.ErrorCodeInvalidRequest)
		for name, want := range map[string]string{"staged.txt": "only copy\n", "file.txt": "edited\n"} {
			if body, err := os.ReadFile(filepath.Join(root, name)); err != nil || string(body) != want {
				t.Fatalf("%s after a refused discard = %q %v", name, body, err)
			}
		}
		if err := discard("file.txt"); err != nil {
			t.Fatal(err)
		}
		if body, err := os.ReadFile(filepath.Join(root, "file.txt")); err != nil || string(body) != "one\ntwo\nthree\n" {
			t.Fatalf("file.txt after discard = %q %v", body, err)
		}
		status, err := s.Status(ctx, root)
		if err != nil || len(status.Entries) != 1 || status.Entries[0].Path != "staged.txt" || status.Entries[0].Staged == nil {
			t.Fatal(status, err)
		}
	})
	t.Run("index_publish_preserves_unrelated_and_rejects_stale", func(t *testing.T) {
		root, r, commit := fixture(t)
		s := service(t)
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
		s := service(t)
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
	t.Run("staging_applies_filters_and_conversion", func(t *testing.T) {
		root, r, commit := fixture(t)
		config, err := r.Config()
		if err != nil {
			t.Fatal(err)
		}
		config.Raw.Section("filter").Subsection("upper").SetOption("clean", "tr a-z A-Z")
		if err := r.SetConfig(config); err != nil {
			t.Fatal(err)
		}
		write := func(name, body string) {
			t.Helper()
			if err := os.WriteFile(filepath.Join(root, name), []byte(body), 0644); err != nil {
				t.Fatal(err)
			}
		}
		write(".gitattributes", "*.dat filter=upper\n*.txt text\n")
		write("stored.dat", "abc\n")
		write("file.txt", "one\r\ntwo\r\nthree\r\n")
		s := service(t)
		staged := func(path string) string {
			t.Helper()
			fresh, err := open(root)
			if err != nil {
				t.Fatal(err)
			}
			value, err := indexContent(fresh, path, maxFile)
			if err != nil || !value.exists {
				t.Fatal(path, value.exists, err)
			}
			return string(value.data)
		}
		revision := func() string {
			t.Helper()
			fresh, err := open(root)
			if err != nil {
				t.Fatal(err)
			}
			value, err := indexRevision(root, fresh)
			if err != nil {
				t.Fatal(err)
			}
			return value
		}
		got, err := s.UpdateIndex(ctx, root, protocol.CommandUpdateGitIndexPayload{Paths: []string{".gitattributes", "stored.dat", "file.txt", "absent.txt"}, Operation: protocol.GitIndexChangeStage, ExpectedIndex: revision(), ExpectedHead: &commit})
		if err != nil || got.Revision != revision() {
			t.Fatal(got, err)
		}
		// The index holds what the clean filter and the line-ending conversion produce, as after git add.
		if text := staged("stored.dat"); text != "ABC\n" {
			t.Fatalf("filtered blob = %q", text)
		}
		if text := staged("file.txt"); text != "one\ntwo\nthree\n" {
			t.Fatalf("converted blob = %q", text)
		}
		// A staged hunk is converted the same way.
		working := "one\r\nchanged\r\nthree\r\n"
		write("file.txt", working)
		sum := blake3.Sum256([]byte(working))
		if _, err := s.EditIndex(ctx, root, "file.txt", revision(), &commit, fmtHash(sum[:]), func(text string) (string, error) {
			return strings.Replace(text, "two\n", "changed\r\n", 1), nil
		}); err != nil {
			t.Fatal(err)
		}
		if text := staged("file.txt"); text != "one\nchanged\nthree\n" {
			t.Fatalf("converted hunk blob = %q", text)
		}
		// A path stored through a filter driver has no hunks to stage or reject.
		write("stored.dat", "abc\ndef\n")
		sum = blake3.Sum256([]byte("abc\ndef\n"))
		before := revision()
		_, err = s.EditIndex(ctx, root, "stored.dat", before, &commit, fmtHash(sum[:]), func(text string) (string, error) {
			t.Fatal("a filtered path ran the transform")
			return text, nil
		})
		requireFault(t, err, protocol.ErrorCodeInvalidRequest)
		if revision() != before {
			t.Fatal("a refused hunk changed the index")
		}
	})
	t.Run("staging_resolves_a_conflicted_path", func(t *testing.T) {
		root, _, _ := fixture(t)
		s := service(t)
		git := func(args ...string) {
			t.Helper()
			if _, err := s.command(ctx, root, append([]string{"-c", "user.name=Fixture", "-c", "user.email=fixture@example.test"}, args...), nil, false); err != nil && args[0] != "merge" {
				t.Fatal(args, err)
			}
		}
		write := func(body string) {
			t.Helper()
			if err := os.WriteFile(filepath.Join(root, "file.txt"), []byte(body), 0644); err != nil {
				t.Fatal(err)
			}
		}
		git("checkout", "-b", "other")
		write("one\ntheirs\nthree\n")
		git("commit", "--all", "--message", "Theirs")
		git("checkout", "main")
		write("one\nours\nthree\n")
		git("commit", "--all", "--message", "Ours")
		git("merge", "other")
		conflicted := func() bool {
			t.Helper()
			fresh, err := open(root)
			if err != nil {
				t.Fatal(err)
			}
			idx, err := fresh.Storer.Index()
			if err != nil {
				t.Fatal(err)
			}
			for _, entry := range idx.Entries {
				if entry.Stage != 0 {
					return true
				}
			}
			return false
		}
		if !conflicted() {
			t.Fatal("fixture merge did not conflict")
		}
		write("one\nboth\nthree\n")
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
		if _, err := s.UpdateIndex(ctx, root, protocol.CommandUpdateGitIndexPayload{Paths: []string{"file.txt"}, Operation: protocol.GitIndexChangeStage, ExpectedIndex: revision, ExpectedHead: current.Head}); err != nil {
			t.Fatal(err)
		}
		if conflicted() {
			t.Fatal("staging the resolved file left the conflict")
		}
	})
	t.Run("a_path_below_a_file_of_head_is_not_held", func(t *testing.T) {
		root, _, _ := fixture(t)
		s := service(t)
		// HEAD holds file.txt as a file; the worktree replaces it by a directory.
		if err := os.Remove(filepath.Join(root, "file.txt")); err != nil {
			t.Fatal(err)
		}
		if err := os.Mkdir(filepath.Join(root, "file.txt"), 0755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(root, "file.txt", "inner"), []byte("new\n"), 0644); err != nil {
			t.Fatal(err)
		}
		update := func(operation protocol.GitIndexChange) {
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
			if _, err := s.UpdateIndex(ctx, root, protocol.CommandUpdateGitIndexPayload{Paths: []string{"file.txt/inner"}, Operation: operation, ExpectedIndex: revision, ExpectedHead: current.Head}); err != nil {
				t.Fatal(operation, err)
			}
		}
		staged := func() bool {
			t.Helper()
			fresh, err := open(root)
			if err != nil {
				t.Fatal(err)
			}
			idx, err := fresh.Storer.Index()
			if err != nil {
				t.Fatal(err)
			}
			_, err = idx.Entry("file.txt/inner")
			return err == nil
		}
		update(protocol.GitIndexChangeStage)
		if !staged() {
			t.Fatal("fixture did not stage the file below the replaced path")
		}
		fresh, err := open(root)
		if err != nil {
			t.Fatal(err)
		}
		tree, err := headTree(fresh)
		if err != nil {
			t.Fatal(err)
		}
		if held, err := heldByHead(tree, "file.txt/inner"); held || err != nil {
			t.Fatalf("held = %v %v", held, err)
		}
		if before, err := treeContent(fresh, tree, "file.txt/inner", 1024); before.exists || err != nil {
			t.Fatalf("HEAD content = %v %v", before.exists, err)
		}
		update(protocol.GitIndexChangeUnstage)
		if staged() {
			t.Fatal("unstaging left the entry")
		}
	})
	t.Run("managed_snapshot_and_targeted_removal", func(t *testing.T) {
		root, r, commit := fixture(t)
		s := service(t)
		index, err := indexRevision(root, r)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(root, "file.txt"), []byte("changed\n"), 0644); err != nil {
			t.Fatal(err)
		}
		id := protocol.NewWorktreeID()
		path, err := s.CreateManagedWorktree(ctx, root, root, id, protocol.CommandCreateManagedWorktreePayload{Branch: "managed-fixture", ExpectedHead: commit, ExpectedIndex: index, IncludeChanges: true})
		if err != nil {
			t.Fatal(err)
		}
		body, err := os.ReadFile(filepath.Join(path, "file.txt"))
		if err != nil || string(body) != "changed\n" {
			t.Fatal(string(body), err)
		}
		sourceIndex, err := indexRevision(root, r)
		if err != nil || sourceIndex != index {
			t.Fatal("source index changed", sourceIndex, err)
		}
		requireFault(t, s.RemoveWorktree(ctx, path, commit, "managed-fixture"), protocol.ErrorCodeConflict)
		if _, err := s.command(ctx, path, []string{"restore", "--source=HEAD", "--worktree", "--", ":(literal)file.txt"}, nil, false); err != nil {
			t.Fatal(err)
		}
		if err := s.RemoveWorktree(ctx, path, commit, "managed-fixture"); err != nil {
			t.Fatal(err)
		}
	})
	t.Run("missing_worktree_registration", func(t *testing.T) {
		root, _, commit := fixture(t)
		s := service(t)
		parent := t.TempDir()
		path, err := s.CreateWorktree(ctx, root, filepath.Join(parent, "checkout"), "stale-fixture", commit)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.RemoveAll(path); err != nil {
			t.Fatal(err)
		}
		if err := s.PruneWorktree(ctx, root, path); err != nil {
			t.Fatal(err)
		}
		entries, err := s.ListWorktrees(ctx, root)
		if err != nil {
			t.Fatal(err)
		}
		for _, entry := range entries.Entries {
			if entry.Path == path {
				t.Fatal("stale registration retained", entries)
			}
		}
	})
	t.Run("worktree_manager_faults", func(t *testing.T) {
		root, _, commit := fixture(t)
		s := service(t)
		parent := t.TempDir()
		named := func(err error, code protocol.ErrorCode, message string) {
			t.Helper()
			requireFault(t, err, code)
			var value *protocol.Fault
			if !errors.As(err, &value) || value.Message != message {
				t.Fatalf("fault message = %v, want %q", err, message)
			}
		}
		path, err := s.CreateWorktree(ctx, root, filepath.Join(parent, "linked"), "linked-fixture", commit)
		if err != nil {
			t.Fatal(err)
		}
		_, err = s.CreateWorktree(ctx, root, filepath.Join(parent, "other"), "linked-fixture", commit)
		named(err, protocol.ErrorCodeConflict, BranchExists)
		_, err = s.CreateWorktree(ctx, root, path, "second-fixture", commit)
		named(err, protocol.ErrorCodeConflict, DestinationExists)
		untracked := filepath.Join(path, "untracked.txt")
		if err := os.WriteFile(untracked, []byte("draft\n"), 0644); err != nil {
			t.Fatal(err)
		}
		named(s.RemoveWorktree(ctx, path, commit, "linked-fixture"), protocol.ErrorCodeConflict, WorktreeDirty)
		if err := os.Remove(untracked); err != nil {
			t.Fatal(err)
		}
		if _, err := s.command(ctx, root, []string{"worktree", "lock", "--", path}, nil, false); err != nil {
			t.Fatal(err)
		}
		listed, err := s.ListWorktrees(ctx, root)
		if err != nil || len(listed.Entries) != 2 || !listed.Entries[0].Main || listed.Entries[0].Locked {
			t.Fatal(listed, err)
		}
		if linked := listed.Entries[1]; linked.Path != path || linked.Main || !linked.Locked || !linked.Available || linked.Branch == nil || *linked.Branch != "linked-fixture" {
			t.Fatalf("locked worktree listing: %+v", linked)
		}
		named(s.RemoveWorktree(ctx, path, commit, "linked-fixture"), protocol.ErrorCodeBusy, WorktreeLocked)
		named(s.PruneWorktree(ctx, root, path), protocol.ErrorCodeBusy, WorktreeLocked)
		if _, err := s.command(ctx, root, []string{"worktree", "unlock", "--", path}, nil, false); err != nil {
			t.Fatal(err)
		}
		named(s.PruneWorktree(ctx, root, path), protocol.ErrorCodeConflict, WorktreePathExists)
		if err := os.RemoveAll(path); err != nil {
			t.Fatal(err)
		}
		listed, err = s.ListWorktrees(ctx, root)
		if err != nil || len(listed.Entries) != 2 || listed.Entries[1].Path != path || listed.Entries[1].Available || listed.Entries[1].Locked {
			t.Fatalf("stale worktree listing: %+v %v", listed, err)
		}
	})
}
func normalizedHunks(text string) []string {
	result := []string{}
	active := false
	for _, line := range strings.SplitAfter(text, "\n") {
		if strings.HasPrefix(line, "@@ ") {
			active = true
			if end := strings.Index(line[3:], " @@"); end >= 0 {
				line = line[:3+end+3] + "\n"
			}
		}
		if active {
			result = append(result, line)
		}
	}
	return result
}
