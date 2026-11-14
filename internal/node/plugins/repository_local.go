package plugins

import (
	"context"
	"errors"
	"io"
	"net/url"
	"os"
	"path/filepath"
	"runtime"
	"strings"

	git "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/cache"
	"github.com/go-git/go-git/v5/plumbing/filemode"
	"github.com/go-git/go-git/v5/plumbing/format/packfile"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/go-git/go-git/v5/storage/filesystem"
)

type localRepositoryObjects struct {
	*filesystem.Storage
	ctx context.Context
}

func (objects *localRepositoryObjects) EncodedObject(kind plumbing.ObjectType, hash plumbing.Hash) (plumbing.EncodedObject, error) {
	if err := objects.ctx.Err(); err != nil {
		return nil, err
	}
	return objects.Storage.EncodedObject(kind, hash)
}

// The pinned file transport waits for upload-pack without closing unread output.
// An exceeded transfer limit can therefore deadlock its process cleanup. Local
// acquisition instead encodes the fixed commit and tree, matching depth one,
// into a file we own before importing its complete pack.
func acquireLocalRepository(ctx context.Context, location *url.URL, revision, directory string, destination *transferStorage) (hash plumbing.Hash, resultErr error) {
	if err := ctx.Err(); err != nil {
		return hash, err
	}
	sourcePath := filepath.FromSlash(location.Path)
	if runtime.GOOS == "windows" && strings.HasPrefix(location.Path, "/") {
		if native := filepath.FromSlash(location.Path[1:]); filepath.VolumeName(native) != "" {
			sourcePath = native
		}
	}
	source, err := git.PlainOpenWithOptions(sourcePath, &git.PlainOpenOptions{EnableDotGitCommonDir: true})
	if err != nil {
		return hash, err
	}
	sourceFiles := source.Storer.(*filesystem.Storage)
	defer sourceFiles.Close()
	// Encoder selection retains object metadata before writing any bytes.
	// Stream payloads so that this phase cannot preload the entire tree.
	objects := &localRepositoryObjects{Storage: filesystem.NewStorageWithOptions(sourceFiles.Filesystem(), cache.NewObjectLRUDefault(), filesystem.Options{LargeObjectThreshold: 1}), ctx: ctx}
	defer func() { resultErr = errors.Join(resultErr, objects.Close()) }()
	source.Storer = objects
	fixed, err := source.ResolveRevision(plumbing.Revision(revision))
	if err != nil {
		return hash, plumbing.ErrReferenceNotFound
	}
	commit, err := source.CommitObject(*fixed)
	if err != nil {
		return hash, err
	}
	tree, err := commit.Tree()
	if err != nil {
		return hash, err
	}
	hashes := []plumbing.Hash{commit.Hash, tree.Hash}
	seen := map[plumbing.Hash]bool{commit.Hash: true, tree.Hash: true}
	walker := object.NewTreeWalker(tree, true, seen)
	// Only object identities are enumerated here; package capture validates paths.
	walker.SkipPathValidation()
	defer walker.Close()
	for {
		if err := ctx.Err(); err != nil {
			return hash, err
		}
		_, entry, err := walker.Next()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return hash, err
		}
		if entry.Mode != filemode.Submodule && !seen[entry.Hash] {
			seen[entry.Hash] = true
			hashes = append(hashes, entry.Hash)
		}
	}
	pack, err := os.OpenFile(filepath.Join(directory, "source.pack"), os.O_RDWR|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		return hash, err
	}
	defer func() { resultErr = errors.Join(resultErr, pack.Close()) }()
	bounded := &transferWriter{WriteCloser: pack, storage: destination, ctx: ctx}
	// Delta selection is unnecessary for a single tree and can preload blobs.
	if _, err := packfile.NewEncoder(bounded, objects, false).Encode(hashes, 0); err != nil {
		return hash, err
	}
	if err := ctx.Err(); err != nil {
		return hash, err
	}
	if _, err := pack.Seek(0, io.SeekStart); err != nil {
		return hash, err
	}
	if err := packfile.UpdateObjectStorage(destination.Storage, pack); err != nil {
		return hash, err
	}
	return commit.Hash, ctx.Err()
}
