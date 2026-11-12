package git

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"strings"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/sailry/sailry-harness/internal/protocol"
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

// stored turns the bytes of a worktree file into the content Git would store
// for it, given the id Git computed. Equal ids mean no conversion. A file
// that differs by line endings is converted here and checked against the id.
// Any other stored form is read from the repository when it is there, which
// it is for an unchanged file. A changed file behind a filter driver is not:
// with write, Git stores the blob as git add would and it is read back;
// without, the side stays unread and its lines uncounted.
func (s *Service) stored(ctx context.Context, root string, r *gogit.Repository, path string, working content, id plumbing.Hash, limit int, write bool) (content, error) {
	if id.IsZero() || id == working.hash {
		return working, nil
	}
	if bytes.Contains(working.data, []byte("\r\n")) {
		data := bytes.ReplaceAll(working.data, []byte("\r\n"), []byte("\n"))
		if plumbing.ComputeHash(plumbing.BlobObject, data) == id {
			return content{data: data, hash: id, mode: working.mode, exists: true, converted: true}, nil
		}
	}
	if _, err := r.BlobObject(id); err != nil {
		if !write {
			return content{hash: id, mode: working.mode, exists: true, binary: true, large: true, converted: true}, nil
		}
		limited, cancel := context.WithTimeout(ctx, readTimeout)
		capture, err := s.run(limited, root, []string{"hash-object", "-w", "--", path}, nil)
		cancel()
		if err != nil {
			return content{}, mapError(err)
		}
		if id = plumbing.NewHash(strings.TrimSpace(string(capture.Stdout))); capture.ExitCode != 0 || id.IsZero() {
			return content{}, fault(protocol.ErrorCodeUnavailable, "Git could not convert the file for comparison")
		}
	}
	value, err := blobContent(r, id, working.mode, limit)
	value.converted = true
	return value, err
}

// storedContent reads a worktree file as Git would store it.
func (s *Service) storedContent(ctx context.Context, root string, r *gogit.Repository, path string, limit int) (content, error) {
	working, err := workingContent(root, path, limit)
	if err != nil || !working.exists {
		return working, err
	}
	id := s.storedIDs(ctx, root, []string{path}, maxFile)[path]
	if err := checkContext(ctx); err != nil {
		return content{}, err
	}
	return s.stored(ctx, root, r, path, working, id, limit, true)
}
