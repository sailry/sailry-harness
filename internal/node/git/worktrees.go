package git

import (
	"bytes"
	"context"
	"errors"
	"net/url"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/go-git/go-git/v5/plumbing"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (s *Service) ListWorktrees(ctx context.Context, root string) (protocol.GitWorktrees, error) {
	head, err := s.Head(ctx, root)
	result := protocol.GitWorktrees{Kind: head.Kind, Entries: []protocol.GitWorktree{}}
	if err != nil || head.Kind == protocol.RepositoryKindDirectory {
		return result, err
	}
	capture, err := s.command(ctx, root, []string{"worktree", "list", "--porcelain", "-z"}, nil, true)
	if err != nil {
		return result, err
	}
	return ParseWorktrees(capture.Stdout, head.Kind, capture.Truncated)
}
func ParseWorktrees(data []byte, kind protocol.RepositoryKind, truncated bool) (protocol.GitWorktrees, error) {
	result := protocol.GitWorktrees{Kind: kind, Entries: []protocol.GitWorktree{}, Truncated: truncated}
	if !utf8.Valid(data) {
		return result, invalid("worktree paths are not UTF-8")
	}
	if len(data) > 0 && data[len(data)-1] != 0 {
		return result, invalid("incomplete Git worktree listing")
	}
	var current *protocol.GitWorktree
	seen := map[string]bool{}
	for _, field := range strings.Split(string(data), "\x00") {
		if field == "" {
			if current != nil {
				if len(result.Entries) == 100 {
					result.Truncated = true
					result.OmittedPaths++
				} else if !seen[current.Path] {
					result.Entries = append(result.Entries, *current)
					seen[current.Path] = true
				}
				current = nil
			}
			continue
		}
		if path, ok := strings.CutPrefix(field, "worktree "); ok {
			if !filepath.IsAbs(path) || len(path) > 4096 || strings.ContainsFunc(path, unicode.IsControl) {
				result.OmittedPaths++
				current = nil
				continue
			}
			current = &protocol.GitWorktree{Path: path, Main: len(result.Entries) == 0}
			info, err := os.Stat(path)
			current.Available = err == nil && info.IsDir()
			continue
		}
		if current == nil {
			continue
		}
		if head, ok := strings.CutPrefix(field, "HEAD "); ok {
			if head != "0000000000000000000000000000000000000000" {
				current.Head = ptr(head)
			}
		} else if branch, ok := strings.CutPrefix(field, "branch refs/heads/"); ok {
			current.Branch = ptr(branch)
		} else if field == "locked" || strings.HasPrefix(field, "locked ") {
			current.Locked = true
		}
	}
	sort.Slice(result.Entries, func(i, j int) bool {
		a, b := result.Entries[i], result.Entries[j]
		if a.Main != b.Main {
			return a.Main
		}
		return a.Path < b.Path
	})
	return result, nil
}
func newDestination(value string) (string, error) {
	if !filepath.IsAbs(value) || len(value) > 4096 || strings.ContainsFunc(value, unicode.IsControl) {
		return "", invalid("expected an absolute new directory")
	}
	parent, err := filepath.EvalSymlinks(filepath.Dir(value))
	if err != nil {
		return "", mapError(err)
	}
	value = filepath.Join(parent, filepath.Base(value))
	if _, err := os.Lstat(value); !errors.Is(err, os.ErrNotExist) {
		if err == nil {
			return "", fault(protocol.ErrorCodeConflict, "destination already exists")
		}
		return "", mapError(err)
	}
	return value, nil
}
func (s *Service) Clone(ctx context.Context, repositoryURL, destination string, branch *string) (string, error) {
	if repositoryURL == "" || len(repositoryURL) > 4096 || strings.HasPrefix(repositoryURL, "-") || strings.ContainsFunc(repositoryURL, unicode.IsControl) {
		return "", invalid("invalid Git clone URL")
	}
	if parsed, e := url.Parse(repositoryURL); e == nil && parsed.Scheme != "" && !filepath.IsAbs(repositoryURL) {
		if parsed.Scheme != "http" && parsed.Scheme != "https" && parsed.Scheme != "ssh" && parsed.Scheme != "file" {
			return "", invalid("repository URL is invalid")
		}
		if parsed.User != nil {
			_, password := parsed.User.Password()
			if password || (parsed.Scheme == "http" || parsed.Scheme == "https") && parsed.User.Username() != "" {
				return "", invalid("use a repository URL without embedded credentials")
			}
		}
	} else if !filepath.IsAbs(repositoryURL) && !(strings.Contains(repositoryURL, "@") && strings.Contains(repositoryURL, ":")) {
		return "", invalid("repository URL is invalid")
	}
	target, err := newDestination(destination)
	if err != nil {
		return "", err
	}
	args := []string{"clone"}
	if branch != nil {
		if len(*branch) > 256 {
			return "", invalid("repository branch is invalid")
		}
		if err := branchName(*branch); err != nil {
			return "", err
		}
		args = append(args, "--branch", *branch)
	}
	args = append(args, "--", repositoryURL, target)
	if _, err := s.command(ctx, filepath.Dir(target), args, nil, false); err != nil {
		return "", fault(protocol.ErrorCodeOutcomeUnknown, "clone did not finish; inspect the retained destination directory")
	}
	canonical, err := filepath.EvalSymlinks(target)
	if err != nil {
		return "", fault(protocol.ErrorCodeOutcomeUnknown, "project clone could not be confirmed")
	}
	if _, err := open(canonical); err != nil {
		return "", fault(protocol.ErrorCodeOutcomeUnknown, "project clone could not be confirmed")
	}
	return canonical, nil
}
func (s *Service) CreateWorktree(ctx context.Context, root, destination, branch, commit string) (string, error) {
	if err := branchName(branch); err != nil {
		return "", err
	}
	if err := fullHash(commit); err != nil {
		return "", err
	}
	r, err := open(root)
	if err != nil {
		return "", err
	}
	if _, err := r.CommitObject(plumbing.NewHash(commit)); err != nil {
		return "", mapError(err)
	}
	if _, err := r.Reference(plumbing.NewBranchReferenceName(branch), true); err == nil {
		return "", fault(protocol.ErrorCodeConflict, "branch already exists")
	}
	target, err := newDestination(destination)
	if err != nil {
		return "", err
	}
	dir, err := commonDir(r)
	if err != nil {
		return "", err
	}
	if pathWithin(target, dir) {
		return "", invalid("worktree destination cannot be inside Git metadata")
	}
	if _, err := s.command(ctx, root, []string{"worktree", "add", "-b", branch, "--", target, commit}, nil, false); err != nil {
		return "", err
	}
	created, err := open(target)
	if err != nil {
		return "", fault(protocol.ErrorCodeOutcomeUnknown, "worktree creation could not be confirmed")
	}
	h, err := head(created)
	if err != nil || h.Head == nil || *h.Head != commit {
		return "", fault(protocol.ErrorCodeOutcomeUnknown, "worktree creation could not be confirmed")
	}
	s.Invalidate(root)
	return target, nil
}
func (s *Service) PrepareManaged(root string) (string, error) {
	r, err := open(root)
	if err != nil {
		return "", err
	}
	dir, err := gitDir(root, r)
	if err != nil {
		return "", err
	}
	if data, e := os.ReadFile(filepath.Join(dir, "commondir")); e == nil {
		dir = filepath.Clean(filepath.Join(dir, strings.TrimSpace(string(data))))
	}
	anchor, err := os.OpenRoot(dir)
	if err != nil {
		return "", mapError(err)
	}
	defer anchor.Close()
	if err := ensureDir(anchor, "info"); err != nil {
		return "", err
	}
	info, err := anchor.OpenRoot("info")
	if err != nil {
		return "", mapError(err)
	}
	defer info.Close()
	stat, e := info.Lstat("exclude")
	if e == nil && !stat.Mode().IsRegular() {
		return "", invalid("Git exclusion entry must be a regular file")
	}
	if e != nil && !errors.Is(e, os.ErrNotExist) {
		return "", mapError(e)
	}
	file, err := info.OpenFile("exclude", os.O_RDWR|os.O_CREATE|os.O_APPEND, 0600)
	if err != nil {
		return "", mapError(err)
	}
	defer file.Close()
	stat, err = file.Stat()
	if err != nil {
		return "", mapError(err)
	}
	if stat.Size() > 1024*1024 {
		return "", invalid("Git exclusion entry exceeds the size limit")
	}
	data := make([]byte, stat.Size())
	if len(data) > 0 {
		if _, err := file.ReadAt(data, 0); err != nil {
			return "", mapError(err)
		}
	}
	found := false
	for _, line := range bytes.Split(data, []byte{'\n'}) {
		if bytes.Equal(bytes.TrimSuffix(line, []byte{'\r'}), []byte(".sailry/")) {
			found = true
		}
	}
	if !found {
		if len(data) > 0 && data[len(data)-1] != '\n' {
			if _, err := file.WriteString("\n"); err != nil {
				return "", mapError(err)
			}
		}
		if _, err := file.WriteString(".sailry/\n"); err != nil {
			return "", mapError(err)
		}
		if err := file.Sync(); err != nil {
			return "", mapError(err)
		}
	}
	scope, err := os.OpenRoot(root)
	if err != nil {
		return "", mapError(err)
	}
	defer scope.Close()
	if err := ensureDir(scope, ".sailry"); err != nil {
		return "", err
	}
	managed, err := scope.OpenRoot(".sailry")
	if err != nil {
		return "", mapError(err)
	}
	defer managed.Close()
	if err := ensureDir(managed, "worktrees"); err != nil {
		return "", err
	}
	return filepath.Join(root, ".sailry", "worktrees"), nil
}
func ensureDir(root *os.Root, path string) error {
	err := root.Mkdir(path, 0700)
	if errors.Is(err, os.ErrExist) {
		info, e := root.Lstat(path)
		if e != nil {
			return mapError(e)
		}
		if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			return fault(protocol.ErrorCodeConflict, "repository metadata directory is unavailable")
		}
		return nil
	}
	return mapError(err)
}
func (s *Service) RemoveWorktree(ctx context.Context, root, expectedHead, expectedBranch string) error {
	r, err := open(root)
	if err != nil {
		return err
	}
	dir, err := gitDir(root, r)
	if err != nil {
		return err
	}
	if _, err := os.Lstat(filepath.Join(dir, "commondir")); err != nil {
		return fault(protocol.ErrorCodeConflict, "cannot remove the main worktree")
	}
	list, err := s.ListWorktrees(ctx, root)
	if err != nil {
		return err
	}
	var main string
	found := false
	for _, entry := range list.Entries {
		if entry.Main {
			main = entry.Path
		}
		if entry.Path == root {
			found = true
			if entry.Locked {
				return fault(protocol.ErrorCodeBusy, "worktree is locked")
			}
			if entry.Main {
				return fault(protocol.ErrorCodeConflict, "cannot remove the main worktree")
			}
		} else if pathWithin(entry.Path, root) {
			return fault(protocol.ErrorCodeConflict, "worktree contains another Git worktree")
		}
	}
	if !found || main == "" {
		return conflict()
	}
	h, err := head(r)
	if err != nil {
		return err
	}
	if h.Head == nil || *h.Head != expectedHead || h.Branch == nil || *h.Branch != expectedBranch {
		return conflict()
	}
	if operation(root, r) != nil {
		return fault(protocol.ErrorCodeConflict, "finish the current Git operation before removing the worktree")
	}
	s.Invalidate(root)
	status, err := s.Status(ctx, root)
	if err != nil {
		return err
	}
	if len(status.Entries) > 0 || status.Truncated || status.OmittedPaths > 0 {
		return fault(protocol.ErrorCodeConflict, "worktree has uncommitted changes")
	}
	for _, lock := range []string{"index.lock", "HEAD.lock"} {
		if _, e := os.Lstat(filepath.Join(dir, lock)); e == nil {
			return fault(protocol.ErrorCodeBusy, "worktree has an active Git lock")
		}
	}
	if _, err := s.command(ctx, main, []string{"worktree", "remove", "--", root}, nil, false); err != nil {
		return err
	}
	if _, err := os.Lstat(root); !errors.Is(err, os.ErrNotExist) {
		return fault(protocol.ErrorCodeOutcomeUnknown, "worktree removal may be partial")
	}
	s.Invalidate(main)
	return nil
}
func (s *Service) PruneWorktree(ctx context.Context, root, path string) error {
	if !filepath.IsAbs(path) {
		return invalid("worktree path must be absolute")
	}
	entries, err := s.ListWorktrees(ctx, root)
	if err != nil {
		return err
	}
	for _, entry := range entries.Entries {
		if entry.Path != path {
			continue
		}
		if entry.Locked {
			return fault(protocol.ErrorCodeBusy, "worktree is locked")
		}
		if _, err := os.Lstat(path); !errors.Is(err, os.ErrNotExist) {
			if err == nil {
				return fault(protocol.ErrorCodeConflict, "worktree path still exists")
			}
			return mapError(err)
		}
		_, err := s.command(ctx, root, []string{"worktree", "remove", "--", path}, nil, false)
		return err
	}
	return fault(protocol.ErrorCodeNotFound, "linked worktree registration not found")
}
