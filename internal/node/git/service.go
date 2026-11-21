// Package git owns bounded repository reads and native Git mutations on Node.
// Behavioral adaptations retain sailry-code 67ae9fa0, Apache-2.0 provenance.
package git

import (
	"context"
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/go-git/go-billy/v5"
	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const (
	maxEntries  = 500
	maxDiff     = 48 * 1024
	maxFile     = 32 * 1024 * 1024
	readTimeout = 5 * time.Second
	// statLimit is the largest side whose changed lines a status counts.
	statLimit = 1024 * 1024
	// statBudget is the share of the status deadline spent counting lines.
	statBudget = time.Second
	// mutationTimeout is the total time of a local native mutation.
	mutationTimeout = 120 * time.Second
	// networkIdle is how long a transfer with a remote may stay silent.
	networkIdle = 120 * time.Second
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
	Profile     string
	// Lock orders run_git_action and create_git_commit with the owner's other
	// effects on a root. The service takes it around their local work and
	// leaves transfers with a remote outside it.
	Lock func(root string) (unlock func())
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
	fetches  map[string]*fetchSlot
	blames   map[string]chan struct{}
	changes  []*changeCapture
	idle     time.Duration // silence limit of a network operation
	stats    time.Duration // line statistics share of a status
	lock     func(root string) func()
}

func New(options Options) *Service {
	s := &Service{run: options.Run, now: options.Now, statuses: map[string]*statusSlot{}, journal: map[string]string{}, fetches: map[string]*fetchSlot{}, blames: map[string]chan struct{}{}, idle: networkIdle, stats: statBudget}
	if s.now == nil {
		s.now = time.Now
	}
	if s.run == nil {
		s.run = nativeRunner(options.Environment, options.Profile)
	}
	if s.lock = options.Lock; s.lock == nil {
		s.lock = func(string) func() { return func() {} }
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
	// The cause stays in the message the user and model read; error
	// reporting sends only the code.
	return fault(protocol.ErrorCodeUnavailable, "Git repository operation failed: "+err.Error())
}
func checkGitMetadata(root string) error {
	metadata, err := os.Lstat(filepath.Join(root, ".git"))
	if err != nil {
		return err
	}
	if metadata.Mode()&os.ModeSymlink != 0 || !metadata.IsDir() && !metadata.Mode().IsRegular() {
		return invalid("Git metadata cannot be a symlink or special entry")
	}
	return nil
}
func open(root string) (*gogit.Repository, error) {
	r, err := plainOpen(root)
	if err != nil {
		return nil, mapError(err)
	}
	return checkWorktree(root, r)
}
func plainOpen(root string) (*gogit.Repository, error) {
	if err := checkGitMetadata(root); err != nil {
		return nil, err
	}
	return gogit.PlainOpenWithOptions(root, &gogit.PlainOpenOptions{DetectDotGit: false, EnableDotGitCommonDir: true})
}
func checkWorktree(root string, r *gogit.Repository) (*gogit.Repository, error) {
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
	storage, ok := r.Storer.(interface{ Filesystem() billy.Filesystem })
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
	return hex.EncodeToString(sum[:]), nil
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
	r, err := s.openReadWorktree(ctx, root)
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
