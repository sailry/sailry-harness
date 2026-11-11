package git

import (
	"bytes"
	"context"
	"errors"
	"io"
	"os"
	"strings"
	"time"
	"unicode/utf8"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/filemode"
	fdiff "github.com/go-git/go-git/v5/plumbing/format/diff"
	"github.com/go-git/go-git/v5/plumbing/object"
	udiff "github.com/go-git/go-git/v5/utils/diff"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sergi/go-diff/diffmatchpatch"
)

type content struct {
	data   []byte
	hash   plumbing.Hash
	mode   filemode.FileMode
	exists bool
	binary bool
	// large marks a side over the inspection limit, which is not read.
	large bool
}

func blobContent(r *gogit.Repository, hash plumbing.Hash, mode filemode.FileMode, limit int) (content, error) {
	if mode == filemode.Submodule {
		return content{data: []byte("Subproject commit " + hash.String() + "\n"), hash: hash, mode: mode, exists: true}, nil
	}
	blob, err := r.BlobObject(hash)
	if err != nil {
		return content{}, mapError(err)
	}
	if blob.Size > int64(limit) {
		if limit == 1024*1024 {
			return content{hash: hash, mode: mode, exists: true, binary: true, large: true}, nil
		}
		return content{}, invalid("Git file exceeds the operation size limit")
	}
	reader, err := blob.Reader()
	if err != nil {
		return content{}, mapError(err)
	}
	defer reader.Close()
	data, err := io.ReadAll(io.LimitReader(reader, int64(limit)+1))
	if err != nil {
		return content{}, mapError(err)
	}
	return content{data: data, hash: hash, mode: mode, exists: true}, nil
}
func treeContent(r *gogit.Repository, tree *object.Tree, path string, limit int) (content, error) {
	if tree == nil {
		return content{}, nil
	}
	entry, err := tree.FindEntry(path)
	if errors.Is(err, object.ErrEntryNotFound) || errors.Is(err, object.ErrFileNotFound) || errors.Is(err, object.ErrDirectoryNotFound) {
		return content{}, nil
	}
	if err != nil {
		return content{}, mapError(err)
	}
	if entry.Mode == filemode.Dir {
		return content{}, invalid("Git inspection requires a file")
	}
	return blobContent(r, entry.Hash, entry.Mode, limit)
}
func headTree(r *gogit.Repository) (*object.Tree, error) {
	ref, err := r.Head()
	if errors.Is(err, plumbing.ErrReferenceNotFound) {
		return nil, nil
	}
	if err != nil {
		return nil, mapError(err)
	}
	commit, err := r.CommitObject(ref.Hash())
	if err != nil {
		return nil, mapError(err)
	}
	tree, err := commit.Tree()
	return tree, mapError(err)
}
func indexContent(r *gogit.Repository, path string, limit int) (content, error) {
	idx, err := r.Storer.Index()
	if err != nil {
		return content{}, mapError(err)
	}
	entry, err := idx.Entry(path)
	if err != nil {
		return content{}, nil
	}
	return blobContent(r, entry.Hash, entry.Mode, limit)
}
func workingContent(root, path string, limit int) (content, error) {
	target, err := safePath(root, path)
	if err != nil {
		return content{}, err
	}
	info, err := os.Lstat(target)
	if errors.Is(err, os.ErrNotExist) {
		return content{}, nil
	}
	if err != nil {
		return content{}, mapError(err)
	}
	if info.Mode()&os.ModeSymlink != 0 {
		scope, err := os.OpenRoot(root)
		if err != nil {
			return content{}, mapError(err)
		}
		defer scope.Close()
		link, err := scope.Readlink(path)
		if err != nil {
			return content{}, mapError(err)
		}
		data := []byte(link)
		return content{data: data, hash: plumbing.ComputeHash(plumbing.BlobObject, data), mode: filemode.Symlink, exists: true}, nil
	}
	if !info.Mode().IsRegular() {
		return content{}, invalid("Git inspection requires a regular worktree file")
	}
	if info.Size() > int64(limit) && limit != 1024*1024 {
		return content{}, invalid("Git file exceeds the operation size limit")
	}
	scope, err := core.OpenFiles(root, core.RestrictedFiles)
	if err != nil {
		return content{}, err
	}
	defer scope.Close()
	file, err := scope.Open(path)
	if err != nil {
		return content{}, err
	}
	defer file.Close()
	if info.Size() > int64(limit) {
		hash := plumbing.NewHasher(plumbing.BlobObject, info.Size())
		if _, err := io.Copy(hash, file); err != nil {
			return content{}, mapError(err)
		}
		mode := filemode.Regular
		if info.Mode()&0111 != 0 {
			mode = filemode.Executable
		}
		return content{hash: hash.Sum(), mode: mode, exists: true, binary: true, large: true}, nil
	}
	data, err := io.ReadAll(io.LimitReader(file, int64(limit)+1))
	if err != nil {
		return content{}, mapError(err)
	}
	if len(data) > limit {
		return content{}, invalid("Git file exceeds the operation size limit")
	}
	mode := filemode.Regular
	if info.Mode()&0111 != 0 {
		mode = filemode.Executable
	}
	return content{data: data, hash: plumbing.ComputeHash(plumbing.BlobObject, data), mode: mode, exists: true}, nil
}
func (s *Service) contents(ctx context.Context, root, path string, scope protocol.GitDiffScope) (content, content, error) {
	if err := validPath(path); err != nil {
		return content{}, content{}, err
	}
	if err := checkContext(ctx); err != nil {
		return content{}, content{}, err
	}
	r, err := open(root)
	if err != nil {
		return content{}, content{}, err
	}
	var before, after content
	switch scope {
	case protocol.GitDiffScopeStaged:
		tree, e := headTree(r)
		if e != nil {
			return before, after, e
		}
		before, err = treeContent(r, tree, path, 1024*1024)
		if err == nil {
			after, err = indexContent(r, path, 1024*1024)
		}
	case protocol.GitDiffScopeUnstaged:
		before, err = indexContent(r, path, 1024*1024)
		if err == nil {
			after, err = workingContent(root, path, 1024*1024)
		}
	case protocol.GitDiffScopeAll:
		tree, e := headTree(r)
		if e != nil {
			return before, after, e
		}
		before, err = treeContent(r, tree, path, 1024*1024)
		if err == nil {
			after, err = workingContent(root, path, 1024*1024)
		}
	default:
		err = invalid("invalid Git diff scope")
	}
	return before, after, err
}
func (s *Service) Diff(ctx context.Context, root, path string, scope protocol.GitDiffScope) (protocol.GitDiff, error) {
	a, b, err := s.contents(ctx, root, path, scope)
	if err != nil {
		return protocol.GitDiff{}, err
	}
	result, err := renderDiff(path, scope, a, b)
	if err == nil {
		err = checkContext(ctx)
	}
	return result, err
}

type patchFile struct {
	path  string
	value content
}

func (f patchFile) Hash() plumbing.Hash     { return f.value.hash }
func (f patchFile) Mode() filemode.FileMode { return f.value.mode }
func (f patchFile) Path() string            { return f.path }

type patchChunk struct {
	text      string
	operation fdiff.Operation
}

func (c patchChunk) Content() string       { return c.text }
func (c patchChunk) Type() fdiff.Operation { return c.operation }

type filePatch struct {
	from, to fdiff.File
	chunks   []fdiff.Chunk
	binary   bool
}

func (p filePatch) Files() (fdiff.File, fdiff.File) { return p.from, p.to }
func (p filePatch) Chunks() []fdiff.Chunk           { return p.chunks }
func (p filePatch) IsBinary() bool                  { return p.binary }

type patchSet []fdiff.FilePatch

func (p patchSet) FilePatches() []fdiff.FilePatch { return p }
func (p patchSet) Message() string                { return "" }

// chunks diffs two texts by line within the read deadline its callers run under.
func chunks(a, b []byte) []fdiff.Chunk { return chunksWithin(a, b, readTimeout) }

// chunksWithin stops refining at the limit: the text not yet compared becomes
// one bulk delete and insert, which is a coarser but still correct diff.
func chunksWithin(a, b []byte, limit time.Duration) []fdiff.Chunk {
	result := []fdiff.Chunk{}
	for _, d := range udiff.DoWithTimeout(string(a), string(b), limit) {
		op := fdiff.Equal
		if d.Type == diffmatchpatch.DiffDelete {
			op = fdiff.Delete
		} else if d.Type == diffmatchpatch.DiffInsert {
			op = fdiff.Add
		}
		result = append(result, patchChunk{d.Text, op})
	}
	return result
}
func renderDiff(path string, scope protocol.GitDiffScope, a, b content) (protocol.GitDiff, error) {
	result := protocol.GitDiff{Path: path, Scope: scope}
	if a.exists == b.exists && a.hash == b.hash && a.mode == b.mode {
		return result, nil
	}
	patch := filePatch{binary: a.binary || b.binary || bytes.IndexByte(a.data, 0) >= 0 || bytes.IndexByte(b.data, 0) >= 0 || !utf8.Valid(a.data) || !utf8.Valid(b.data)}
	if a.exists {
		patch.from = patchFile{path, a}
	}
	if b.exists {
		patch.to = patchFile{path, b}
	}
	result.Binary = patch.binary
	if !patch.binary {
		patch.chunks = chunks(a.data, b.data)
		for _, c := range patch.chunks {
			lines := strings.Count(c.Content(), "\n")
			if c.Content() != "" && !strings.HasSuffix(c.Content(), "\n") {
				lines++
			}
			if c.Type() == fdiff.Add {
				result.Additions += uint64(lines)
			} else if c.Type() == fdiff.Delete {
				result.Deletions += uint64(lines)
			}
		}
	}
	var output bytes.Buffer
	if err := fdiff.NewUnifiedEncoder(&output, 3).Encode(patchSet{patch}); err != nil {
		return result, mapError(err)
	}
	result.Text, result.Truncated = bounded(output.String(), maxDiff)
	return result, nil
}
