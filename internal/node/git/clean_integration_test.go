//go:build integration

package git

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// TestStoredContentIntegration runs native Git with a counting clean filter and
// line-ending conversion in a temporary repository: status, change inventory
// and diffs compare what Git would store, and only for files Git reads again.
func TestStoredContentIntegration(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("Git executable is required")
	}
	if runtime.GOOS == "windows" {
		t.Skip("the fixture filter is a shell command")
	}
	ctx := context.Background()
	root, r, _ := fixture(t)
	env := []string{}
	for _, pair := range os.Environ() {
		if key, _, _ := strings.Cut(pair, "="); key != "XDG_CONFIG_HOME" {
			env = append(env, pair)
		}
	}
	s := New(Options{Environment: append(env, "XDG_CONFIG_HOME="+filepath.Join(os.Getenv("HOME"), ".config"))})
	counter := filepath.Join(t.TempDir(), "runs")
	config, err := r.Config()
	if err != nil {
		t.Fatal(err)
	}
	config.Raw.Section("filter").Subsection("upper").SetOption("clean", "echo run >> '"+counter+"'; tr a-z A-Z")
	if err := r.SetConfig(config); err != nil {
		t.Fatal(err)
	}
	git := func(args ...string) {
		t.Helper()
		if _, err := s.command(ctx, root, append([]string{"-c", "user.name=Fixture", "-c", "user.email=fixture@example.test"}, args...), nil, false); err != nil {
			t.Fatal(args, err, s.Output(root))
		}
	}
	// A file older than the index is one Git accepts by its stat data.
	write := func(name, body string, age time.Duration) {
		t.Helper()
		path := filepath.Join(root, name)
		if err := os.WriteFile(path, []byte(body), 0644); err != nil {
			t.Fatal(err)
		}
		at := time.Now().Add(-age)
		if err := os.Chtimes(path, at, at); err != nil {
			t.Fatal(err)
		}
	}
	runs := func() int {
		t.Helper()
		body, err := os.ReadFile(counter)
		if err != nil && !os.IsNotExist(err) {
			t.Fatal(err)
		}
		return strings.Count(string(body), "run")
	}
	status := func() map[string]protocol.GitEntry {
		t.Helper()
		s.Invalidate(root)
		value, err := s.Status(ctx, root)
		if err != nil {
			t.Fatal(err)
		}
		entries := map[string]protocol.GitEntry{}
		for _, entry := range value.Entries {
			entries[entry.Path] = entry
		}
		return entries
	}
	changes := func() protocol.GitChanges {
		t.Helper()
		target := protocol.GitChangeTarget{Kind: "worktree", Data: protocol.GitChangeTargetWorktreePayload{Scope: protocol.GitDiffScopeUnstaged}}
		value, err := s.Changes(ctx, root, protocol.CommandListGitChangesPayload{Target: &target, Limit: 10})
		if err != nil {
			t.Fatal(err)
		}
		return value
	}
	diff := func(path string) protocol.GitDiff {
		t.Helper()
		value, err := s.Diff(ctx, root, path, protocol.GitDiffScopeUnstaged)
		if err != nil {
			t.Fatal(err)
		}
		return value
	}
	write(".gitattributes", "*.dat filter=upper\n*.txt text=auto eol=lf\n", time.Hour)
	write("stored.dat", "abc\ndef\n", time.Hour)
	// The index records the size of the CRLF file with its LF blob, as after a
	// checkout that converts line endings.
	write("file.txt", "one\r\ntwo\r\nthree\r\n", time.Hour)
	git("add", "--all")
	git("commit", "--message", "Add a filtered file")
	if err := os.Remove(counter); err != nil {
		t.Fatal("staging did not run the clean filter", err)
	}

	t.Run("stat_equal_files_run_no_filter", func(t *testing.T) {
		if entries := status(); len(entries) != 0 {
			t.Fatalf("status = %+v", entries)
		}
		if list := changes(); len(list.Entries) != 0 {
			t.Fatalf("changes = %+v", list.Entries)
		}
		if got := runs(); got != 0 {
			t.Fatalf("filter ran %d times for files whose stat data matches the index", got)
		}
	})
	t.Run("touched_filtered_file_is_clean", func(t *testing.T) {
		write("stored.dat", "abc\ndef\n", 30*time.Minute)
		if entries := status(); len(entries) != 0 {
			t.Fatalf("status = %+v", entries)
		}
		if got := runs(); got != 1 {
			t.Fatalf("status ran the filter %d times for one touched file", got)
		}
		if list := changes(); len(list.Entries) != 0 {
			t.Fatalf("changes = %+v", list.Entries)
		}
		if got := runs(); got != 2 {
			t.Fatalf("filter runs after the change inventory = %d, want 2", got)
		}
		if value := diff("stored.dat"); value.Text != "" || value.Additions != 0 || value.Deletions != 0 {
			t.Fatalf("diff of an unchanged filtered file = %+v", value)
		}
	})
	t.Run("changed_filtered_file_is_modified", func(t *testing.T) {
		write("stored.dat", "abc\nxyz\n", 30*time.Minute)
		entry, found := status()["stored.dat"]
		if !found || entry.Unstaged == nil || *entry.Unstaged != protocol.GitChangeKindModified {
			t.Fatalf("status entry = %+v %v", entry, found)
		}
		// Counting lines never stores a blob, so this side stays uncounted.
		if !entry.UnstagedDiff.Truncated {
			t.Fatalf("line statistics = %+v", entry.UnstagedDiff)
		}
		if list := changes(); len(list.Entries) != 1 || list.Entries[0].Path != "stored.dat" {
			t.Fatalf("changes = %+v", list.Entries)
		}
		// The diff is between stored forms, as git diff shows it.
		if value := diff("stored.dat"); value.Additions != 1 || value.Deletions != 1 || !strings.Contains(value.Text, "-DEF\n+XYZ\n") {
			t.Fatalf("diff = %+v", value)
		}
		write("stored.dat", "abc\ndef\n", 30*time.Minute)
	})
	t.Run("line_ending_conversion", func(t *testing.T) {
		write("file.txt", "one\r\ntwo\r\nthree\r\n", 30*time.Minute)
		if entries := status(); len(entries) != 0 {
			t.Fatalf("status of an unchanged CRLF file = %+v", entries)
		}
		if list := changes(); len(list.Entries) != 0 {
			t.Fatalf("changes = %+v", list.Entries)
		}
		if value := diff("file.txt"); value.Text != "" {
			t.Fatalf("diff of an unchanged CRLF file = %+v", value)
		}
		// Like git status, a file whose size differs from the recorded one is
		// listed without a content comparison; its diff and counts are empty.
		write("file.txt", "one\ntwo\nthree\n", 30*time.Minute)
		if entry, found := status()["file.txt"]; !found || entry.UnstagedDiff != (protocol.GitLineStats{}) {
			t.Fatalf("status of a file stored unchanged with another size = %+v %v", entry, found)
		}
		if value := diff("file.txt"); value.Text != "" {
			t.Fatalf("diff of a file stored unchanged = %+v", value)
		}
		if list := changes(); len(list.Entries) != 0 {
			t.Fatalf("changes = %+v", list.Entries)
		}
		working := "one\r\nchanged\r\nthree\r\n"
		write("file.txt", working, 30*time.Minute)
		entry, found := status()["file.txt"]
		if want := (protocol.GitLineStats{Additions: 1, Deletions: 1}); !found || entry.UnstagedDiff != want || entry.Diff != want {
			t.Fatalf("status entry = %+v %v", entry, found)
		}
		value := diff("file.txt")
		if value.Additions != 1 || value.Deletions != 1 || !strings.Contains(value.Text, " one\n-two\n+changed\n three\n") {
			t.Fatalf("diff of one edited line = %+v", value)
		}
		list := changes()
		if len(list.Entries) != 1 || list.Entries[0].Path != "file.txt" {
			t.Fatalf("changes = %+v", list.Entries)
		}
		change, err := s.Change(ctx, root, protocol.CommandReadGitChangePayload{Snapshot: list.Snapshot, Path: "file.txt", ExpectedMetadataRevision: list.Entries[0].MetadataRevision, Limit: 10})
		if err != nil {
			t.Fatal(err)
		}
		added, deleted := 0, 0
		for _, line := range change.Lines {
			switch line.Kind {
			case protocol.GitChangeLineKindAdded:
				added++
			case protocol.GitChangeLineKindDeleted:
				deleted++
			}
		}
		if added != 1 || deleted != 1 {
			t.Fatalf("change lines = %+v", change.Lines)
		}
		// Reverting the hunk restores the line with the file's own line ending.
		fresh, err := open(root)
		if err != nil {
			t.Fatal(err)
		}
		index, err := indexRevision(root, fresh)
		if err != nil {
			t.Fatal(err)
		}
		current, err := head(fresh)
		if err != nil {
			t.Fatal(err)
		}
		source, patch := blake3.Sum256([]byte(working)), blake3.Sum256([]byte(value.Text))
		hunk := protocol.CommandApplyGitHunkPayload{Worktree: protocol.NewWorktreeID(), Path: "file.txt", Scope: protocol.GitDiffScopeUnstaged, Operation: protocol.GitHunkReject, DiffRevision: fmtHash(patch[:]), ExpectedRevision: fmtHash(source[:]), ExpectedIndex: index, ExpectedHead: current.Head, ExpectedBranch: current.Branch}
		if _, err := s.ApplyHunk(ctx, root, hunk); err != nil {
			t.Fatal(err)
		}
		if body, err := os.ReadFile(filepath.Join(root, "file.txt")); err != nil || string(body) != "one\r\ntwo\r\nthree\r\n" {
			t.Fatalf("file after reverting the hunk = %q %v", body, err)
		}
		if entries := status(); len(entries) != 0 {
			t.Fatalf("status after reverting the hunk = %+v", entries)
		}
		// Staging the hunk stores the edited line in the stored form.
		write("file.txt", working, 30*time.Minute)
		hunk.Operation = protocol.GitHunkStage
		if _, err := s.ApplyHunk(ctx, root, hunk); err != nil {
			t.Fatal(err)
		}
		if fresh, err = open(root); err != nil {
			t.Fatal(err)
		}
		if staged, err := indexContent(fresh, "file.txt", maxFile); err != nil || string(staged.data) != "one\nchanged\nthree\n" {
			t.Fatalf("staged hunk = %q %v", staged.data, err)
		}
		if entry := status()["file.txt"]; entry.Staged == nil || entry.Unstaged != nil {
			t.Fatalf("status after staging the hunk = %+v", entry)
		}
	})
	t.Run("status_answers_when_a_required_filter_cannot_run", func(t *testing.T) {
		config, err := r.Config()
		if err != nil {
			t.Fatal(err)
		}
		config.Raw.Section("filter").Subsection("upper").SetOption("clean", "a-filter-program-that-is-not-installed").SetOption("required", "true")
		if err := r.SetConfig(config); err != nil {
			t.Fatal(err)
		}
		write("stored.dat", "abc\ndef\n", 20*time.Minute)
		// Without the filter Git compares the file's bytes with the stored form.
		if entry, found := status()["stored.dat"]; !found || entry.Unstaged == nil {
			t.Fatalf("status entry = %+v %v", entry, found)
		}
	})
}
