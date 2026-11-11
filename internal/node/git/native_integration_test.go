//go:build integration

package git

import (
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
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
