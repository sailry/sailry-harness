package git

import (
	"context"
	"path/filepath"
	"reflect"
	"strings"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/config"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type worktreeRevision struct {
	index        string
	head, branch *string
}

type fetchSlot struct {
	token chan struct{}
	users int
}

// FETCH_HEAD belongs to a worktree, not a remote. Fetch and pull share this
// lock so another Sailry transfer cannot replace it while native pull reads it.
// Acquire it before the root lock; waiting for a transfer never blocks edits.
func (s *Service) lockFetch(ctx context.Context, r *gogit.Repository) (func(), error) {
	dir, err := gitDir("", r)
	if err != nil {
		return nil, err
	}
	dir, err = filepath.EvalSymlinks(dir)
	if err != nil {
		return nil, mapError(err)
	}
	s.mu.Lock()
	slot := s.fetches[dir]
	if slot == nil {
		slot = &fetchSlot{token: make(chan struct{}, 1)}
		s.fetches[dir] = slot
	}
	slot.users++
	s.mu.Unlock()
	release := func() {
		s.mu.Lock()
		slot.users--
		if slot.users == 0 {
			delete(s.fetches, dir)
		}
		s.mu.Unlock()
	}
	select {
	case slot.token <- struct{}{}:
		return func() { <-slot.token; release() }, nil
	case <-ctx.Done():
		release()
		return nil, mapError(ctx.Err())
	}
}

type pullTarget struct {
	args   []string
	branch *config.Branch
	remote *config.RemoteConfig
}

func capturePull(r *gogit.Repository, branch string, target []string) (pullTarget, error) {
	cfg, err := r.ConfigScoped(config.GlobalScope)
	if err != nil {
		return pullTarget{}, mapError(err)
	}
	result := pullTarget{args: append([]string(nil), target...), branch: cfg.Branches[branch]}
	remote := "origin"
	if result.branch != nil && result.branch.Remote != "" {
		remote = result.branch.Remote
	}
	configuredRemote := remote
	if len(target) > 0 {
		remote = target[0]
	}
	result.remote = cfg.Remotes[remote]
	if len(target) < 2 && result.branch != nil && configuredRemote == remote && result.branch.Merge != "" {
		// Keep every configured merge ref for native merge/octopus semantics.
		merges := cfg.Raw.Section("branch").Subsection(branch).OptionAll("merge")
		if len(merges) == 0 {
			merges = []string{result.branch.Merge.String()}
		}
		result.args = append([]string{remote}, merges...)
	}
	return result, nil
}

// The first fetch transfers without the root lock. Recheck the admitted
// worktree and target under that lock before native pull performs its own
// negotiation and merge/rebase, preserving Git's rebase/fork-point behavior.
func (s *Service) pull(ctx context.Context, root, flag string, expected worktreeRevision, target ...string) error {
	r, err := open(root)
	if err != nil {
		return err
	}
	captured, err := capturePull(r, valueOr(expected.branch), target)
	if err != nil {
		return err
	}
	unlock, err := s.lockFetch(ctx, r)
	if err != nil {
		return err
	}
	defer unlock()
	if _, err := s.network(ctx, root, append([]string{"fetch"}, captured.args...)); err != nil {
		return err
	}
	defer s.lock(root)()
	if err := s.Check(ctx, root, expected.index, expected.head, expected.branch); err != nil {
		return err
	}
	r, err = open(root)
	if err != nil {
		return err
	}
	current, err := capturePull(r, valueOr(expected.branch), target)
	if err != nil {
		return err
	}
	if !reflect.DeepEqual(captured, current) {
		return fault(protocol.ErrorCodeRevisionConflict, "Git pull target changed; review before retrying")
	}
	_, err = s.network(ctx, root, append([]string{"pull", flag, "--no-edit"}, captured.args...))
	return err
}

// Resolve the selected branch's push destination with native Git so renamed
// upstreams and triangular workflows keep their configured destination. The
// explicit source ref remains the selected branch after the pull releases its
// root lock, even if another operation switches the worktree before push.
func (s *Service) branchPush(ctx context.Context, root, branch string) ([]string, error) {
	if err := branchName(branch); err != nil {
		return nil, err
	}
	ref := plumbing.NewBranchReferenceName(branch).String()
	result, err := s.command(ctx, root, []string{"for-each-ref", "--format=%(push:remotename)%00%(push:remoteref)%00%(push)%00%(upstream:remotename)", ref}, nil, true)
	if err != nil {
		return nil, err
	}
	fields := strings.Split(strings.TrimSuffix(string(result.Stdout), "\n"), "\x00")
	if len(fields) != 4 {
		return nil, fault(protocol.ErrorCodeNotConfigured, "Git branch push destination is not configured")
	}
	remote, destination, tracking, upstream := fields[0], fields[1], fields[2], fields[3]
	if destination == "" {
		r, err := open(root)
		if err != nil {
			return nil, err
		}
		cfg, err := r.ConfigScoped(config.GlobalScope)
		if err != nil {
			return nil, mapError(err)
		}
		// The remotename atom is empty when Git uses an implicit remote.
		if remote == "" {
			remote = "origin"
			if len(cfg.Remotes) == 1 {
				for name := range cfg.Remotes {
					remote = name
				}
			}
		}
		if target := cfg.Remotes[remote]; target != nil {
			// remoteref only reports an explicit remote.push refspec. Native
			// Git resolves push.default via its local tracking ref instead.
			for _, fetch := range target.Fetch {
				reverse := config.RefSpec(strings.TrimPrefix(fetch.String(), "+")).Reverse()
				if tracking != "" && reverse.Match(plumbing.ReferenceName(tracking)) {
					destination = reverse.Dst(plumbing.ReferenceName(tracking)).String()
					break
				}
			}
			if destination == "" && upstream != "" && upstream != remote && len(cfg.Raw.Section("remote").Subsection(remote).OptionAll("push")) == 0 {
				// @{push} cannot resolve simple's triangular case, although
				// native push sends the same-named branch to the push remote.
				mode, err := s.command(ctx, root, []string{"config", "--default=simple", "--get", "push.default"}, nil, true)
				if err != nil {
					return nil, err
				}
				if strings.TrimSpace(string(mode.Stdout)) == "simple" {
					destination = ref
				}
			}
		}
	}
	if name(remote) != nil || !strings.HasPrefix(destination, "refs/") || plumbing.ReferenceName(destination).Validate() != nil {
		return nil, fault(protocol.ErrorCodeNotConfigured, "Git branch push destination is not configured")
	}
	return []string{"push", remote, ref + ":" + destination}, nil
}
