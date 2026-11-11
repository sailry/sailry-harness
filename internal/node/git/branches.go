package git

import (
	"context"
	"io"
	"os"
	"path/filepath"
	"slices"
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
	for _, item := range []struct{ path, name string }{{"MERGE_HEAD", "merge"}, {"CHERRY_PICK_HEAD", "cherry-pick"}, {"REVERT_HEAD", "revert"}, {"rebase-merge", "rebase"}, {"rebase-apply", "rebase"}} {
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

// aheadBehind counts the commits reachable from only a and from only b. It
// ports libgit2's graph_ahead_behind, which the Rust service called without a
// further limit: both histories are painted newest first until every queued
// commit descends from a shared one, then each tip is counted down to the first
// shared commit. The walk is bounded by the distance to the merge base rather
// than by the length of the history, and shares Git's reliance on commit dates.
func aheadBehind(ctx context.Context, r *gogit.Repository, a, b plumbing.Hash) (uint64, uint64, error) {
	if a == b {
		return 0, 0, nil
	}
	const (
		left = 1 << iota
		right
		stale
		counted
	)
	type node struct {
		commit *object.Commit
		flags  uint8
	}
	nodes := map[plumbing.Hash]*node{}
	load := func(hash plumbing.Hash) (*node, error) {
		if found := nodes[hash]; found != nil {
			return found, nil
		}
		commit, err := r.CommitObject(hash)
		if err != nil {
			return nil, mapError(err)
		}
		nodes[hash] = &node{commit: commit}
		return nodes[hash], nil
	}
	// The queue is ordered oldest first, so the newest commit is popped from its end.
	var queue []*node
	push := func(value *node) {
		when := value.commit.Committer.When
		i := sort.Search(len(queue), func(i int) bool { return queue[i].commit.Committer.When.After(when) })
		queue = slices.Insert(queue, i, value)
	}
	pop := func() *node {
		value := queue[len(queue)-1]
		queue = queue[:len(queue)-1]
		return value
	}
	one, err := load(a)
	if err != nil {
		return 0, 0, err
	}
	two, err := load(b)
	if err != nil {
		return 0, 0, err
	}
	one.flags, two.flags = left, right
	push(one)
	push(two)
	var roots []*node
	fresh := func(value *node) bool { return value.flags&stale == 0 }
	for len(queue) > 0 && (slices.ContainsFunc(queue, fresh) || slices.ContainsFunc(roots, fresh)) {
		if err := checkContext(ctx); err != nil {
			return 0, 0, err
		}
		current := pop()
		flags := current.flags & (left | right | stale)
		if flags == left|right {
			// Everything below a shared commit is shared.
			flags |= stale
		}
		for _, hash := range current.commit.ParentHashes {
			parent, err := load(hash)
			if err != nil {
				return 0, 0, err
			}
			if parent.flags&flags != flags {
				parent.flags |= flags
				push(parent)
			}
		}
		if len(current.commit.ParentHashes) == 0 {
			// A root reached from one side keeps the walk going until the other side passes it.
			roots = append(roots, current)
		}
	}
	var ahead, behind uint64
	queue = queue[:0]
	push(one)
	push(two)
	for len(queue) > 0 {
		if err := checkContext(ctx); err != nil {
			return 0, 0, err
		}
		current := pop()
		if current.flags&counted != 0 || current.flags&(left|right) == left|right {
			continue
		}
		current.flags |= counted
		if current.flags&left != 0 {
			ahead++
		} else {
			behind++
		}
		for _, hash := range current.commit.ParentHashes {
			parent, err := load(hash)
			if err != nil {
				return 0, 0, err
			}
			push(parent)
		}
	}
	return ahead, behind, nil
}
