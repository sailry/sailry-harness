package git

import (
	"bytes"
	"context"
	"errors"
	"os"
	"strings"
	"unicode/utf8"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/filemode"
	idxfmt "github.com/go-git/go-git/v5/plumbing/format/index"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func (s *Service) UpdateIndex(ctx context.Context, root string, input protocol.CommandUpdateGitIndexPayload) (protocol.GitIndex, error) {
	ctx, cancel := context.WithTimeout(ctx, readTimeout)
	defer cancel()
	if len(input.Paths) == 0 || len(input.Paths) > maxEntries {
		return protocol.GitIndex{}, invalid("invalid Git index update")
	}
	for _, path := range input.Paths {
		if err := validPath(path); err != nil {
			return protocol.GitIndex{}, err
		}
	}
	return s.publishIndex(ctx, root, input.ExpectedIndex, input.ExpectedHead, func(r *gogit.Repository, idx *idxfmt.Index, tree *object.Tree) error {
		for _, path := range input.Paths {
			if err := checkContext(ctx); err != nil {
				return err
			}
			switch input.Operation {
			case protocol.GitIndexChangeStage:
				if err := stageIndexContent(ctx, root, r, idx, path); err != nil {
					return err
				}
			case protocol.GitIndexChangeUnstage:
				if err := setIndexReference(idx, tree, path); err != nil {
					return err
				}
			default:
				return invalid("invalid Git index operation")
			}
		}
		return nil
	}, nil)
}
func setIndexContent(r *gogit.Repository, idx *idxfmt.Index, path string, value content) error {
	kept := idx.Entries[:0]
	for _, entry := range idx.Entries {
		if entry.Name != path {
			kept = append(kept, entry)
		}
	}
	idx.Entries = kept
	if !value.exists {
		return nil
	}
	obj := r.Storer.NewEncodedObject()
	obj.SetType(plumbing.BlobObject)
	obj.SetSize(int64(len(value.data)))
	writer, err := obj.Writer()
	if err != nil {
		return mapError(err)
	}
	if _, err = writer.Write(value.data); err != nil {
		writer.Close()
		return mapError(err)
	}
	if err = writer.Close(); err != nil {
		return mapError(err)
	}
	hash, err := r.Storer.SetEncodedObject(obj)
	if err != nil {
		return mapError(err)
	}
	if err := syncBlob(r, hash); err != nil {
		return err
	}
	idx.Entries = append(idx.Entries, &idxfmt.Entry{Name: path, Hash: hash, Mode: value.mode, Size: uint32(len(value.data))})
	return nil
}
func (s *Service) publishIndex(ctx context.Context, root, expected string, expectedHead *string, mutate func(*gogit.Repository, *idxfmt.Index, *object.Tree) error, verify func() error) (protocol.GitIndex, error) {
	limited, cancel := context.WithTimeout(ctx, readTimeout)
	defer cancel()
	r, err := open(root)
	if err != nil {
		return protocol.GitIndex{}, err
	}
	dir, err := gitDir(root, r)
	if err != nil {
		return protocol.GitIndex{}, err
	}
	anchor, err := os.OpenRoot(dir)
	if err != nil {
		return protocol.GitIndex{}, mapError(err)
	}
	defer anchor.Close()
	lock, err := anchor.OpenFile("index.lock", os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if errors.Is(err, os.ErrExist) {
		return protocol.GitIndex{}, fault(protocol.ErrorCodeBusy, "Git index is locked")
	}
	if err != nil {
		return protocol.GitIndex{}, mapError(err)
	}
	owned := true
	defer func() {
		lock.Close()
		if owned {
			_ = anchor.Remove("index.lock")
		}
	}()
	revision, err := indexRevision(root, r)
	if err != nil {
		return protocol.GitIndex{}, err
	}
	h, err := head(r)
	if err != nil {
		return protocol.GitIndex{}, err
	}
	if revision != expected || !sameString(h.Head, expectedHead) {
		return protocol.GitIndex{}, conflict()
	}
	idx, err := r.Storer.Index()
	if err != nil {
		return protocol.GitIndex{}, mapError(err)
	}
	for _, entry := range idx.Entries {
		if entry.Stage != 0 {
			return protocol.GitIndex{}, fault(protocol.ErrorCodeConflict, "resolve index conflicts before updating staged paths")
		}
	}
	tree, err := headTree(r)
	if err != nil {
		return protocol.GitIndex{}, err
	}
	if err := mutate(r, idx, tree); err != nil {
		return protocol.GitIndex{}, err
	}
	if err := checkContext(limited); err != nil {
		return protocol.GitIndex{}, err
	}
	var candidate bytes.Buffer
	if err := idxfmt.NewEncoder(&candidate).Encode(idx); err != nil {
		return protocol.GitIndex{}, mapError(err)
	}
	if candidate.Len() > 16*1024*1024 {
		return protocol.GitIndex{}, invalid("Git index exceeds the operation size limit")
	}
	if _, err = lock.Write(candidate.Bytes()); err != nil {
		return protocol.GitIndex{}, mapError(err)
	}
	if err = lock.Sync(); err != nil {
		return protocol.GitIndex{}, mapError(err)
	}
	info, err := lock.Stat()
	if err != nil {
		return protocol.GitIndex{}, mapError(err)
	}
	current, e := anchor.Lstat("index.lock")
	if e != nil || !os.SameFile(info, current) {
		owned = false
		return protocol.GitIndex{}, conflict()
	}
	next, err := indexRevision(root, r)
	if err != nil {
		return protocol.GitIndex{}, err
	}
	now, err := head(r)
	if err != nil {
		return protocol.GitIndex{}, err
	}
	if next != expected || !sameString(now.Head, expectedHead) || !sameString(now.Branch, h.Branch) {
		return protocol.GitIndex{}, conflict()
	}
	if verify != nil {
		if err := verify(); err != nil {
			return protocol.GitIndex{}, err
		}
	}
	if err = lock.Close(); err != nil {
		return protocol.GitIndex{}, mapError(err)
	}
	if err = anchor.Rename("index.lock", "index"); err != nil {
		return protocol.GitIndex{}, fault(protocol.ErrorCodeOutcomeUnknown, "Git index publication could not be confirmed")
	}
	owned = false
	if err := syncDirectory(dir); err != nil {
		return protocol.GitIndex{}, fault(protocol.ErrorCodeOutcomeUnknown, "Git index publication could not be confirmed")
	}
	s.Invalidate(root)
	sum := blake3.Sum256(candidate.Bytes())
	return protocol.GitIndex{Revision: fmtHash(sum[:])}, nil
}
func sameString(a, b *string) bool { return a == nil && b == nil || a != nil && b != nil && *a == *b }

// EditIndex transforms the current staged text and compares the observed working
// file revision again immediately before publishing the detached index candidate.
func (s *Service) EditIndex(ctx context.Context, root, path, expectedIndex string, expectedHead *string, expectedSource string, transform func(string) (string, error)) (protocol.GitIndex, error) {
	ctx, cancel := context.WithTimeout(ctx, readTimeout)
	defer cancel()
	if err := validPath(path); err != nil {
		return protocol.GitIndex{}, err
	}
	files, err := core.OpenFiles(root, core.RestrictedFiles)
	if err != nil {
		return protocol.GitIndex{}, err
	}
	defer files.Close()
	verify := func() error { _, err := files.CaptureText(ctx, path, expectedSource); return err }
	if err := verify(); err != nil {
		return protocol.GitIndex{}, err
	}
	return s.publishIndex(ctx, root, expectedIndex, expectedHead, func(r *gogit.Repository, idx *idxfmt.Index, _ *object.Tree) error {
		source, err := indexContent(r, path, protocol.MAX_EDIT_BYTES)
		if err != nil {
			return err
		}
		if source.exists && (source.mode != filemode.Regular && source.mode != filemode.Executable || bytes.IndexByte(source.data, 0) >= 0 || !utf8.Valid(source.data)) {
			return invalid("only regular text files support hunk staging")
		}
		if !source.exists {
			filter, err := NewWatchFilter(root)
			if err != nil {
				return err
			}
			if filter.matcher != nil && filter.matcher.Match(strings.Split(path, "/"), false) {
				return invalid("ignored files cannot be staged by this operation")
			}
			working, err := workingContent(root, path, protocol.MAX_EDIT_BYTES)
			if err != nil {
				return err
			}
			source.mode = working.mode
		}
		result, err := transform(string(source.data))
		if err != nil {
			return err
		}
		source.data = []byte(result)
		source.exists = true
		if source.mode == 0 {
			source.mode = filemode.Regular
		}
		return setIndexContent(r, idx, path, source)
	}, verify)
}

func setIndexReference(idx *idxfmt.Index, tree *object.Tree, path string) error {
	kept := idx.Entries[:0]
	for _, entry := range idx.Entries {
		if entry.Name != path {
			kept = append(kept, entry)
		}
	}
	idx.Entries = kept
	if tree == nil {
		return nil
	}
	entry, err := tree.FindEntry(path)
	if errors.Is(err, object.ErrEntryNotFound) || errors.Is(err, object.ErrFileNotFound) {
		return nil
	}
	if err != nil {
		return mapError(err)
	}
	idx.Entries = append(idx.Entries, &idxfmt.Entry{Name: path, Hash: entry.Hash, Mode: entry.Mode})
	return nil
}
