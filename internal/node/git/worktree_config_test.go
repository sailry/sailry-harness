package git

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestServiceReadWorktreeConfig(t *testing.T) {
	main, repository, commit := fixture(t)
	common := filepath.Join(main, ".git")
	linked, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	admin := filepath.Join(common, "worktrees", "linked")
	if err := os.MkdirAll(admin, 0700); err != nil {
		t.Fatal(err)
	}
	for path, value := range map[string]string{
		filepath.Join(linked, ".git"):     "gitdir: " + filepath.ToSlash(admin) + "\n",
		filepath.Join(admin, "gitdir"):    filepath.ToSlash(filepath.Join(linked, ".git")) + "\n",
		filepath.Join(admin, "commondir"): "../..\n",
		filepath.Join(admin, "HEAD"):      "ref: refs/heads/linked\n",
		filepath.Join(linked, "file.txt"): "one\nlinked\nthree\n",
		filepath.Join(admin, "config.worktree"): "[core]\n\tworktree = " + filepath.ToSlash(linked) + "\n" +
			"[branch \"linked\"]\n\tmerge = refs/heads/actual\n[filter \"Fixture.Driver\"]\n\tclean = fixture-filter-program\n",
	} {
		writeWorktreeReadFixture(t, path, value)
	}
	if err := repository.Storer.SetReference(plumbing.NewHashReference("refs/heads/linked", plumbing.NewHash(commit))); err != nil {
		t.Fatal(err)
	}
	checkout, err := gogit.PlainOpenWithOptions(linked, &gogit.PlainOpenOptions{EnableDotGitCommonDir: true})
	if err != nil {
		t.Fatal(err)
	}
	worktree, err := checkout.Worktree()
	if err != nil {
		t.Fatal(err)
	}
	linkedBlob, err := worktree.Add("file.txt")
	if err != nil {
		t.Fatal(err)
	}
	mainBlob := plumbing.ComputeHash(plumbing.BlobObject, []byte("one\ntwo\nthree\n"))
	mainIndex, err := indexRevision(main, repository)
	if err != nil {
		t.Fatal(err)
	}
	linkedIndex, err := indexRevision(linked, checkout)
	if err != nil || linkedIndex == "absent" || linkedIndex == mainIndex {
		t.Fatal(linkedIndex, err)
	}
	writeWorktreeReadFixture(t, filepath.Join(common, "config"), "[core]\n\trepositoryformatversion = 1\n\tbare = false\n"+
		"[extensions]\n\tworktreeConfig = true\n[branch \"linked\"]\n\tremote = origin\n\tmerge = refs/heads/base\n")

	for _, test := range []struct{ root, gitDir, branch, index string }{
		{main, common, "main", mainIndex},
		{linked, admin, "linked", linkedIndex},
	} {
		t.Run(test.branch, func(t *testing.T) {
			const config = "local\x00core.repositoryformatversion\n1\x00local\x00extensions.worktreeconfig\ntrue\x00" +
				"local\x00branch.linked.remote\norigin\x00local\x00branch.linked.merge\nrefs/heads/base\x00"
			status := "# branch.oid " + commit + "\x00# branch.head " + test.branch + "\x00"
			runner := func(ctx context.Context, root string, args []string, _ []byte) (CommandResult, error) {
				read, _ := ctx.Value(gitReadKey{}).(bool)
				if root != test.root || !read {
					return CommandResult{}, fmt.Errorf("unexpected Git read at %q", root)
				}
				switch {
				case slices.Contains(args, "rev-parse") && args[0] == "--git-dir=.git":
					return CommandResult{Stdout: []byte(test.gitDir + "\n" + common + "\n" + root + "\n")}, nil
				case slices.Contains(args, "config") && args[0] == "--git-dir=.git":
					cfg := config
					if root == linked {
						cfg += "worktree\x00branch.linked.merge\nrefs/heads/actual\x00worktree\x00filter.Fixture.Driver.clean\nfixture-filter-program\x00"
					}
					return CommandResult{Stdout: []byte(cfg)}, nil
				case slices.Equal(args, StatusArgs(nil)):
					if root == linked {
						return CommandResult{ExitCode: 128}, nil
					}
					return CommandResult{Stdout: []byte(status)}, nil
				case slices.Equal(args, StatusArgs([]string{"Fixture.Driver"})) && root == linked:
					return CommandResult{Stdout: fmt.Appendf([]byte(status), "1 M. N... 100644 100644 100644 %s %s file.txt\x00", mainBlob, linkedBlob)}, nil
				case slices.Contains(args, "stash"):
					return CommandResult{}, nil
				default:
					return CommandResult{}, fmt.Errorf("unexpected Git arguments: %v", args)
				}
			}
			s := New(Options{Run: runner})
			out, err := s.Read(context.Background(), test.root, protocol.Command{Kind: "inspect_git"})
			got, ok := out.Data.(protocol.GitStatus)
			if err != nil || out.Kind != "git_status" || !ok || got.Kind != protocol.RepositoryKindReady ||
				!sameString(got.Head, &commit) || !sameString(got.Branch, &test.branch) || !sameString(got.IndexRevision, &test.index) {
				t.Fatalf("inspect_git = %+v, %v", out, err)
			}
			if test.root == linked {
				if len(got.Entries) != 1 {
					t.Fatalf("linked status = %+v", got.Entries)
				}
				entry := got.Entries[0]
				if entry.Path != "file.txt" || entry.Staged == nil || *entry.Staged != protocol.GitChangeKindModified ||
					entry.StagedDiff != (protocol.GitLineStats{Additions: 1, Deletions: 1}) {
					t.Fatalf("linked status = %+v", entry)
				}
			}
			out, err = s.Read(context.Background(), test.root, protocol.Command{Kind: "list_git_branches"})
			branches, ok := out.Data.(protocol.GitBranches)
			if err != nil || out.Kind != "git_branches" || !ok || branches.Kind != protocol.RepositoryKindReady ||
				!sameString(branches.Current, &test.branch) || len(branches.Entries) != 2 || !sameString(branches.Sync.HeadSummary, ptr("Initial commit")) {
				t.Fatalf("list_git_branches = %+v, %v", out, err)
			}
			if test.root == linked && !sameString(branches.Sync.Upstream, ptr("origin/actual")) {
				t.Fatalf("linked upstream = %v", branches.Sync.Upstream)
			}
			if strings.Contains(s.Output(test.root), "fixture-filter-program") {
				t.Fatal("Git output exposed config values")
			}
		})
	}
}

func TestServiceReadWorktreeConfigGuards(t *testing.T) {
	for _, name := range []string{"unsupported_extension", "outside_worktree", "bare_worktree", "invalid_marker", "changed_HEAD", "changed_index"} {
		t.Run(name, func(t *testing.T) {
			root, _, commit := fixture(t)
			admin := filepath.Join(root, ".git")
			config := "[core]\n\trepositoryformatversion = 0\n[extensions]\n\tworktreeConfig = true\n"
			top := root
			switch name {
			case "unsupported_extension":
				config = "[core]\n\trepositoryformatversion = 1\n[extensions]\n\tworktreeConfig = true\n\trefStorage = reftable\n"
			case "outside_worktree":
				top = t.TempDir()
				writeWorktreeReadFixture(t, filepath.Join(admin, "config.worktree"), "[core]\n\tworktree = "+filepath.ToSlash(top)+"\n")
			case "bare_worktree":
				writeWorktreeReadFixture(t, filepath.Join(admin, "config.worktree"), "[core]\n\tbare = true\n")
			case "invalid_marker":
				root = filepath.Join(root, "nested")
				admin = filepath.Join(root, ".git")
				if err := os.MkdirAll(admin, 0700); err != nil {
					t.Fatal(err)
				}
			}
			writeWorktreeReadFixture(t, filepath.Join(admin, "config"), config)
			s := New(Options{Run: func(_ context.Context, _ string, args []string, _ []byte) (CommandResult, error) {
				switch {
				case name == "bare_worktree" && slices.Contains(args, "rev-parse"):
					return CommandResult{ExitCode: 128}, nil
				case name != "unsupported_extension" && name != "invalid_marker" && slices.Contains(args, "rev-parse"):
					return CommandResult{Stdout: []byte(admin + "\n" + admin + "\n" + top + "\n")}, nil
				case name == "outside_worktree" || name == "bare_worktree" || name == "unsupported_extension" || name == "invalid_marker":
					return CommandResult{}, fmt.Errorf("unexpected Git command: %v", args)
				case slices.Contains(args, "config"):
					return CommandResult{Stdout: []byte("local\x00core.repositoryformatversion\n0\x00local\x00extensions.worktreeconfig\ntrue\x00")}, nil
				case slices.Equal(args, StatusArgs(nil)):
					var err error
					if name == "changed_HEAD" {
						err = os.WriteFile(filepath.Join(admin, "HEAD"), []byte("ref: refs/heads/unborn\n"), 0600)
					} else {
						err = os.Remove(filepath.Join(admin, "index"))
					}
					return CommandResult{Stdout: []byte("# branch.oid " + commit + "\x00# branch.head main\x00")}, err
				default:
					return CommandResult{}, fmt.Errorf("unexpected Git arguments: %v", args)
				}
			}})
			switch name {
			case "unsupported_extension", "outside_worktree", "bare_worktree", "invalid_marker":
				for _, kind := range []string{"inspect_git", "list_git_branches"} {
					_, err := s.Read(context.Background(), root, protocol.Command{Kind: kind})
					switch name {
					case "unsupported_extension":
						requireFault(t, err, protocol.ErrorCodeUnavailable)
						if !strings.Contains(err.Error(), "refstorage") {
							t.Fatal(err)
						}
					case "invalid_marker":
						requireFault(t, err, protocol.ErrorCodeNotFound)
					default:
						requireFault(t, err, protocol.ErrorCodeInvalidRequest)
					}
				}
			default:
				_, err := s.Read(context.Background(), root, protocol.Command{Kind: "inspect_git"})
				requireFault(t, err, protocol.ErrorCodeConflict)
			}
		})
	}
}

func writeWorktreeReadFixture(t *testing.T, path, value string) {
	t.Helper()
	if err := os.WriteFile(path, []byte(value), 0600); err != nil {
		t.Fatal(err)
	}
}
