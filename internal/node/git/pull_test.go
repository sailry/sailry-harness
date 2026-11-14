package git

import (
	"context"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/config"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func configurePullUpstream(t *testing.T, r *gogit.Repository) {
	t.Helper()
	cfg, err := r.Config()
	if err != nil {
		t.Fatal(err)
	}
	cfg.Remotes["origin"] = &config.RemoteConfig{Name: "origin", URLs: []string{"https://example.invalid/repository.git"}}
	cfg.Branches["main"] = &config.Branch{Name: "main", Remote: "origin", Merge: plumbing.NewBranchReferenceName("main")}
	if err := r.SetConfig(cfg); err != nil {
		t.Fatal(err)
	}
}

func pullRequest(t *testing.T, root string, r *gogit.Repository, commit, kind string) protocol.CommandRunGitActionPayload {
	t.Helper()
	index, err := indexRevision(root, r)
	if err != nil {
		t.Fatal(err)
	}
	action := protocol.GitAction{Kind: kind, Data: protocol.GitActionPullPayload{Rebase: true}}
	if kind == "sync" {
		action.Data = protocol.GitActionSyncPayload{Rebase: true}
	}
	return protocol.CommandRunGitActionPayload{Action: action, ExpectedIndex: index, ExpectedHead: &commit, ExpectedBranch: ptr("main")}
}

func TestPullRejectsChangedWorktree(t *testing.T) {
	for _, kind := range []string{"pull", "sync"} {
		for _, change := range []string{"branch", "head", "index", "upstream", "remote"} {
			t.Run(kind+"/"+change, func(t *testing.T) {
				root, r, commit := fixture(t)
				configurePullUpstream(t, r)
				input := pullRequest(t, root, r, commit, kind)
				locked, fetched := false, false
				s := New(Options{Lock: func(string) func() {
					locked = true
					return func() { locked = false }
				}, Run: func(_ context.Context, _ string, args []string, _ []byte) (CommandResult, error) {
					if args[0] == "for-each-ref" {
						return CommandResult{Stdout: []byte("origin\x00refs/heads/main\x00\x00origin\n")}, nil
					}
					if args[0] != "fetch" || locked || fetched {
						t.Fatalf("unexpected native command %q (locked=%v, fetched=%v)", args, locked, fetched)
					}
					fetched = true
					switch change {
					case "branch":
						if err := r.Storer.SetReference(plumbing.NewHashReference("refs/heads/other", plumbing.NewHash(commit))); err != nil {
							t.Fatal(err)
						}
						if err := r.Storer.SetReference(plumbing.NewSymbolicReference(plumbing.HEAD, "refs/heads/other")); err != nil {
							t.Fatal(err)
						}
					case "head":
						w, err := r.Worktree()
						if err != nil {
							t.Fatal(err)
						}
						if _, err := w.Commit("Concurrent commit", &gogit.CommitOptions{AllowEmptyCommits: true, Author: &object.Signature{Name: "Fixture", Email: "fixture@example.test", When: time.Unix(200, 0)}}); err != nil {
							t.Fatal(err)
						}
					case "index":
						if err := os.WriteFile(filepath.Join(root, "staged.txt"), []byte("concurrent staging\n"), 0644); err != nil {
							t.Fatal(err)
						}
						w, err := r.Worktree()
						if err != nil {
							t.Fatal(err)
						}
						if _, err := w.Add("staged.txt"); err != nil {
							t.Fatal(err)
						}
					default:
						cfg, err := r.Config()
						if err != nil {
							t.Fatal(err)
						}
						if change == "upstream" {
							cfg.Branches["main"].Merge = "refs/heads/other"
						} else {
							cfg.Remotes["origin"].URLs = []string{"https://example.invalid/other.git"}
						}
						if err := r.SetConfig(cfg); err != nil {
							t.Fatal(err)
						}
					}
					return CommandResult{}, nil
				}})
				requireFault(t, s.Action(context.Background(), root, input), protocol.ErrorCodeRevisionConflict)
				if locked || !fetched {
					t.Fatalf("lock=%v fetch=%v", locked, fetched)
				}
			})
		}
	}
}

func TestCommitFollowUpUsesCommittedBranch(t *testing.T) {
	for _, after := range []protocol.GitAfterCommit{protocol.GitAfterCommitSync, protocol.GitAfterCommitPush} {
		for _, changed := range []bool{false, true} {
			t.Run(string(after)+"/changed="+map[bool]string{false: "false", true: "true"}[changed], func(t *testing.T) {
				root, r, commit := fixture(t)
				configurePullUpstream(t, r)
				action := pullRequest(t, root, r, commit, "pull")
				input := protocol.CommandCreateGitCommitPayload{Message: "Change", ExpectedIndex: action.ExpectedIndex, ExpectedHead: action.ExpectedHead, ExpectedBranch: action.ExpectedBranch}
				input.Options.After, input.Options.Tracked = after, true
				var committed string
				var ran []string
				locked := false
				switchBranch := func() error {
					if err := r.Storer.SetReference(plumbing.NewHashReference("refs/heads/other", plumbing.NewHash(commit))); err != nil {
						return err
					}
					return r.Storer.SetReference(plumbing.NewSymbolicReference(plumbing.HEAD, "refs/heads/other"))
				}
				s := New(Options{Lock: func(string) func() {
					locked = true
					return func() {
						locked = false
						// A switch between commit and a push-only follow-up must
						// not change which branch the push sends.
						if changed && after == protocol.GitAfterCommitPush {
							if err := switchBranch(); err != nil {
								t.Fatal(err)
							}
						}
					}
				}, Run: func(_ context.Context, _ string, args []string, _ []byte) (CommandResult, error) {
					ran = append(ran, args[0])
					switch args[0] {
					case "commit":
						if !locked {
							t.Fatal("commit must hold the root lock")
						}
						if err := os.WriteFile(filepath.Join(root, "file.txt"), []byte("commit --all changed the index\n"), 0644); err != nil {
							return CommandResult{}, err
						}
						w, err := r.Worktree()
						if err != nil {
							return CommandResult{}, err
						}
						if _, err = w.Add("file.txt"); err != nil {
							return CommandResult{}, err
						}
						hash, err := w.Commit("Change", &gogit.CommitOptions{Author: &object.Signature{Name: "Fixture", Email: "fixture@example.test", When: time.Unix(200, 0)}})
						committed = hash.String()
						return CommandResult{}, err
					case "for-each-ref":
						if args[len(args)-1] != "refs/heads/main" {
							t.Fatalf("follow-up selected a different branch: %q", args)
						}
						return CommandResult{Stdout: []byte("origin\x00refs/heads/main\x00\x00origin\n")}, nil
					case "fetch":
						if locked {
							t.Fatal("prefetch held the root lock")
						}
						if changed {
							return CommandResult{}, switchBranch()
						}
					case "pull":
						if !locked || changed {
							t.Fatalf("pull lock=%v after conflict=%v", locked, changed)
						}
					case "push":
						if locked || !reflect.DeepEqual(args, []string{"push", "--progress", "origin", "refs/heads/main:refs/heads/main"}) {
							t.Fatalf("push lock=%v args=%q", locked, args)
						}
					}
					return CommandResult{}, nil
				}})
				result, err := s.CreateCommit(context.Background(), root, input)
				if err != nil || result.ID != committed || committed == commit {
					t.Fatalf("commit result=%+v error=%v committed=%s", result, err, committed)
				}
				if changed && after == protocol.GitAfterCommitSync {
					requireFault(t, result.FollowUp, protocol.ErrorCodeRevisionConflict)
					if !reflect.DeepEqual(ran, []string{"commit", "for-each-ref", "fetch"}) {
						t.Fatalf("conflicted follow-up ran %q", ran)
					}
				} else if result.FollowUp != nil || ran[len(ran)-1] != "push" {
					t.Fatalf("follow-up=%+v ran=%q", result.FollowUp, ran)
				}
			})
		}
	}
}

// Done is first observed by lockFetch: revision checks only inspect Err. This
// lets the test rendezvous at the competing fetch's lock without timed sleeps.
type fetchWaitingContext struct {
	context.Context
	waiting chan struct{}
	once    sync.Once
}

func (c *fetchWaitingContext) Done() <-chan struct{} {
	c.once.Do(func() { close(c.waiting) })
	return c.Context.Done()
}

func TestPullSerializesFetchHead(t *testing.T) {
	root, r, commit := fixture(t)
	configurePullUpstream(t, r)
	input := pullRequest(t, root, r, commit, "pull")
	inPull, finishPull := make(chan struct{}), make(chan struct{})
	var release sync.Once
	defer release.Do(func() { close(finishPull) })
	var fetches atomic.Int32
	var rootLock sync.Mutex
	s := New(Options{Lock: func(string) func() { rootLock.Lock(); return rootLock.Unlock }, Run: func(_ context.Context, _ string, args []string, _ []byte) (CommandResult, error) {
		switch args[0] {
		case "fetch":
			fetches.Add(1)
		case "pull":
			close(inPull)
			<-finishPull
		}
		return CommandResult{}, nil
	}})
	pulled := make(chan error, 1)
	go func() { pulled <- s.Action(context.Background(), root, input) }()
	select {
	case <-inPull:
	case err := <-pulled:
		t.Fatalf("pull stopped before integration: %v", err)
	}
	waiter := &fetchWaitingContext{Context: context.Background(), waiting: make(chan struct{})}
	fetched := make(chan error, 1)
	fetch := input
	fetch.Action = protocol.GitAction{Kind: "fetch", Data: protocol.GitActionFetchPayload{All: true}}
	go func() { fetched <- s.Action(waiter, root, fetch) }()
	select {
	case <-waiter.waiting:
	case err := <-fetched:
		t.Fatalf("fetch ran before pull finished reading FETCH_HEAD: %v", err)
	}
	if got := fetches.Load(); got != 1 {
		t.Fatalf("concurrent native fetch calls=%d", got)
	}
	// Cancellation while queued must also release its slot without calling Git.
	cancelled, cancel := context.WithCancel(context.Background())
	cancel()
	_, err := s.lockFetch(cancelled, r)
	requireFault(t, err, protocol.ErrorCodeCancelled)
	release.Do(func() { close(finishPull) })
	if err := <-pulled; err != nil {
		t.Fatal(err)
	}
	if err := <-fetched; err != nil {
		t.Fatal(err)
	}
	if fetches.Load() != 2 || len(s.fetches) != 0 {
		t.Fatalf("fetches=%d retained locks=%d", fetches.Load(), len(s.fetches))
	}
}

func TestSyncPushKeepsSelectedBranch(t *testing.T) {
	root, r, commit := fixture(t)
	configurePullUpstream(t, r)
	input := pullRequest(t, root, r, commit, "sync")
	var pushed []string
	s := New(Options{Lock: func(string) func() {
		return func() {
			if err := r.Storer.SetReference(plumbing.NewHashReference("refs/heads/other", plumbing.NewHash(commit))); err != nil {
				t.Fatal(err)
			}
			if err := r.Storer.SetReference(plumbing.NewSymbolicReference(plumbing.HEAD, "refs/heads/other")); err != nil {
				t.Fatal(err)
			}
		}
	}, Run: func(_ context.Context, _ string, args []string, _ []byte) (CommandResult, error) {
		if args[0] == "for-each-ref" {
			return CommandResult{Stdout: []byte("publish\x00refs/heads/review\x00\x00origin\n")}, nil
		}
		if args[0] == "push" {
			pushed = args
		}
		return CommandResult{}, nil
	}})
	if err := s.Action(context.Background(), root, input); err != nil {
		t.Fatal(err)
	}
	if strings.Join(pushed, " ") != "push --progress publish refs/heads/main:refs/heads/review" {
		t.Fatalf("push followed a switched branch: %q", pushed)
	}
}

func TestPushKeepsSelectedBranch(t *testing.T) {
	for _, probe := range []struct {
		name  string
		force bool
	}{
		{name: "normal"},
		{name: "force", force: true},
	} {
		t.Run(probe.name, func(t *testing.T) {
			root, r, commit := fixture(t)
			configurePullUpstream(t, r)
			input := pullRequest(t, root, r, commit, "pull")
			input.Action = protocol.GitAction{Kind: "push", Data: protocol.GitActionPushPayload{Force: probe.force}}
			var pushed []string
			s := New(Options{Lock: func(string) func() {
				t.Fatal("push must not hold the root lock")
				return func() {}
			}, Run: func(_ context.Context, _ string, args []string, _ []byte) (CommandResult, error) {
				switch args[0] {
				case "for-each-ref":
					if args[len(args)-1] != "refs/heads/main" {
						t.Fatalf("push resolved a different branch: %q", args)
					}
					if err := r.Storer.SetReference(plumbing.NewHashReference("refs/heads/other", plumbing.NewHash(commit))); err != nil {
						return CommandResult{}, err
					}
					if err := r.Storer.SetReference(plumbing.NewSymbolicReference(plumbing.HEAD, "refs/heads/other")); err != nil {
						return CommandResult{}, err
					}
					return CommandResult{Stdout: []byte("origin\x00refs/heads/release\x00\x00origin\n")}, nil
				case "push":
					pushed = args
				default:
					t.Fatalf("unexpected command: %q", args)
				}
				return CommandResult{}, nil
			}})
			if err := s.Action(context.Background(), root, input); err != nil {
				t.Fatal(err)
			}
			want := []string{"push", "--progress"}
			if probe.force {
				want = append(want, "--force-with-lease")
			}
			want = append(want, "origin", "refs/heads/main:refs/heads/release")
			if !reflect.DeepEqual(pushed, want) {
				t.Fatalf("push followed a switched branch: %q want %q", pushed, want)
			}
		})
	}
}
