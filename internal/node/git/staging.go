package git

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os"
	"strings"

	"github.com/go-git/go-git/v5/plumbing"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// stage adds whole paths with native git add. Git applies each path's clean
// filter and line-ending conversion, which a blob written from the worktree
// bytes bypasses, and it resolves a conflicted path as it does anywhere else.
func (s *Service) stage(ctx context.Context, root string, input protocol.CommandUpdateGitIndexPayload) (protocol.GitIndex, error) {
	r, err := open(root)
	if err != nil {
		return protocol.GitIndex{}, err
	}
	revision, err := indexRevision(root, r)
	if err != nil {
		return protocol.GitIndex{}, err
	}
	current, err := head(r)
	if err != nil {
		return protocol.GitIndex{}, err
	}
	if revision != input.ExpectedIndex || !sameString(current.Head, input.ExpectedHead) {
		return protocol.GitIndex{}, conflict()
	}
	idx, err := r.Storer.Index()
	if err != nil {
		return protocol.GitIndex{}, mapError(err)
	}
	tracked := map[string]bool{}
	for _, entry := range idx.Entries {
		tracked[entry.Name] = true
	}
	// One ignore filter serves every untracked path of the update.
	var filter *WatchFilter
	var paths bytes.Buffer
	for _, path := range input.Paths {
		target, err := safePath(root, path)
		if err != nil {
			return protocol.GitIndex{}, err
		}
		info, err := os.Lstat(target)
		switch {
		case errors.Is(err, os.ErrNotExist):
			// A path neither on disk nor in the index has nothing to stage.
			below := false
			for _, entry := range idx.Entries {
				if below = strings.HasPrefix(entry.Name, path+"/"); below {
					break
				}
			}
			if !tracked[path] && !below {
				continue
			}
		case err != nil:
			return protocol.GitIndex{}, mapError(err)
		case !info.Mode().IsRegular() && info.Mode()&os.ModeSymlink == 0:
			return protocol.GitIndex{}, invalid("expected a regular Git resource")
		case !tracked[path]:
			if filter == nil {
				if filter, err = NewWatchFilter(root); err != nil {
					return protocol.GitIndex{}, err
				}
			}
			if filter.matcher != nil && filter.matcher.Match(strings.Split(path, "/"), false) {
				return protocol.GitIndex{}, invalid("ignored files cannot be staged by this operation")
			}
		}
		paths.WriteString(path)
		paths.WriteByte(0)
	}
	if paths.Len() == 0 {
		return protocol.GitIndex{Revision: revision}, nil
	}
	defer s.Invalidate(root)
	if _, err := s.command(ctx, root, []string{"--literal-pathspecs", "add", "--all", "--pathspec-from-file=-", "--pathspec-file-nul"}, paths.Bytes(), false); err != nil {
		return protocol.GitIndex{}, err
	}
	next, err := indexRevision(root, r)
	return protocol.GitIndex{Revision: next}, err
}

// requireUnfiltered refuses a path stored through a filter driver such as LFS.
// Its index content is what the driver produced, so text assembled from that
// content and worktree lines is not a version of the file.
func (s *Service) requireUnfiltered(ctx context.Context, root, path string) error {
	capture, err := s.command(ctx, root, []string{"check-attr", "-z", "filter", "--", path}, nil, true)
	if err != nil {
		return err
	}
	fields := bytes.Split(capture.Stdout, []byte{0})
	if len(fields) < 3 {
		return invalid("Git attributes could not be read")
	}
	switch driver := string(fields[2]); driver {
	case "unspecified", "unset", "set":
		return nil
	default:
		return invalid(fmt.Sprintf("this file is stored through the Git filter %q; stage the whole file", driver))
	}
}

// cleanBlob stores text as the blob Git would store for path, applying its
// line-ending conversion as git add does.
func (s *Service) cleanBlob(ctx context.Context, root, path string, text []byte) (plumbing.Hash, error) {
	capture, err := s.command(ctx, root, []string{"hash-object", "-w", "--path=" + path, "--stdin"}, text, true)
	if err != nil {
		return plumbing.ZeroHash, err
	}
	id := strings.TrimSpace(string(capture.Stdout))
	if !plumbing.IsHash(id) {
		return plumbing.ZeroHash, invalid("Git stored no object for the staged text")
	}
	return plumbing.NewHash(id), nil
}
