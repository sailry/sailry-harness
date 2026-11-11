package git

import (
	"context"
	"errors"
	"os"
	"strings"

	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/filemode"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func IgnorePattern(path string, directory bool) string {
	var b strings.Builder
	b.WriteByte('/')
	for _, r := range path {
		if strings.ContainsRune("*?[]\\ #!", r) {
			b.WriteByte('\\')
		}
		b.WriteRune(r)
	}
	if directory {
		b.WriteByte('/')
	}
	return b.String()
}
func (s *Service) Ignore(ctx context.Context, root string, paths []string, directories bool) error {
	if err := pathsValid(paths); err != nil {
		return err
	}
	r, err := open(root)
	if err != nil {
		return err
	}
	idx, err := r.Storer.Index()
	if err != nil {
		return mapError(err)
	}
	for _, path := range paths {
		target, e := safePath(root, path)
		if e != nil {
			return e
		}
		info, e := os.Lstat(target)
		if e != nil {
			return mapError(e)
		}
		if directories {
			if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
				return invalid("ignore target is not a directory")
			}
		} else {
			if !info.Mode().IsRegular() && info.Mode()&os.ModeSymlink == 0 {
				return invalid("ignore target is not a file")
			}
			if _, e := idx.Entry(path); e == nil {
				return fault(protocol.ErrorCodeConflict, "tracked files cannot be ignored")
			}
		}
	}
	files, err := core.OpenFiles(root, core.RestrictedFiles)
	if err != nil {
		return err
	}
	defer files.Close()
	text := ""
	var expected *string
	value, e := files.ReadContent(ctx, ".gitignore")
	if e == nil {
		if value.Truncated {
			return invalid("ignore file exceeds the edit limit")
		}
		text = value.Text
		expected = value.Revision
	} else {
		var f *protocol.Fault
		if !errors.As(e, &f) || f.Code != protocol.ErrorCodeNotFound {
			return e
		}
	}
	ending := "\n"
	if strings.Contains(text, "\r\n") {
		ending = "\r\n"
	}
	original := text
	seen := map[string]bool{}
	for _, path := range paths {
		rule := IgnorePattern(path, directories)
		if seen[rule] {
			continue
		}
		seen[rule] = true
		if text != "" && !strings.HasSuffix(text, "\n") {
			text += ending
		}
		text += rule + ending
	}
	if text != original {
		_, err = files.Save(ctx, ".gitignore", text, expected)
	}
	return err
}
func (s *Service) trash(ctx context.Context, root string, paths []string, untracked bool) error {
	if err := pathsValid(paths); err != nil {
		return err
	}
	if untracked {
		status, err := s.Status(ctx, root)
		if err != nil {
			return err
		}
		selected := map[string]bool{}
		for _, e := range status.Entries {
			if e.Untracked {
				selected[e.Path] = true
			}
		}
		for _, path := range paths {
			if !selected[path] {
				return conflict()
			}
		}
	}
	files, err := core.OpenFiles(root, core.RestrictedFiles)
	if err != nil {
		return err
	}
	defer files.Close()
	for i, path := range paths {
		if err := files.Trash(path); err != nil {
			if i > 0 {
				return fault(protocol.ErrorCodeOutcomeUnknown, "some entries moved to Trash; inspect repository before retrying")
			}
			return err
		}
	}
	return nil
}

// heldByHead reports whether the HEAD tree records an entry at path. It reads
// no blob, so it costs the same for a large file. An unborn branch holds nothing.
func heldByHead(tree *object.Tree, path string) (bool, error) {
	entry, err := headEntry(tree, path)
	return entry != nil, err
}

// headEntry finds the entry the HEAD tree records at path, nil when it records
// none. A path below a file or submodule of HEAD is not recorded: go-git
// reports the object there, which is not a tree, as an object it cannot find.
func headEntry(tree *object.Tree, path string) (*object.TreeEntry, error) {
	if tree == nil {
		return nil, nil
	}
	entry, err := tree.FindEntry(path)
	switch {
	case err == nil:
		return entry, nil
	case errors.Is(err, object.ErrEntryNotFound), errors.Is(err, object.ErrFileNotFound), errors.Is(err, object.ErrDirectoryNotFound):
		return nil, nil
	case errors.Is(err, plumbing.ErrObjectNotFound):
		for end := strings.IndexByte(path, '/'); end >= 0; {
			parent, e := tree.FindEntry(path[:end])
			if e != nil {
				break
			}
			if parent.Mode != filemode.Dir {
				return nil, nil
			}
			next := strings.IndexByte(path[end+1:], '/')
			if next < 0 {
				break
			}
			end += next + 1
		}
	}
	return nil, mapError(err)
}
func (s *Service) discardAll(ctx context.Context, root string, paths []string) error {
	if err := pathsValid(paths); err != nil {
		return err
	}
	r, err := open(root)
	if err != nil {
		return err
	}
	tree, err := headTree(r)
	if err != nil {
		return err
	}
	tracked, added := []string{}, []string{}
	for _, path := range paths {
		if _, err := safePath(root, path); err != nil {
			return err
		}
		if held, err := heldByHead(tree, path); err != nil {
			return err
		} else if held {
			tracked = append(tracked, path)
		} else {
			added = append(added, path)
		}
	}
	changed := false
	run := func(args []string) error {
		_, err := s.command(ctx, root, args, nil, false)
		if err != nil && changed {
			return fault(protocol.ErrorCodeOutcomeUnknown, "some changes were discarded before the operation failed")
		}
		if err == nil {
			changed = true
		}
		return err
	}
	if len(tracked) > 0 {
		args, err := CLIArgs(protocol.GitAction{Kind: "discard", Data: protocol.GitActionDiscardPayload{Paths: tracked}}, "", "", 0)
		if err != nil {
			return err
		}
		if err := run(args); err != nil {
			return err
		}
	}
	for _, path := range added {
		if err := run([]string{"rm", "--cached", "--force", "--ignore-unmatch", "--", ":(literal)" + path}); err != nil {
			return err
		}
		if _, err := os.Lstat(root + string(os.PathSeparator) + path); err == nil {
			if err := s.trash(ctx, root, []string{path}, false); err != nil {
				return fault(protocol.ErrorCodeOutcomeUnknown, "some changes were discarded before the operation failed")
			}
		}
	}
	return nil
}
