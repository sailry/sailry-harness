package git

import (
	"context"
	"errors"
	"os"
	"reflect"
	"strconv"
	"strings"
	"unicode"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func data[T any](value any) T {
	if v, ok := value.(*T); ok && v != nil {
		return *v
	}
	if v, ok := value.(T); ok {
		return v
	}
	var empty T
	return empty
}
func name(value string) error {
	if value == "" || strings.HasPrefix(value, "-") || len(value) > 1024 || strings.ContainsFunc(value, unicode.IsControl) {
		return invalid("invalid Git target")
	}
	return nil
}
func branchName(value string) error {
	if err := name(value); err != nil {
		return err
	}
	if err := plumbing.NewBranchReferenceName(value).Validate(); err != nil {
		return invalid("invalid branch name")
	}
	return nil
}
func fullHash(value string) error {
	if len(value) != 40 {
		return invalid("expected a full commit identifier")
	}
	for _, c := range value {
		if !strings.ContainsRune("0123456789abcdefABCDEF", c) {
			return invalid("expected a full commit identifier")
		}
	}
	return nil
}

// CLIArgs preserves native argv without executing a shell. Repository-dependent
// validation and compound operations remain in Action.
func CLIArgs(action protocol.GitAction, branch, operation string, stashPosition int) ([]string, error) {
	switch action.Kind {
	case "start_branch":
		v := data[protocol.GitActionStartBranchPayload](action.Data)
		if err := branchName(v.Name); err != nil {
			return nil, err
		}
		return []string{"checkout", "-b", v.Name}, nil
	case "add_remote":
		v := data[protocol.GitActionAddRemotePayload](action.Data)
		if err := name(v.Name); err != nil {
			return nil, err
		}
		if v.Url == "" || len(v.Url) > 4096 || strings.HasPrefix(v.Url, "-") || strings.ContainsFunc(v.Url, unicode.IsControl) {
			return nil, invalid("invalid Git remote URL")
		}
		return []string{"remote", "add", "--", v.Name, v.Url}, nil
	case "remove_remote":
		v := data[protocol.GitActionRemoveRemotePayload](action.Data)
		return []string{"remote", "remove", v.Name}, name(v.Name)
	case "fetch":
		v := data[protocol.GitActionFetchPayload](action.Data)
		args := []string{"fetch"}
		if v.Prune {
			args = append(args, "--prune")
		}
		if v.All && v.Remote != nil {
			return nil, invalid("choose one remote or all remotes")
		}
		if v.Remote != nil {
			if err := name(*v.Remote); err != nil {
				return nil, err
			}
			args = append(args, *v.Remote)
		} else if v.All {
			args = append(args, "--all")
		}
		return args, nil
	case "pull":
		v := data[protocol.GitActionPullPayload](action.Data)
		flag := "--no-rebase"
		if v.Rebase {
			flag = "--rebase"
		}
		args := []string{"pull", flag, "--no-edit"}
		if v.Remote != nil {
			if err := name(*v.Remote); err != nil {
				return nil, err
			}
			args = append(args, *v.Remote)
			if v.Branch != nil {
				if err := name(*v.Branch); err != nil {
					return nil, err
				}
				args = append(args, *v.Branch)
			}
		} else if v.Branch != nil {
			return nil, invalid("pull branch requires a remote")
		}
		return args, nil
	case "push":
		v := data[protocol.GitActionPushPayload](action.Data)
		args := []string{"push"}
		if v.Publish {
			args = append(args, "--set-upstream")
		}
		if v.Force {
			args = append(args, "--force-with-lease")
		}
		if v.Remote != nil {
			if err := name(*v.Remote); err != nil {
				return nil, err
			}
			if branch == "" {
				return nil, invalid("push requires an attached branch")
			}
			args = append(args, *v.Remote, "refs/heads/"+branch+":refs/heads/"+branch)
		} else if v.Publish {
			return nil, invalid("select a remote to publish")
		}
		return args, nil
	case "merge", "rebase", "cherry_pick":
		var commit string
		switch action.Kind {
		case "merge":
			commit = data[protocol.GitActionMergePayload](action.Data).Commit
		case "rebase":
			commit = data[protocol.GitActionRebasePayload](action.Data).Commit
		case "cherry_pick":
			commit = data[protocol.GitActionCherryPickPayload](action.Data).Commit
		}
		if err := fullHash(commit); err != nil {
			return nil, err
		}
		verb := strings.ReplaceAll(action.Kind, "_", "-")
		return []string{verb, commit}, nil
	case "track":
		v := data[protocol.GitActionTrackPayload](action.Data)
		return []string{"branch", "--set-upstream-to=" + v.Branch}, name(v.Branch)
	case "switch_remote":
		v := data[protocol.GitActionSwitchRemotePayload](action.Data)
		return []string{"switch", "--track", v.Branch}, name(v.Branch)
	case "stash":
		v := data[protocol.GitActionStashPayload](action.Data)
		args := []string{"stash", "push"}
		switch v.Mode {
		case protocol.GitStashModeAll:
			args = append(args, "--include-untracked")
		case protocol.GitStashModeStaged:
			args = append(args, "--staged")
		case protocol.GitStashModeTracked:
		default:
			return nil, invalid("invalid stash mode")
		}
		return args, nil
	case "apply_stash":
		v := data[protocol.GitActionApplyStashPayload](action.Data)
		return []string{"stash", "apply", v.Commit}, fullHash(v.Commit)
	case "drop_stash":
		if stashPosition < 0 {
			return nil, invalid("stash does not exist")
		}
		return []string{"stash", "drop", "stash@{" + strconv.Itoa(stashPosition) + "}"}, nil
	case "drop_all_stashes":
		return []string{"stash", "clear"}, nil
	case "create_tag":
		v := data[protocol.GitActionCreateTagPayload](action.Data)
		if err := plumbing.NewTagReferenceName(v.Name).Validate(); err != nil {
			return nil, invalid("invalid tag name")
		}
		if err := fullHash(v.Commit); err != nil {
			return nil, err
		}
		args := []string{"tag"}
		if v.Message != nil && *v.Message != "" {
			if len(*v.Message) > 16384 || strings.ContainsRune(*v.Message, 0) {
				return nil, invalid("invalid tag message")
			}
			args = append(args, "--annotate", "--message", *v.Message)
		}
		return append(args, "--", v.Name, v.Commit), nil
	case "delete_tag":
		v := data[protocol.GitActionDeleteTagPayload](action.Data)
		if err := plumbing.NewTagReferenceName(v.Name).Validate(); err != nil {
			return nil, invalid("invalid tag name")
		}
		if err := fullHash(v.Commit); err != nil {
			return nil, err
		}
		ref := "refs/tags/" + v.Name
		if v.Remote != nil {
			return []string{"push", "--force-with-lease=" + ref + ":" + v.Commit, *v.Remote, ":" + ref}, name(*v.Remote)
		}
		return []string{"update-ref", "-d", ref, v.Commit}, nil
	case "push_tags":
		v := data[protocol.GitActionPushTagsPayload](action.Data)
		return []string{"push", "--tags", v.Remote}, name(v.Remote)
	case "discard":
		v := data[protocol.GitActionDiscardPayload](action.Data)
		if err := pathsValid(v.Paths); err != nil {
			return nil, err
		}
		args := []string{"restore", "--source=HEAD", "--staged", "--worktree", "--"}
		for _, p := range v.Paths {
			args = append(args, ":(literal)"+p)
		}
		return args, nil
	case "continue", "abort":
		if operation != "merge" && operation != "rebase" && operation != "cherry-pick" {
			return nil, invalid("no Git operation to continue or abort")
		}
		return []string{operation, "--" + action.Kind}, nil
	case "prune_worktree":
		v := data[protocol.GitActionPruneWorktreePayload](action.Data)
		return []string{"worktree", "remove", "--", v.Path}, nil
	}
	return nil, invalid("Git action requires its compound operation owner")
}
func pathsValid(paths []string) error {
	if len(paths) == 0 || len(paths) > maxEntries {
		return invalid("invalid repository paths")
	}
	for _, path := range paths {
		if err := validPath(path); err != nil {
			return err
		}
	}
	return nil
}

// remoteAction reports whether an action transfers with a remote.
func remoteAction(action protocol.GitAction) bool {
	switch action.Kind {
	case "fetch", "pull", "push", "sync", "push_tags", "delete_remote_branch":
		return true
	case "delete_tag":
		return data[protocol.GitActionDeleteTagPayload](action.Data).Remote != nil
	}
	return false
}

// pull fetches outside the root lock and lets git pull integrate under it. Its
// own fetch then finds the objects present, so the lock covers a short
// negotiation and the merge or rebase, not the transfer.
func (s *Service) pull(ctx context.Context, root, flag string, target ...string) error {
	if _, err := s.network(ctx, root, append([]string{"fetch"}, target...)); err != nil {
		return err
	}
	defer s.lock(root)()
	_, err := s.network(ctx, root, append([]string{"pull", flag, "--no-edit"}, target...))
	return err
}

func (s *Service) Action(ctx context.Context, root string, input protocol.CommandRunGitActionPayload) error {
	action := input.Action
	// A transfer with a remote must not hold the root lock that file effects
	// wait for; every other action holds it throughout.
	if !remoteAction(action) {
		defer s.lock(root)()
	}
	if action.Kind == "initialize" {
		if input.ExpectedIndex != "" || input.ExpectedHead != nil || input.ExpectedBranch != nil {
			return conflict()
		}
		if _, err := os.Lstat(root + string(os.PathSeparator) + ".git"); !errors.Is(err, os.ErrNotExist) {
			return conflict()
		}
		_, err := gogit.PlainInitWithOptions(root, &gogit.PlainInitOptions{InitOptions: gogit.InitOptions{DefaultBranch: plumbing.NewBranchReferenceName("main")}})
		s.Invalidate(root)
		return mapError(err)
	}
	if err := s.Check(ctx, root, input.ExpectedIndex, input.ExpectedHead, input.ExpectedBranch); err != nil {
		return err
	}
	r, err := open(root)
	if err != nil {
		return err
	}
	defer s.Invalidate(root)
	run := func(args []string) error { _, err := s.command(ctx, root, args, nil, false); return err }
	transfer := func(args []string) error { _, err := s.network(ctx, root, args); return err }
	branch := ""
	if input.ExpectedBranch != nil {
		branch = *input.ExpectedBranch
	}
	position := -1
	stashPosition := func(commit string) (int, error) {
		stashes, err := s.stashes(ctx, root)
		if err != nil {
			return -1, err
		}
		for i, v := range stashes {
			if v.Commit == commit {
				return i, nil
			}
		}
		return -1, conflict()
	}
	switch action.Kind {
	case "ignore":
		v := data[protocol.GitActionIgnorePayload](action.Data)
		return s.Ignore(ctx, root, v.Paths, v.Directories)
	case "trash":
		return s.trash(ctx, root, data[protocol.GitActionTrashPayload](action.Data).Paths, true)
	case "discard_all":
		return s.discardAll(ctx, root, data[protocol.GitActionDiscardAllPayload](action.Data).Paths)
	case "discard":
		// Restoring from HEAD removes a path HEAD does not hold, and a staged
		// new file exists nowhere else. Such paths are refused here; discard_all
		// unstages them and moves the file to Trash.
		v := data[protocol.GitActionDiscardPayload](action.Data)
		if err := pathsValid(v.Paths); err != nil {
			return err
		}
		tree, err := headTree(r)
		if err != nil {
			return err
		}
		for _, path := range v.Paths {
			if _, err := safePath(root, path); err != nil {
				return err
			}
			held, err := heldByHead(tree, path)
			if err != nil {
				return err
			}
			if !held {
				return invalid("discard requires files that HEAD holds")
			}
		}
	case "delete_remote_branch":
		v := data[protocol.GitActionDeleteRemoteBranchPayload](action.Data)
		if err := name(v.Branch); err != nil {
			return err
		}
		if err := fullHash(v.Commit); err != nil {
			return err
		}
		remote, local, err := remoteBranch(r, v.Branch)
		if err != nil {
			return err
		}
		ref := "refs/heads/" + local
		return transfer([]string{"push", "--force-with-lease=" + ref + ":" + v.Commit, remote, ":" + ref})
	case "sync":
		v := data[protocol.GitActionSyncPayload](action.Data)
		flag := "--no-rebase"
		if v.Rebase {
			flag = "--rebase"
		}
		if err := s.pull(ctx, root, flag); err != nil {
			return err
		}
		return transfer([]string{"push"})
	case "start_branch":
		if input.ExpectedHead != nil {
			return conflict()
		}
		v := data[protocol.GitActionStartBranchPayload](action.Data)
		if _, e := r.Reference(plumbing.NewBranchReferenceName(v.Name), true); e == nil || branch == v.Name {
			return fault(protocol.ErrorCodeConflict, BranchExists)
		}
	case "apply_stash":
		v := data[protocol.GitActionApplyStashPayload](action.Data)
		position, err = stashPosition(v.Commit)
		if err != nil {
			return err
		}
		if err := run([]string{"stash", "apply", v.Commit}); err != nil {
			return err
		}
		if v.Pop {
			next, e := stashPosition(v.Commit)
			if e != nil || next != position {
				return fault(protocol.ErrorCodeOutcomeUnknown, "stash applied but its position changed before removal")
			}
			if err := run([]string{"stash", "drop", "stash@{" + strconv.Itoa(position) + "}"}); err != nil {
				return fault(protocol.ErrorCodeOutcomeUnknown, "stash applied but removal could not be confirmed")
			}
		}
		return nil
	case "drop_stash":
		position, err = stashPosition(data[protocol.GitActionDropStashPayload](action.Data).Commit)
		if err != nil {
			return err
		}
	case "drop_all_stashes":
		v := data[protocol.GitActionDropAllStashesPayload](action.Data)
		stashes, e := s.stashes(ctx, root)
		if e != nil {
			return e
		}
		commits := []string{}
		for _, v := range stashes {
			commits = append(commits, v.Commit)
		}
		if !reflect.DeepEqual(commits, v.Commits) {
			return conflict()
		}
	case "undo_commit":
		h, e := r.Head()
		if e != nil {
			return mapError(e)
		}
		c, e := r.CommitObject(h.Hash())
		if e != nil {
			return mapError(e)
		}
		if c.NumParents() > 0 {
			return run([]string{"reset", "--soft", c.ParentHashes[0].String()})
		}
		if branch == "" {
			return invalid("cannot undo a detached root commit")
		}
		return run([]string{"update-ref", "-d", "HEAD", c.Hash.String()})
	case "switch_stashing":
		v := data[protocol.GitActionSwitchStashingPayload](action.Data)
		if err := checkBranch(r, v.Branch, v.Commit); err != nil {
			return err
		}
		if err := run([]string{"stash", "push", "--include-untracked"}); err != nil {
			return err
		}
		if err := run([]string{"switch", v.Branch}); err != nil {
			return fault(protocol.ErrorCodeOutcomeUnknown, "changes were stashed before switching failed")
		}
		return nil
	case "switch_tracking":
		return s.switchTracking(ctx, root, r, data[protocol.GitActionSwitchTrackingPayload](action.Data))
	case "switch_remote":
		target := data[protocol.GitActionSwitchRemotePayload](action.Data).Branch
		config, e := r.Config()
		if e != nil {
			return mapError(e)
		}
		for local, b := range config.Branches {
			if b.Remote+"/"+b.Merge.Short() == target {
				if _, err := r.Reference(plumbing.NewBranchReferenceName(local), true); err == nil {
					return run([]string{"switch", local})
				}
			}
		}
	case "prune_worktree":
		v := data[protocol.GitActionPruneWorktreePayload](action.Data)
		return s.PruneWorktree(ctx, root, v.Path)
	}
	for _, remote := range actionRemotes(action) {
		if _, err := r.Remote(remote); err != nil {
			return fault(protocol.ErrorCodeNotConfigured, "Git remote is not configured")
		}
	}
	args, err := CLIArgs(action, branch, valueOr(operation(root, r)), position)
	if err != nil {
		return err
	}
	// fetch, pull and push talk to a remote; so do the pushes that delete a
	// remote tag or branch.
	if args[0] == "pull" {
		return s.pull(ctx, root, args[1], args[3:]...)
	}
	if args[0] == "fetch" || args[0] == "push" {
		return transfer(args)
	}
	return run(args)
}

// remoteBranch splits a remote-tracking branch into its remote and the branch
// there. A remote's name may hold a slash, so the longest configured remote
// that begins the name is the one meant.
func remoteBranch(r *gogit.Repository, branch string) (string, string, error) {
	config, err := r.Config()
	if err != nil {
		return "", "", mapError(err)
	}
	remote := ""
	for name := range config.Remotes {
		if strings.HasPrefix(branch, name+"/") && len(name) > len(remote) {
			remote = name
		}
	}
	if remote == "" {
		return "", "", fault(protocol.ErrorCodeNotConfigured, "Git remote is not configured")
	}
	return remote, branch[len(remote)+1:], nil
}
func valueOr(value *string) string {
	if value == nil {
		return ""
	}
	return *value
}
func checkBranch(r *gogit.Repository, name, commit string) error {
	if err := branchName(name); err != nil {
		return err
	}
	if err := fullHash(commit); err != nil {
		return err
	}
	ref, err := r.Reference(plumbing.NewBranchReferenceName(name), true)
	if err != nil {
		return mapError(err)
	}
	if ref.Hash().String() != commit {
		return conflict()
	}
	return nil
}
func actionRemotes(action protocol.GitAction) []string {
	var value *string
	switch action.Kind {
	case "fetch":
		value = data[protocol.GitActionFetchPayload](action.Data).Remote
	case "pull":
		value = data[protocol.GitActionPullPayload](action.Data).Remote
	case "push":
		value = data[protocol.GitActionPushPayload](action.Data).Remote
	case "push_tags":
		return []string{data[protocol.GitActionPushTagsPayload](action.Data).Remote}
	case "remove_remote":
		return []string{data[protocol.GitActionRemoveRemotePayload](action.Data).Name}
	case "delete_tag":
		value = data[protocol.GitActionDeleteTagPayload](action.Data).Remote
	}
	if value != nil {
		return []string{*value}
	}
	return nil
}
func (s *Service) switchTracking(ctx context.Context, root string, r *gogit.Repository, v protocol.GitActionSwitchTrackingPayload) error {
	if err := branchName(v.Local); err != nil {
		return err
	}
	if _, err := r.Reference(plumbing.ReferenceName("refs/remotes/"+v.Remote), true); err != nil {
		return mapError(err)
	}
	args := []string{"switch", "--track", "-c", v.Local, v.Remote}
	if _, err := r.Reference(plumbing.NewBranchReferenceName(v.Local), true); err == nil {
		config, e := r.Config()
		if e != nil {
			return mapError(e)
		}
		branch := config.Branches[v.Local]
		if branch == nil || branch.Remote+"/"+branch.Merge.Short() != v.Remote {
			return fault(protocol.ErrorCodeConflict, "local branch tracks a different remote; choose another name")
		}
		args = []string{"switch", v.Local}
	}
	if v.Stash {
		if _, err := s.command(ctx, root, []string{"stash", "push", "--include-untracked"}, nil, false); err != nil {
			return err
		}
	}
	_, err := s.command(ctx, root, args, nil, false)
	if err != nil && v.Stash {
		return fault(protocol.ErrorCodeOutcomeUnknown, "changes were stashed before switching failed")
	}
	return err
}
