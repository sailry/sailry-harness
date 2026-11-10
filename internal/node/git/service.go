// Package git owns bounded repository reads and native Git mutations on Node.
// Behavioral adaptations retain sailry-code 67ae9fa0, Apache-2.0 provenance.
package git

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"
	"unicode"
	"unicode/utf8"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/storage/filesystem"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const (
	maxEntries  = 500
	maxDiff     = 48 * 1024
	maxFile     = 32 * 1024 * 1024
	readTimeout = 5 * time.Second
)

type CommandResult struct {
	Stdout, Stderr []byte
	ExitCode       int
	Truncated      bool
	InvalidUTF8    bool
}
type Runner func(context.Context, string, []string, []byte) (CommandResult, error)
type Options struct {
	Run         Runner
	Now         func() time.Time
	Environment []string
}
type statusSlot struct {
	done      chan struct{}
	value     protocol.GitStatus
	err       error
	expires   time.Time
	observers int
	cancel    context.CancelFunc
}
type Service struct {
	run      Runner
	now      func() time.Time
	mu       sync.Mutex
	statuses map[string]*statusSlot
	journal  map[string]string
	times    map[string]map[string]int64
	changes  []*changeCapture
}

func New(options Options) *Service {
	s := &Service{run: options.Run, now: options.Now, statuses: map[string]*statusSlot{}, journal: map[string]string{}, times: map[string]map[string]int64{}}
	if s.now == nil {
		s.now = time.Now
	}
	if s.run == nil {
		s.run = nativeRunner(options.Environment)
	}
	return s
}
func invalid(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}
func fault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}
func conflict() error {
	return fault(protocol.ErrorCodeRevisionConflict, "Git branch, HEAD, or index changed; review before retrying")
}
func mapError(err error) error {
	if err == nil {
		return nil
	}
	var value *protocol.Fault
	if errors.As(err, &value) {
		return err
	}
	if errors.Is(err, context.Canceled) {
		return fault(protocol.ErrorCodeCancelled, "Git read cancelled")
	}
	if errors.Is(err, context.DeadlineExceeded) {
		return fault(protocol.ErrorCodeBusy, "Git read deadline exceeded (GitReadTimeout)")
	}
	if errors.Is(err, gogit.ErrRepositoryNotExists) || errors.Is(err, plumbing.ErrObjectNotFound) || errors.Is(err, plumbing.ErrReferenceNotFound) || errors.Is(err, os.ErrNotExist) {
		return fault(protocol.ErrorCodeNotFound, "Git resource does not exist")
	}
	return fault(protocol.ErrorCodeUnavailable, "Git repository operation failed")
}
func open(root string) (*gogit.Repository, error) {
	metadata, err := os.Lstat(filepath.Join(root, ".git"))
	if err != nil {
		return nil, mapError(err)
	}
	if metadata.Mode()&os.ModeSymlink != 0 || !metadata.IsDir() && !metadata.Mode().IsRegular() {
		return nil, invalid("Git metadata cannot be a symlink or special entry")
	}
	r, err := gogit.PlainOpenWithOptions(root, &gogit.PlainOpenOptions{DetectDotGit: false, EnableDotGitCommonDir: true})
	if err != nil {
		return nil, mapError(err)
	}
	worktree, err := r.Worktree()
	if err != nil {
		return nil, invalid("expected a non-bare worktree")
	}
	actual, err := filepath.EvalSymlinks(worktree.Filesystem.Root())
	if err != nil {
		return nil, mapError(err)
	}
	expected, err := filepath.EvalSymlinks(root)
	if err != nil {
		return nil, mapError(err)
	}
	if actual != expected {
		return nil, invalid("expected the exact registered worktree root")
	}
	return r, nil
}
func pathWithin(path, root string) bool {
	rel, e := filepath.Rel(root, path)
	return e == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator))
}
func validPath(path string) error {
	if path == "" || len(path) > 4096 || strings.ContainsRune(path, '\\') || filepath.IsAbs(path) || strings.ContainsFunc(path, unicode.IsControl) || !utf8.ValidString(path) {
		return invalid("invalid repository path")
	}
	for _, p := range strings.Split(path, "/") {
		if p == "" || p == "." || p == ".." || strings.EqualFold(p, ".git") || strings.EqualFold(p, ".sailry") {
			return invalid("invalid repository path")
		}
	}
	return nil
}
func safePath(root, path string) (string, error) {
	if err := validPath(path); err != nil {
		return "", err
	}
	target := filepath.Join(root, filepath.FromSlash(path))
	for p := filepath.Dir(target); ; p = filepath.Dir(p) {
		info, err := os.Lstat(p)
		if errors.Is(err, os.ErrNotExist) {
			if p == root {
				return "", mapError(err)
			}
			continue
		}
		if err != nil {
			return "", mapError(err)
		}
		if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			return "", invalid("repository path crosses an unsupported entry")
		}
		if p == root {
			break
		}
		if filepath.Dir(p) == p || !pathWithin(p, root) {
			return "", invalid("repository path is outside worktree")
		}
	}
	return target, nil
}
func gitDir(root string, r *gogit.Repository) (string, error) {
	storage, ok := r.Storer.(*filesystem.Storage)
	if !ok {
		return "", invalid("repository has no file-backed metadata")
	}
	return storage.Filesystem().Root(), nil
}
func indexRevision(root string, r *gogit.Repository) (string, error) {
	dir, err := gitDir(root, r)
	if err != nil {
		return "", err
	}
	path := filepath.Join(dir, "index")
	info, err := os.Lstat(path)
	if errors.Is(err, os.ErrNotExist) {
		return "absent", nil
	}
	if err != nil {
		return "", mapError(err)
	}
	if !info.Mode().IsRegular() || info.Size() > 16*1024*1024 {
		return "", invalid("expected a bounded regular Git index")
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return "", mapError(err)
	}
	sum := blake3.Sum256(data)
	return fmtHash(sum[:]), nil
}
func fmtHash(value []byte) string {
	const digits = "0123456789abcdef"
	b := make([]byte, len(value)*2)
	for i, v := range value {
		b[i*2] = digits[v>>4]
		b[i*2+1] = digits[v&15]
	}
	return string(b)
}
func bounded(text string, limit int) (string, bool) {
	if len(text) <= limit {
		return text, false
	}
	for limit > 0 && !utf8.RuneStart(text[limit]) {
		limit--
	}
	return text[:limit], true
}
func ptr[T any](v T) *T                   { return &v }
func (s *Service) Invalidate(root string) { s.mu.Lock(); delete(s.statuses, root); s.mu.Unlock() }

// EntryTimes supplies modification timestamps for the existing native activity
// projection without changing the v1 GitEntry declaration.
func (s *Service) EntryTimes(root string) map[string]int64 {
	s.mu.Lock()
	defer s.mu.Unlock()
	result := map[string]int64{}
	for p, v := range s.times[root] {
		result[p] = v
	}
	return result
}
func checkContext(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return mapError(err)
	}
	return nil
}
func validateRevision(value string) error {
	if strings.TrimSpace(value) == "" || len(value) > 1024 || strings.HasPrefix(value, "-") || strings.ContainsRune(value, ':') || strings.ContainsFunc(value, unicode.IsControl) {
		return invalid("invalid Git commit revision")
	}
	return nil
}
func resolve(r *gogit.Repository, value string) (plumbing.Hash, error) {
	if err := validateRevision(value); err != nil {
		return plumbing.ZeroHash, err
	}
	hash, err := r.ResolveRevision(plumbing.Revision(value))
	if err != nil {
		return plumbing.ZeroHash, mapError(err)
	}
	if _, err = r.CommitObject(*hash); err != nil {
		return plumbing.ZeroHash, mapError(err)
	}
	return *hash, nil
}
func (s *Service) Head(ctx context.Context, root string) (protocol.GitHead, error) {
	if err := checkContext(ctx); err != nil {
		return protocol.GitHead{}, err
	}
	r, err := open(root)
	if err != nil {
		if errors.Is(err, gogit.ErrRepositoryNotExists) {
			return protocol.GitHead{Kind: protocol.RepositoryKindDirectory}, nil
		}
		if _, e := os.Lstat(filepath.Join(root, ".git")); errors.Is(e, os.ErrNotExist) {
			return protocol.GitHead{Kind: protocol.RepositoryKindDirectory}, nil
		}
		return protocol.GitHead{}, err
	}
	return head(r)
}
func head(r *gogit.Repository) (protocol.GitHead, error) {
	ref, err := r.Head()
	if errors.Is(err, plumbing.ErrReferenceNotFound) {
		raw, e := r.Reference(plumbing.HEAD, false)
		if e != nil {
			return protocol.GitHead{}, mapError(e)
		}
		var branch *string
		if raw.Type() == plumbing.SymbolicReference && raw.Target().IsBranch() {
			branch = ptr(raw.Target().Short())
		}
		return protocol.GitHead{Kind: protocol.RepositoryKindUnborn, Branch: branch}, nil
	}
	if err != nil {
		return protocol.GitHead{}, mapError(err)
	}
	var branch *string
	if ref.Name().IsBranch() {
		branch = ptr(ref.Name().Short())
	}
	return protocol.GitHead{Kind: protocol.RepositoryKindReady, Branch: branch, Head: ptr(ref.Hash().String())}, nil
}
func (s *Service) Check(ctx context.Context, root, index string, expectedHead, branch *string) error {
	r, err := open(root)
	if err != nil {
		return err
	}
	current, err := head(r)
	if err != nil {
		return err
	}
	revision, err := indexRevision(root, r)
	if err != nil {
		return err
	}
	equal := func(a, b *string) bool { return a == nil && b == nil || a != nil && b != nil && *a == *b }
	if revision != index || !equal(current.Head, expectedHead) || !equal(current.Branch, branch) {
		return conflict()
	}
	return checkContext(ctx)
}
