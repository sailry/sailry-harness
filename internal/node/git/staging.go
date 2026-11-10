package git

import (
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/filemode"
	idxfmt "github.com/go-git/go-git/v5/plumbing/format/index"
	"github.com/go-git/go-git/v5/storage/filesystem"
	"github.com/sailry/sailry-harness/internal/core"
)

func stageContent(ctx context.Context, root string, r *gogit.Repository, idx *idxfmt.Index, path string) (content, error) {
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
	if _, err := idx.Entry(path); err != nil {
		filter, e := NewWatchFilter(root)
		if e != nil {
			return content{}, e
		}
		if filter.matcher != nil && filter.matcher.Match(strings.Split(path, "/"), info.IsDir()) {
			return content{}, invalid("ignored files cannot be staged by this operation")
		}
	}
	if info.Mode()&os.ModeSymlink != 0 {
		scope, err := os.OpenRoot(root)
		if err != nil {
			return content{}, mapError(err)
		}
		defer scope.Close()
		link, err := scope.Readlink(filepath.FromSlash(path))
		if err != nil {
			return content{}, mapError(err)
		}
		bytes := []byte(link)
		return content{data: bytes, hash: plumbing.ComputeHash(plumbing.BlobObject, bytes), mode: filemode.Symlink, exists: true}, nil
	}
	if !info.Mode().IsRegular() {
		return content{}, invalid("expected a regular Git resource")
	}
	if err := checkContext(ctx); err != nil {
		return content{}, err
	}
	return workingContent(root, path, maxFile)
}

func stageIndexContent(ctx context.Context, root string, r *gogit.Repository, idx *idxfmt.Index, path string) error {
	target, err := safePath(root, path)
	if err != nil {
		return err
	}
	info, err := os.Lstat(target)
	if errors.Is(err, os.ErrNotExist) {
		return setIndexContent(r, idx, path, content{})
	}
	if err != nil {
		return mapError(err)
	}
	if info.Mode()&os.ModeSymlink != 0 {
		value, err := stageContent(ctx, root, r, idx, path)
		if err != nil {
			return err
		}
		return setIndexContent(r, idx, path, value)
	}
	if !info.Mode().IsRegular() {
		return invalid("expected a regular Git resource")
	}
	if _, err := idx.Entry(path); err != nil {
		filter, e := NewWatchFilter(root)
		if e != nil {
			return e
		}
		if filter.matcher != nil && filter.matcher.Match(strings.Split(path, "/"), false) {
			return invalid("ignored files cannot be staged by this operation")
		}
	}
	files, err := core.OpenFiles(root, core.RestrictedFiles)
	if err != nil {
		return err
	}
	defer files.Close()
	file, err := files.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()
	before, err := file.Stat()
	if err != nil {
		return mapError(err)
	}
	storage, ok := r.Storer.(*filesystem.Storage)
	if !ok {
		return invalid("repository has no file-backed object storage")
	}
	writer, header, err := storage.LazyWriter()
	if err != nil {
		return mapError(err)
	}
	if err := header(plumbing.BlobObject, before.Size()); err != nil {
		writer.Close()
		return mapError(err)
	}
	hash := plumbing.NewHasher(plumbing.BlobObject, before.Size())
	buffer := make([]byte, 64*1024)
	var count int64
	for {
		if err := checkContext(ctx); err != nil {
			writer.Close()
			return err
		}
		n, err := file.Read(buffer)
		if n > 0 {
			if _, e := writer.Write(buffer[:n]); e != nil {
				writer.Close()
				return mapError(e)
			}
			hash.Write(buffer[:n])
			count += int64(n)
		}
		if err == io.EOF {
			break
		}
		if err != nil {
			writer.Close()
			return mapError(err)
		}
	}
	after, err := file.Stat()
	if err != nil {
		writer.Close()
		return mapError(err)
	}
	if count != before.Size() || after.Size() != before.Size() || after.ModTime() != before.ModTime() || !os.SameFile(before, after) {
		writer.Close()
		return conflict()
	}
	if err := writer.Close(); err != nil {
		return mapError(err)
	}
	mode := filemode.Regular
	if before.Mode()&0111 != 0 {
		mode = filemode.Executable
	}
	kept := idx.Entries[:0]
	for _, entry := range idx.Entries {
		if entry.Name != path {
			kept = append(kept, entry)
		}
	}
	if err := syncBlob(r, hash.Sum()); err != nil {
		return err
	}
	idx.Entries = append(kept, &idxfmt.Entry{Name: path, Hash: hash.Sum(), Mode: mode, Size: uint32(before.Size())})
	return nil
}
