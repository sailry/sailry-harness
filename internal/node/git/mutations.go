package git

import (
	"context"
	"errors"
	"path/filepath"
	"strings"

	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/storage/filesystem"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (s *Service) CreateCommit(ctx context.Context, root string, input protocol.CommandCreateGitCommitPayload) (protocol.OutputGitCommitCreatedPayload, error) {
	result := protocol.OutputGitCommitCreatedPayload{}
	if strings.TrimSpace(input.Message) == "" || len(input.Message) > 16*1024 || strings.ContainsRune(input.Message, 0) {
		return result, invalid("invalid Git commit message")
	}
	// The commit holds the root lock; its push or sync runs after it.
	unlock := s.lock(root)
	locked := true
	release := func() {
		if locked {
			locked = false
			unlock()
		}
	}
	defer release()
	if err := s.Check(ctx, root, input.ExpectedIndex, input.ExpectedHead, input.ExpectedBranch); err != nil {
		return result, err
	}
	args := []string{"commit", "--message", input.Message}
	if input.Amend {
		args = append(args, "--amend")
	}
	if input.Options.Signoff {
		args = append(args, "--signoff")
	}
	if input.Options.SkipHooks {
		args = append(args, "--no-verify")
	}
	if input.Options.All {
		if _, err := s.command(ctx, root, []string{"add", "--all"}, nil, false); err != nil {
			return result, err
		}
	}
	if input.Options.Tracked {
		args = append(args, "--all")
	}
	defer s.Invalidate(root)
	if _, err := s.command(ctx, root, args, nil, false); err != nil {
		return result, err
	}
	r, err := open(root)
	if err != nil {
		return result, fault(protocol.ErrorCodeOutcomeUnknown, "Git commit result could not be confirmed")
	}
	h, err := head(r)
	if err != nil || h.Head == nil {
		return result, fault(protocol.ErrorCodeOutcomeUnknown, "Git commit result could not be confirmed")
	}
	result.ID = *h.Head
	release()
	follow := func() error {
		if input.Options.After == protocol.GitAfterCommitNone {
			return nil
		}
		if input.Options.PushRemote != nil {
			if input.ExpectedBranch == nil {
				return invalid("publishing requires an attached branch")
			}
			if _, err := r.Remote(*input.Options.PushRemote); err != nil {
				return mapError(err)
			}
			_, err := s.network(ctx, root, []string{"push", "--set-upstream", *input.Options.PushRemote, "HEAD:refs/heads/" + *input.ExpectedBranch})
			return err
		}
		if input.Options.After == protocol.GitAfterCommitSync {
			if err := s.pull(ctx, root, "--no-rebase"); err != nil {
				return err
			}
		}
		_, err := s.network(ctx, root, []string{"push"})
		return err
	}
	if err := follow(); err != nil {
		var value *protocol.Fault
		if errors.As(err, &value) {
			result.FollowUp = value
		} else {
			result.FollowUp = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Git follow-up failed"}
		}
	}
	return result, nil
}
func (s *Service) CreateBranch(ctx context.Context, root, name, commit string) (protocol.GitBranch, error) {
	if err := branchName(name); err != nil {
		return protocol.GitBranch{}, err
	}
	if err := fullHash(commit); err != nil {
		return protocol.GitBranch{}, err
	}
	r, err := open(root)
	if err != nil {
		return protocol.GitBranch{}, err
	}
	release, err := lockReferences(r, []string{"refs/heads/" + name})
	if err != nil {
		return protocol.GitBranch{}, err
	}
	defer release()
	if _, err := r.CommitObject(plumbing.NewHash(commit)); err != nil {
		return protocol.GitBranch{}, mapError(err)
	}
	ref := plumbing.NewBranchReferenceName(name)
	if _, err := r.Reference(ref, false); err == nil {
		return protocol.GitBranch{}, fault(protocol.ErrorCodeConflict, BranchExists)
	}
	if err := checkContext(ctx); err != nil {
		return protocol.GitBranch{}, err
	}
	if err := r.Storer.CheckAndSetReference(plumbing.NewHashReference(ref, plumbing.NewHash(commit)), nil); err != nil {
		return protocol.GitBranch{}, mapError(err)
	}
	s.Invalidate(root)
	return protocol.GitBranch{Name: name, Commit: commit}, nil
}
func (s *Service) RenameBranch(ctx context.Context, root, name, newName, commit string) (protocol.GitBranch, error) {
	if err := branchName(newName); err != nil {
		return protocol.GitBranch{}, err
	}
	r, err := open(root)
	if err != nil {
		return protocol.GitBranch{}, err
	}
	release, err := lockReferences(r, []string{"HEAD", "refs/heads/" + name, "refs/heads/" + newName, "config", "packed-refs"})
	if err != nil {
		return protocol.GitBranch{}, err
	}
	defer release()
	if err := checkBranch(r, name, commit); err != nil {
		return protocol.GitBranch{}, err
	}
	h, err := head(r)
	if err != nil {
		return protocol.GitBranch{}, err
	}
	result := protocol.GitBranch{Name: newName, Commit: commit, Current: h.Branch != nil && *h.Branch == name}
	if name == newName {
		return result, nil
	}
	if _, err := r.Reference(plumbing.NewBranchReferenceName(newName), false); err == nil {
		return result, fault(protocol.ErrorCodeConflict, BranchExists)
	}
	if err := s.branchAvailable(ctx, root, name, true); err != nil {
		return result, err
	}
	if err := checkContext(ctx); err != nil {
		return result, err
	}
	unknown := func() error {
		return fault(protocol.ErrorCodeOutcomeUnknown, "branch rename may have changed refs or configuration; inspect the repository before retrying")
	}
	if err := r.Storer.SetReference(plumbing.NewHashReference(plumbing.NewBranchReferenceName(newName), plumbing.NewHash(commit))); err != nil {
		return result, unknown()
	}
	if result.Current {
		if err := r.Storer.SetReference(plumbing.NewSymbolicReference(plumbing.HEAD, plumbing.NewBranchReferenceName(newName))); err != nil {
			return result, unknown()
		}
	}
	config, err := r.Config()
	if err != nil {
		return result, unknown()
	}
	if branch := config.Branches[name]; branch != nil {
		delete(config.Branches, name)
		branch.Name = newName
		config.Branches[newName] = branch
		if err := r.Storer.SetConfig(config); err != nil {
			return result, unknown()
		}
	}
	if store, ok := r.Storer.(*filesystem.Storage); ok {
		fs := store.Filesystem()
		oldLog := "logs/refs/heads/" + name
		newLog := "logs/refs/heads/" + newName
		if _, e := fs.Lstat(oldLog); e == nil {
			if e := fs.MkdirAll(filepath.Dir(newLog), 0755); e != nil {
				return result, unknown()
			}
			if e := fs.Rename(oldLog, newLog); e != nil {
				return result, unknown()
			}
		}
	}
	if err := r.Storer.RemoveReference(plumbing.NewBranchReferenceName(name)); err != nil {
		return result, unknown()
	}
	s.Invalidate(root)
	return result, nil
}
func (s *Service) branchAvailable(ctx context.Context, root, name string, allowCurrent bool) error {
	worktrees, err := s.ListWorktrees(ctx, root)
	if err != nil {
		return err
	}
	for _, entry := range worktrees.Entries {
		if entry.Branch != nil && *entry.Branch == name && (!allowCurrent || entry.Path != root) {
			return fault(protocol.ErrorCodeConflict, BranchInWorktree)
		}
	}
	return nil
}
func (s *Service) DeleteBranch(ctx context.Context, root string, input protocol.CommandDeleteGitBranchPayload) error {
	r, err := open(root)
	if err != nil {
		return err
	}
	names := []string{"HEAD", "refs/heads/" + input.Name, "config", "packed-refs"}
	if input.ExpectedBranch != nil {
		names = append(names, "refs/heads/"+*input.ExpectedBranch)
	}
	release, err := lockReferences(r, names)
	if err != nil {
		return err
	}
	defer release()
	if err := checkBranch(r, input.Name, input.Commit); err != nil {
		return err
	}
	h, err := head(r)
	if err != nil {
		return err
	}
	if h.Head == nil || *h.Head != input.ExpectedHead || !sameString(h.Branch, input.ExpectedBranch) {
		return conflict()
	}
	if h.Branch != nil && *h.Branch == input.Name {
		return fault(protocol.ErrorCodeConflict, "cannot delete the current branch")
	}
	if err := s.branchAvailable(ctx, root, input.Name, false); err != nil {
		return err
	}
	ahead, _, err := aheadBehind(ctx, r, plumbing.NewHash(input.Commit), plumbing.NewHash(input.ExpectedHead))
	if err != nil {
		return err
	}
	if ahead > 0 {
		return fault(protocol.ErrorCodeConflict, "branch has unmerged commits")
	}
	config, err := r.Config()
	if err != nil {
		return mapError(err)
	}
	delete(config.Branches, input.Name)
	if err := r.Storer.SetConfig(config); err != nil {
		return fault(protocol.ErrorCodeOutcomeUnknown, "branch deletion may have changed metadata")
	}
	if err := r.Storer.RemoveReference(plumbing.NewBranchReferenceName(input.Name)); err != nil {
		return fault(protocol.ErrorCodeOutcomeUnknown, "branch deletion may have changed metadata")
	}
	s.Invalidate(root)
	return nil
}
func (s *Service) SwitchBranch(ctx context.Context, root string, input protocol.CommandSwitchGitBranchPayload) (protocol.GitBranch, error) {
	result := protocol.GitBranch{Name: input.Name, Commit: input.Commit, Current: true}
	if err := s.Check(ctx, root, input.ExpectedIndex, input.ExpectedHead, input.ExpectedBranch); err != nil {
		return result, err
	}
	r, err := open(root)
	if err != nil {
		return result, err
	}
	if err := checkBranch(r, input.Name, input.Commit); err != nil {
		return result, err
	}
	if err := s.branchAvailable(ctx, root, input.Name, true); err != nil {
		return result, err
	}
	defer s.Invalidate(root)
	_, err = s.command(ctx, root, []string{"switch", input.Name}, nil, false)
	return result, err
}
func (s *Service) MergeBranch(ctx context.Context, root string, input protocol.CommandMergeGitBranchPayload) (protocol.GitMerge, error) {
	result := protocol.GitMerge{}
	if err := s.Check(ctx, root, input.ExpectedIndex, &input.ExpectedHead, &input.ExpectedBranch); err != nil {
		return result, err
	}
	r, err := open(root)
	if err != nil {
		return result, err
	}
	if err := checkBranch(r, input.Name, input.Commit); err != nil {
		return result, err
	}
	if operation(root, r) != nil {
		return result, fault(protocol.ErrorCodeConflict, "finish the current Git operation before merging")
	}
	sourceAhead, targetAhead, err := aheadBehind(ctx, r, plumbing.NewHash(input.Commit), plumbing.NewHash(input.ExpectedHead))
	if err != nil {
		return result, err
	}
	if sourceAhead == 0 {
		return protocol.GitMerge{Kind: protocol.GitMergeKindUpToDate, Commit: input.ExpectedHead}, nil
	}
	status, err := s.Status(ctx, root)
	if err != nil {
		return result, err
	}
	if len(status.Entries) > 0 || status.Truncated || status.OmittedPaths > 0 {
		return result, fault(protocol.ErrorCodeConflict, "commit or stash target worktree changes before merging")
	}
	defer s.Invalidate(root)
	if _, err := s.command(ctx, root, []string{"merge", "--no-edit", input.Commit}, nil, false); err != nil {
		return result, err
	}
	h, err := s.Head(ctx, root)
	if err != nil || h.Head == nil {
		return result, fault(protocol.ErrorCodeOutcomeUnknown, "merge may have changed files or references")
	}
	kind := protocol.GitMergeKindMergeCommit
	if targetAhead == 0 {
		kind = protocol.GitMergeKindFastForward
	}
	return protocol.GitMerge{Kind: kind, Commit: *h.Head}, nil
}
