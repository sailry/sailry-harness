package git

import (
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/filemode"
)

// storedBatch is the number of paths one hash-object process answers within
// the bounded capture of its output.
const storedBatch = 1000

// storedIDs asks Git for the blob it would store for worktree files: the id of
// each file's content after the conversion of its path (line endings, ident,
// encoding and a filter driver such as LFS, which Git runs itself). Callers
// pass only paths Git has to read again, so a file whose stat data matches the
// index never reaches a filter. Only regular files up to limit bytes are
// asked for; a path that is missing from the result, also after a failed or
// stopped process, has no known stored form and is compared by its bytes.
// One process answers a batch of paths and nothing is written.
func (s *Service) storedIDs(ctx context.Context, root string, paths []string, limit int64) map[string]plumbing.Hash {
	ids := map[string]plumbing.Hash{}
	regular := make([]string, 0, len(paths))
	for _, path := range paths {
		// Git reads a line that starts with a quote as a quoted path.
		if strings.HasPrefix(path, `"`) {
			continue
		}
		info, err := os.Lstat(filepath.Join(root, filepath.FromSlash(path)))
		if err == nil && info.Mode().IsRegular() && info.Size() <= limit {
			regular = append(regular, path)
		}
	}
	for len(regular) > 0 && ctx.Err() == nil {
		batch := regular[:min(len(regular), storedBatch)]
		regular = regular[len(batch):]
		limited, cancel := context.WithTimeout(ctx, readTimeout)
		capture, err := s.run(limited, root, []string{"hash-object", "--stdin-paths"}, []byte(strings.Join(batch, "\n")+"\n"))
		cancel()
		if err != nil || capture.ExitCode != 0 || capture.Truncated {
			continue
		}
		lines := strings.Split(strings.TrimSuffix(string(capture.Stdout), "\n"), "\n")
		if len(lines) != len(batch) {
			continue
		}
		for i, line := range lines {
			if plumbing.IsHash(line) {
				ids[batch[i]] = plumbing.NewHash(line)
			}
		}
	}
	return ids
}

// filtered reports which of the paths are stored through a configured filter
// driver, such as Git LFS. Git answers from the attributes alone, so no filter
// program runs, and a repository without filter drivers is not asked.
func (s *Service) filtered(ctx context.Context, root string, r *gogit.Repository, paths []string) map[string]bool {
	drivers, err := filterDrivers(r)
	if err != nil || len(drivers) == 0 {
		return nil
	}
	found := map[string]bool{}
	for len(paths) > 0 && ctx.Err() == nil {
		batch := paths[:min(len(paths), storedBatch)]
		paths = paths[len(batch):]
		limited, cancel := context.WithTimeout(ctx, readTimeout)
		capture, err := s.run(limited, root, []string{"check-attr", "-z", "--stdin", "filter"}, []byte(strings.Join(batch, "\x00")+"\x00"))
		cancel()
		if err != nil || capture.ExitCode != 0 {
			continue
		}
		fields := bytes.Split(capture.Stdout, []byte{0})
		for i := 0; i+2 < len(fields); i += 3 {
			if slices.Contains(drivers, string(fields[i+2])) {
				found[string(fields[i])] = true
			}
		}
	}
	return found
}

// opaque is a worktree file that is not compared as text: it changed, and its
// size is all that is said of it. Nothing of it is read.
func opaque(root, path string) (content, error) {
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
	mode := filemode.Regular
	if info.Mode()&0111 != 0 {
		mode = filemode.Executable
	}
	return content{mode: mode, exists: true, binary: true, size: info.Size()}, nil
}

// stored turns the bytes of a worktree file into the content Git would store
// for it, given the id Git computed. Equal ids mean no conversion. A file
// that differs by line endings is converted in memory and checked against the
// id. Any other stored form is not compared as text.
func stored(working content, id plumbing.Hash) content {
	if id.IsZero() || id == working.hash {
		return working
	}
	if bytes.Contains(working.data, []byte("\r\n")) {
		data := bytes.ReplaceAll(working.data, []byte("\r\n"), []byte("\n"))
		if plumbing.ComputeHash(plumbing.BlobObject, data) == id {
			return content{data: data, hash: id, mode: working.mode, exists: true, size: working.size, converted: true}
		}
	}
	return content{hash: id, mode: working.mode, exists: true, binary: true, large: working.large, size: working.size}
}

// storedContent reads a worktree file for a comparison with what Git holds.
// A file behind a filter driver is not read: its stored form is the driver's,
// and it is shown by its size whether or not the driver can run.
func (s *Service) storedContent(ctx context.Context, root string, r *gogit.Repository, path string, limit int) (content, error) {
	if s.filtered(ctx, root, r, []string{path})[path] {
		return opaque(root, path)
	}
	working, err := workingContent(root, path, limit)
	if err != nil || !working.exists {
		return working, err
	}
	id := s.storedIDs(ctx, root, []string{path}, maxFile)[path]
	if err := checkContext(ctx); err != nil {
		return content{}, err
	}
	return stored(working, id), nil
}
