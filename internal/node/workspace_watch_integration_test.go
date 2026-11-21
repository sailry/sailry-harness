//go:build integration

package node

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	workspacegit "github.com/sailry/sailry-harness/internal/node/git"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestGitWorktreeConfigIntegration(t *testing.T) {
	executable, err := exec.LookPath("git")
	if err != nil {
		t.Skip("Git executable is required")
	}
	directory, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("USERPROFILE", home)
	t.Setenv("XDG_CONFIG_HOME", filepath.Join(home, ".config"))
	var environment []string
	for _, value := range os.Environ() {
		key, _, _ := strings.Cut(value, "=")
		if !strings.HasPrefix(strings.ToUpper(key), "GIT_") {
			environment = append(environment, value)
		}
	}
	fixtureEnvironment := append(slices.Clone(environment), "GIT_CONFIG_NOSYSTEM=1", "GIT_CONFIG_GLOBAL="+filepath.Join(home, ".gitconfig"))
	main, linked := filepath.Join(directory, "main"), filepath.Join(directory, "linked")
	git := func(t *testing.T, root string, args ...string) string {
		t.Helper()
		ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
		defer cancel()
		command := exec.CommandContext(ctx, executable, append([]string{"--no-pager", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.test"}, args...)...)
		command.Dir, command.Env = root, fixtureEnvironment
		output, err := command.CombinedOutput()
		if err != nil {
			t.Fatalf("fixture git %v: %v: %s", args, err, output)
		}
		return strings.TrimSpace(string(output))
	}
	write := func(t *testing.T, path, text string) {
		t.Helper()
		if err := os.WriteFile(path, []byte(text), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	git(t, directory, "init", "--template=", "--object-format=sha1", "--initial-branch=main", main)
	git(t, main, "config", "core.autocrlf", "false")
	write(t, filepath.Join(main, "file.txt"), "one\nbase\nthree\n")
	git(t, main, "add", "--", "file.txt")
	git(t, main, "commit", "-m", "Initial commit")
	commit := git(t, main, "rev-parse", "HEAD")
	git(t, main, "tag", "fixture")
	git(t, main, "config", "core.repositoryformatversion", "1")
	git(t, main, "config", "extensions.worktreeConfig", "true")
	git(t, main, "worktree", "add", "-b", "linked", "--", linked, commit)
	common := filepath.Join(main, ".git")
	if err := os.MkdirAll(filepath.Join(common, "info"), 0o700); err != nil {
		t.Fatal(err)
	}
	write(t, filepath.Join(common, "info", "exclude"), "ignored.txt\n")
	options := workspacegit.Options{Environment: environment}
	for _, worktree := range []struct{ branch, root string }{{"main", main}, {"linked", linked}} {
		t.Run(worktree.branch, func(t *testing.T) {
			// Each worktree has its own index and remote config. The remote is the
			// checkout itself, so tag listing needs neither network nor another mount.
			root := worktree.root
			git(t, root, "config", "--worktree", "remote.self.url", ".")
			path := filepath.Join(root, "file.txt")
			staged, edited := worktree.branch+" index", worktree.branch+" edited"
			write(t, path, "one\n"+staged+"\nthree\n")
			git(t, root, "add", "--", "file.txt")
			write(t, path, "one\n"+edited+"\nthree\n")
			write(t, filepath.Join(root, "new.txt"), "new file\n")
			write(t, filepath.Join(root, "ignored.txt"), "ignored file\n")
			t.Run("reads", func(t *testing.T) {
				checkWorktreeConfigReads(t, options, root, worktree.branch, commit, staged, edited)
			})
			t.Run("watch", func(t *testing.T) {
				admin := filepath.FromSlash(git(t, root, "rev-parse", "--absolute-git-dir"))
				ref := "refs/heads/watched-" + worktree.branch
				checkWorktreeConfigWatch(t, options, root, admin, filepath.Join(common, filepath.FromSlash(ref)), func() {
					git(t, root, "update-ref", ref, commit)
				})
			})
		})
	}
}

func checkWorktreeConfigReads(t *testing.T, options workspacegit.Options, root, branch, commit, staged, edited string) {
	t.Helper()
	ctx := t.Context()
	service := workspacegit.New(options)
	path := "file.txt"
	head, err := service.Head(ctx, root)
	if err != nil || head.Kind != protocol.RepositoryKindReady || head.Head == nil || *head.Head != commit || head.Branch == nil || *head.Branch != branch {
		t.Fatalf("HEAD = %+v, %v", head, err)
	}
	resolved, err := service.Read(ctx, root, protocol.Command{Kind: "resolve_git_revision", Data: protocol.CommandResolveGitRevisionPayload{Revision: "HEAD"}})
	revision, ok := resolved.Data.(protocol.OutputGitRevisionPayload)
	if err != nil || !ok || revision.Commit != commit {
		t.Fatalf("resolve HEAD = %+v, %v", resolved, err)
	}
	log, err := service.Log(ctx, root, protocol.CommandReadGitLogPayload{Path: &path, Limit: 10})
	if err != nil || len(log.Entries) != 1 || log.Entries[0].ID != commit {
		t.Fatalf("history = %+v, %v", log, err)
	}
	patch, err := service.Commit(ctx, root, commit)
	if err != nil || patch.Entry.ID != commit || len(patch.Files) != 1 || patch.Files[0].Path != path || patch.Files[0].Additions != 3 {
		t.Fatalf("commit = %+v, %v", patch, err)
	}
	shown, err := service.Show(ctx, root, protocol.CommandReadGitShowPayload{Revision: commit, Path: &path, Limit: 3})
	file, ok := shown.Data.(protocol.GitFile)
	if err != nil || !ok || file.Text != "1: one\n2: base\n3: three\n" {
		t.Fatalf("show file = %+v, %v", shown, err)
	}
	blame, err := service.Blame(ctx, root, protocol.CommandReadGitBlamePayload{Path: path, Offset: 1, Limit: 1})
	if err != nil || len(blame.Entries) != 1 || blame.Entries[0].Commit != commit || blame.Entries[0].OriginalLine != 2 {
		t.Fatalf("blame = %+v, %v", blame, err)
	}
	diff, err := service.Diff(ctx, root, path, protocol.GitDiffScopeUnstaged)
	if err != nil || !strings.Contains(diff.Text, "-"+staged+"\n+"+edited+"\n") {
		t.Fatalf("worktree diff = %+v, %v", diff, err)
	}
	tags, err := service.RemoteTags(ctx, root, "self")
	if err != nil || len(tags) != 1 || tags[0].Name != "fixture" || tags[0].Commit != commit {
		t.Fatalf("remote tags = %+v, %v", tags, err)
	}

	target := protocol.GitChangeTarget{Kind: "worktree", Data: protocol.GitChangeTargetWorktreePayload{Scope: protocol.GitDiffScopeUnstaged}}
	page, err := service.Changes(ctx, root, protocol.CommandListGitChangesPayload{Target: &target, Limit: 1})
	if err != nil || page.Next == nil || len(page.Entries) != 1 || page.Entries[0].Path != path {
		t.Fatalf("change inventory = %+v, %v", page, err)
	}
	state := page.Snapshot.Target.Data.(protocol.GitChangeStateWorktreePayload)
	if err := service.Check(ctx, root, state.IndexRevision, &commit, &branch); err == nil || !strings.Contains(err.Error(), "worktreeconfig") {
		t.Fatalf("write precondition unexpectedly accepted the extension: %v", err)
	}
	// Restore a snapshot on a fresh service so the first read cannot use the cache.
	service = workspacegit.New(options)
	rest, err := service.Changes(ctx, root, protocol.CommandListGitChangesPayload{Cursor: page.Next, Limit: 10})
	if err != nil || rest.Next != nil || len(rest.Entries) != 1 || rest.Entries[0].Path != "new.txt" {
		t.Fatalf("change inventory continuation = %+v, %v", rest, err)
	}
	metadata := page.Entries[0].MetadataRevision
	change, err := service.Change(ctx, root, protocol.CommandReadGitChangePayload{Snapshot: page.Snapshot, Path: path, ExpectedMetadataRevision: metadata, Limit: 10})
	if err != nil || change.Revision == "" || !slices.ContainsFunc(change.Lines, func(line protocol.GitChangeLine) bool {
		return line.Kind == protocol.GitChangeLineKindAdded && line.Text == edited
	}) {
		t.Fatalf("change = %+v, %v", change, err)
	}
	contextLines, err := service.Context(ctx, root, protocol.CommandReadGitContextPayload{
		Snapshot: page.Snapshot, Path: path, ExpectedMetadataRevision: metadata, ExpectedRevision: change.Revision, Side: protocol.GitContextSideOld, Offset: 1, Limit: 1,
	})
	if err != nil || contextLines.Text != staged+"\n" {
		t.Fatalf("old context = %+v, %v", contextLines, err)
	}
}

func checkWorktreeConfigWatch(t *testing.T, options workspacegit.Options, root, admin, refPath string, changeRef func()) {
	t.Helper()
	watches := newWorkspaceWatches(workspacegit.New(options))
	t.Cleanup(func() { _ = watches.close() })
	open, stop := context.WithCancel(t.Context())
	var seenRef atomic.Bool
	events, err := watches.observe(open, root, protocol.NodeID{}, protocol.WorktreeID{1}, true, func(paths []string) {
		if slices.Contains(paths, refPath) {
			seenRef.Store(true)
		}
	})
	stop()
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = events.Close() })
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	next := func() protocol.UpdateFilesChangedPayload {
		t.Helper()
		update, err := events.Next(ctx)
		payload, ok := update.Data.(protocol.UpdateFilesChangedPayload)
		if err != nil || update.Kind != "files_changed" || !ok {
			t.Fatalf("workspace update = %+v, %v", update, err)
		}
		return payload
	}
	initial := next()
	active := events.(*workspaceEvents).active
	active.mu.Lock()
	active.filter.Changed(nil)
	ignored := !active.filter.Changed([]string{filepath.Join(root, "ignored.txt")})
	index := []string{filepath.Join(admin, "index")}
	indexOnly := active.filter.Changed(index) && !active.filter.ReferencesChanged(index)
	active.mu.Unlock()
	if !ignored || !indexOnly {
		t.Fatalf("refreshed filter ignored file = %t, index-only invalidation = %t", ignored, indexOnly)
	}
	changeRef()
	for {
		if update := next(); seenRef.Load() && update.References > initial.References {
			break
		}
	}
}
