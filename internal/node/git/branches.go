package git

import (
	"context"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (s *Service) Branches(ctx context.Context, root string) (protocol.GitBranches, error) {
	h, err := s.Head(ctx, root)
	result := protocol.GitBranches{Kind: h.Kind, Current: h.Branch, Entries: []protocol.GitBranch{}, Sync: protocol.DefaultGitSync()}
	if err != nil || h.Kind == protocol.RepositoryKindDirectory {
		return result, err
	}
	r, err := open(root)
	if err != nil {
		return result, err
	}
	iter, err := r.References()
	if err != nil {
		return result, mapError(err)
	}
	defer iter.Close()
	count := 0
	err = iter.ForEach(func(ref *plumbing.Reference) error {
		if err := checkContext(ctx); err != nil {
			return err
		}
		if !ref.Name().IsBranch() && !ref.Name().IsRemote() {
			return nil
		}
		if count == maxEntries {
			result.Truncated = true
			return io.EOF
		}
		count++
		if ref.Type() == plumbing.SymbolicReference {
			return nil
		}
		name := ref.Name().Short()
		if len(name) > 1024 {
			result.OmittedNames++
			return nil
		}
		remote := ref.Name().IsRemote()
		result.Entries = append(result.Entries, protocol.GitBranch{Name: name, Commit: ref.Hash().String(), Remote: remote, Current: !remote && h.Branch != nil && *h.Branch == name})
		return nil
	})
	if err != nil && err != io.EOF {
		return result, mapError(err)
	}
	sort.Slice(result.Entries, func(i, j int) bool {
		a, b := result.Entries[i], result.Entries[j]
		if a.Remote != b.Remote {
			return !a.Remote
		}
		return a.Name < b.Name
	})
	result.Sync, err = s.sync(ctx, root, r, h)
	return result, err
}
func (s *Service) stashes(ctx context.Context, root string) ([]protocol.GitStash, error) {
	capture, err := s.command(ctx, root, []string{"stash", "list", "-z", "--format=%H%x00%gs"}, nil, true)
	if err != nil {
		return nil, err
	}
	if capture.Truncated || capture.InvalidUTF8 {
		return nil, fault(protocol.ErrorCodeUnavailable, "Git stash listing exceeds output limit")
	}
	fields := strings.Split(strings.TrimSuffix(string(capture.Stdout), "\x00"), "\x00")
	result := []protocol.GitStash{}
	if len(fields) == 1 && fields[0] == "" {
		return result, nil
	}
	if len(fields)%2 != 0 {
		return nil, fault(protocol.ErrorCodeUnavailable, "invalid Git stash listing")
	}
	for i := 0; i < len(fields) && len(result) < maxEntries; i += 2 {
		result = append(result, protocol.GitStash{Commit: fields[i], Message: fields[i+1]})
	}
	return result, nil
}
func operation(root string, r *gogit.Repository) *string {
	dir, err := gitDir(root, r)
	if err != nil {
		return nil
	}
	for _, item := range []struct{ path, name string }{{"MERGE_HEAD", "merge"}, {"CHERRY_PICK_HEAD", "cherry-pick"}, {"rebase-merge", "rebase"}, {"rebase-apply", "rebase"}} {
		if _, err := os.Lstat(filepath.Join(dir, item.path)); err == nil {
			return ptr(item.name)
		}
	}
	return nil
}
func (s *Service) sync(ctx context.Context, root string, r *gogit.Repository, h protocol.GitHead) (protocol.GitSync, error) {
	result := protocol.DefaultGitSync()
	var err error
	result.Stashes, err = s.stashes(ctx, root)
	if err != nil {
		return result, err
	}
	result.Operation = operation(root, r)
	config, err := r.Config()
	if err != nil {
		return result, mapError(err)
	}
	for name, remote := range config.Remotes {
		result.Remotes = append(result.Remotes, name)
		if len(remote.URLs) > 0 {
			result.RemoteUrls[name] = redact(remote.URLs[0])
		}
	}
	sort.Strings(result.Remotes)
	if len(result.Remotes) > maxEntries {
		result.Remotes = result.Remotes[:maxEntries]
	}
	for name, branch := range config.Branches {
		if branch.Remote != "" && branch.Merge != "" {
			target := branch.Remote + "/" + branch.Merge.Short()
			if branch.Remote == "." {
				target = branch.Merge.Short()
			}
			result.Tracking[name] = target
			if h.Branch != nil && name == *h.Branch {
				result.Upstream = ptr(target)
			}
		}
	}
	tags, err := r.Tags()
	if err != nil {
		return result, mapError(err)
	}
	defer tags.Close()
	_ = tags.ForEach(func(ref *plumbing.Reference) error {
		if len(result.Tags) == maxEntries {
			return io.EOF
		}
		result.Tags = append(result.Tags, protocol.GitTag{Name: ref.Name().Short(), Commit: ref.Hash().String()})
		return checkContext(ctx)
	})
	if h.Head != nil {
		commit, err := r.CommitObject(plumbing.NewHash(*h.Head))
		if err != nil {
			return result, mapError(err)
		}
		message, _ := bounded(commit.Message, 65536)
		result.HeadMessage = &message
		summary := strings.SplitN(commit.Message, "\n", 2)[0]
		result.HeadSummary = &summary
	}
	if h.Head != nil && result.Upstream != nil {
		hash, err := r.ResolveRevision(plumbing.Revision("refs/remotes/" + *result.Upstream))
		if err == nil {
			result.Ahead, result.Behind, err = aheadBehind(ctx, r, plumbing.NewHash(*h.Head), *hash)
			if err != nil {
				return result, err
			}
		}
	}
	return result, checkContext(ctx)
}
func aheadBehind(ctx context.Context, r *gogit.Repository, a, b plumbing.Hash) (uint64, uint64, error) {
	reachable := func(hash plumbing.Hash) (map[plumbing.Hash]bool, error) {
		result := map[plumbing.Hash]bool{}
		iter, err := r.Log(&gogit.LogOptions{From: hash})
		if err != nil {
			return nil, mapError(err)
		}
		defer iter.Close()
		err = iter.ForEach(func(c *object.Commit) error { result[c.Hash] = true; return checkContext(ctx) })
		return result, err
	}
	left, err := reachable(a)
	if err != nil {
		return 0, 0, err
	}
	right, err := reachable(b)
	if err != nil {
		return 0, 0, err
	}
	var ahead, behind uint64
	for h := range left {
		if !right[h] {
			ahead++
		}
	}
	for h := range right {
		if !left[h] {
			behind++
		}
	}
	return ahead, behind, nil
}
