package git

import (
	"context"
	"errors"
	"os"
	"path/filepath"

	gogit "github.com/go-git/go-git/v5"
	idxfmt "github.com/go-git/go-git/v5/plumbing/format/index"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type capturedChange struct {
	path           string
	working, index content
}

// CreateManagedWorktree copies captured changes into a new owned checkout. The
// source worktree and index are never modified or stashed.
func (s *Service) CreateManagedWorktree(ctx context.Context, source, main string, id protocol.WorktreeID, input protocol.CommandCreateManagedWorktreePayload) (string, error) {
	if input.Base != nil && input.IncludeChanges {
		return "", invalid("a base commit cannot include source changes")
	}
	base := input.ExpectedHead
	if input.Base != nil {
		base = *input.Base
	}
	r, err := open(source)
	if err != nil {
		return "", err
	}
	if input.Base == nil {
		h, err := head(r)
		if err != nil {
			return "", err
		}
		revision, err := indexRevision(source, r)
		if err != nil {
			return "", err
		}
		if h.Head == nil || *h.Head != input.ExpectedHead || revision != input.ExpectedIndex {
			return "", conflict()
		}
	}
	idx, err := r.Storer.Index()
	if err != nil {
		return "", mapError(err)
	}
	for _, entry := range idx.Entries {
		if entry.Stage != 0 {
			return "", fault(protocol.ErrorCodeConflict, "resolve index conflicts before creating a worktree")
		}
	}
	changes := []capturedChange{}
	if input.IncludeChanges {
		s.Invalidate(source)
		status, err := s.Status(ctx, source)
		if err != nil {
			return "", err
		}
		if status.Truncated || status.OmittedPaths > 0 {
			return "", fault(protocol.ErrorCodeUnavailable, "source changes exceed snapshot limits")
		}
		for _, entry := range status.Entries {
			if entry.Conflicted {
				return "", fault(protocol.ErrorCodeConflict, "resolve index conflicts before creating a worktree")
			}
			working, err := workingContent(source, entry.Path, maxFile)
			if err != nil {
				return "", err
			}
			staged, err := indexContent(r, entry.Path, maxFile)
			if err != nil {
				return "", err
			}
			changes = append(changes, capturedChange{entry.Path, working, staged})
		}
		revision, err := indexRevision(source, r)
		if err != nil {
			return "", err
		}
		h, err := head(r)
		if err != nil {
			return "", err
		}
		if revision != input.ExpectedIndex || h.Head == nil || *h.Head != input.ExpectedHead {
			return "", conflict()
		}
	}
	directory, err := s.PrepareManaged(main)
	if err != nil {
		return "", err
	}
	target, err := s.CreateWorktree(ctx, source, filepath.Join(directory, id.String()), input.Branch, base)
	if err != nil {
		return "", err
	}
	if len(changes) == 0 {
		return target, nil
	}
	unknown := func() error {
		return fault(protocol.ErrorCodeOutcomeUnknown, "worktree was created but copying changes did not complete")
	}
	anchor, err := os.OpenRoot(target)
	if err != nil {
		return "", unknown()
	}
	defer anchor.Close()
	for _, change := range changes {
		if err := checkContext(ctx); err != nil {
			return "", unknown()
		}
		parent := filepath.Dir(filepath.FromSlash(change.path))
		if parent != "." {
			if err := anchor.MkdirAll(parent, 0755); err != nil {
				return "", unknown()
			}
		}
		if !change.working.exists {
			if err := anchor.Remove(filepath.FromSlash(change.path)); err != nil && !errors.Is(err, os.ErrNotExist) {
				return "", unknown()
			}
			continue
		}
		if change.working.mode == 0120000 {
			if err := anchor.Remove(filepath.FromSlash(change.path)); err != nil && !errors.Is(err, os.ErrNotExist) {
				return "", unknown()
			}
			if err := anchor.Symlink(string(change.working.data), filepath.FromSlash(change.path)); err != nil {
				return "", unknown()
			}
			continue
		}
		mode := os.FileMode(0644)
		if change.working.mode == 0100755 {
			mode = 0755
		}
		file, err := anchor.OpenFile(filepath.FromSlash(change.path), os.O_WRONLY|os.O_CREATE|os.O_TRUNC, mode)
		if err != nil {
			return "", unknown()
		}
		_, err = file.Write(change.working.data)
		closeErr := file.Close()
		if err != nil || closeErr != nil {
			return "", unknown()
		}
	}
	created, err := open(target)
	if err != nil {
		return "", unknown()
	}
	revision, err := indexRevision(target, created)
	if err != nil {
		return "", unknown()
	}
	_, err = s.publishIndex(ctx, target, revision, &base, func(_ *gogit.Repository, idx *idxfmt.Index, _ *object.Tree) error {
		for _, change := range changes {
			if !change.index.exists {
				dropIndexEntries(idx, change.path, false)
				continue
			}
			// Linked worktrees share their object store. Reuse the captured
			// reference, including packed objects and non-blob index modes.
			dropIndexEntries(idx, change.path, true)
			idx.Entries = append(idx.Entries, &idxfmt.Entry{Name: change.path, Hash: change.index.hash, Mode: change.index.mode})
		}
		return nil
	}, nil)
	if err != nil {
		return "", unknown()
	}
	return target, nil
}

// CaptureManaged returns the source sandbox's protected child-checkout boundary.
// Repository preparation reuses the single managed/exclude mutation owner.
func (s *Service) CaptureManaged(root string) (string, error) {
	if _, err := open(root); err == nil {
		return s.PrepareManaged(root)
	}
	anchor, err := os.OpenRoot(root)
	if err != nil {
		return "", mapError(err)
	}
	defer anchor.Close()
	info, err := anchor.Lstat(".sailry")
	if errors.Is(err, os.ErrNotExist) {
		return "", nil
	}
	if err != nil {
		return "", mapError(err)
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return "", nil
	}
	directory, err := anchor.OpenRoot(".sailry")
	if err != nil {
		return "", mapError(err)
	}
	defer directory.Close()
	info, err = directory.Lstat("worktrees")
	if errors.Is(err, os.ErrNotExist) {
		return "", nil
	}
	if err != nil {
		return "", mapError(err)
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return "", nil
	}
	return filepath.Join(root, ".sailry", "worktrees"), nil
}
