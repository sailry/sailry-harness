package git

import (
	"bytes"
	"context"
	"errors"
	"path/filepath"
	"strings"
	"unicode"

	"github.com/go-git/go-billy/v5/osfs"
	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/config"
	"github.com/go-git/go-git/v5/plumbing/cache"
	gitconfig "github.com/go-git/go-git/v5/plumbing/format/config"
	"github.com/go-git/go-git/v5/storage/filesystem"
	"github.com/go-git/go-git/v5/storage/filesystem/dotgit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type worktreeReadStorage struct {
	*filesystem.Storage
	settings *config.Config
}

func (s *worktreeReadStorage) Config() (*config.Config, error) { return s.settings, nil }

func readFilterConfig(r *gogit.Repository) (*config.Config, error) {
	if _, ok := r.Storer.(*worktreeReadStorage); ok {
		return r.Config()
	}
	return r.ConfigScoped(config.GlobalScope)
}

func (s *Service) openReadWorktree(ctx context.Context, root string) (*gogit.Repository, error) {
	r, err := plainOpen(root)
	if err != nil && onlyWorktreeConfigUnsupported(err) {
		r, err = s.openWorktreeConfig(ctx, root)
	}
	if err != nil {
		return nil, mapError(err)
	}
	return checkWorktree(root, r)
}

func (s *Service) openDiffWorktree(ctx context.Context, root string) (*gogit.Repository, error) {
	r, err := s.openReadWorktree(ctx, root)
	if err != nil {
		return nil, err
	}
	// Reuse the resolved Git filesystem. PlainOpen cannot set the threshold
	// that lets status and diff see a large loose object's size without inflating it.
	options := filesystem.Options{LargeObjectThreshold: statLimit}
	switch store := r.Storer.(type) {
	case *filesystem.Storage:
		r.Storer = filesystem.NewStorageWithOptions(store.Filesystem(), cache.NewObjectLRUDefault(), options)
	case *worktreeReadStorage:
		store.Storage = filesystem.NewStorageWithOptions(store.Filesystem(), cache.NewObjectLRUDefault(), options)
	}
	return r, nil
}

func onlyWorktreeConfigUnsupported(err error) bool {
	if !errors.Is(err, gogit.ErrUnknownExtension) && !errors.Is(err, gogit.ErrUnsupportedExtensionRepositoryFormatVersion) {
		return false
	}
	_, names, _ := strings.Cut(err.Error(), ": ")
	for name := range strings.SplitSeq(names, ", ") {
		if name != "worktreeconfig" {
			return false
		}
	}
	return true
}

func (s *Service) openWorktreeConfig(ctx context.Context, root string) (*gogit.Repository, error) {
	// Let native Git apply core.bare and core.worktree before opening the admin
	// directory in go-git. Pin the marker to prevent discovery in a parent.
	capture, err := s.command(ctx, root, []string{"--git-dir=.git", "rev-parse", "--path-format=absolute", "--git-dir", "--git-common-dir", "--show-toplevel"}, nil, true)
	if err != nil {
		return nil, err
	}
	paths := strings.Split(strings.TrimSuffix(string(capture.Stdout), "\n"), "\n")
	if capture.Truncated || capture.InvalidUTF8 || len(paths) != 3 {
		return nil, invalid("invalid Git worktree layout")
	}
	for i, path := range paths {
		path = strings.TrimSuffix(path, "\r")
		if !filepath.IsAbs(path) || len(path) > 4096 || strings.ContainsFunc(path, unicode.IsControl) {
			return nil, invalid("invalid Git worktree layout")
		}
		paths[i], err = filepath.EvalSymlinks(path)
		if err != nil {
			return nil, err
		}
	}
	expected, err := filepath.EvalSymlinks(root)
	if err != nil {
		return nil, err
	}
	if paths[2] != expected {
		return nil, invalid("expected the exact registered worktree root")
	}
	settings, err := s.readWorktreeConfig(ctx, root)
	if err != nil {
		return nil, err
	}
	fs := dotgit.NewRepositoryFilesystem(osfs.New(paths[0], osfs.WithBoundOS()), osfs.New(paths[1], osfs.WithBoundOS()))
	storage := &worktreeReadStorage{Storage: filesystem.NewStorage(fs, cache.NewObjectLRUDefault()), settings: settings}
	return gogit.Open(storage, osfs.New(expected, osfs.WithBoundOS()))
}

func (s *Service) readWorktreeConfig(ctx context.Context, root string) (*config.Config, error) {
	// Config values can contain credentials. Keep this capture out of the Git log.
	limited, cancel := context.WithTimeout(ctx, readTimeout)
	defer cancel()
	const keys = `^(core\.repositoryformatversion|extensions\..*|branch\..*|remote\..*|url\..*|filter\..+\.(clean|process|required))$`
	capture, err := s.run(context.WithValue(limited, gitReadKey{}, true), root,
		[]string{"--git-dir=.git", "config", "--includes", "--null", "--show-scope", "--get-regexp", keys}, nil)
	if err != nil {
		return nil, err
	}
	text, terminated := strings.CutSuffix(string(capture.Stdout), "\x00")
	fields := strings.Split(text, "\x00")
	if capture.ExitCode != 0 || capture.Truncated || capture.InvalidUTF8 || !terminated || len(fields)%2 != 0 {
		return nil, fault(protocol.ErrorCodeUnavailable, "could not read Git worktree config")
	}
	raw := gitconfig.New()
	found := false
	for i := 0; i < len(fields); i += 2 {
		scope := fields[i]
		key, value, _ := strings.Cut(fields[i+1], "\n")
		section, option, ok := strings.Cut(key, ".")
		if !ok || option == "" {
			return nil, fault(protocol.ErrorCodeUnavailable, "invalid Git worktree config output")
		}
		switch key {
		case "extensions.worktreeconfig":
			found = found || scope == "local"
			continue
		case "core.repositoryformatversion":
			if scope != "local" {
				continue
			}
		}
		if scope != "local" && scope != "worktree" && section != "filter" {
			continue
		}
		subsection := ""
		if last := strings.LastIndexByte(option, '.'); last >= 0 {
			subsection, option = option[:last], option[last+1:]
		}
		raw.AddOption(section, subsection, option, value)
	}
	if !found {
		return nil, fault(protocol.ErrorCodeConflict, "Git config changed during inspection")
	}
	var data bytes.Buffer
	if err := gitconfig.NewEncoder(&data).Encode(raw); err != nil {
		return nil, err
	}
	settings, err := config.ReadConfig(&data)
	if err != nil {
		return nil, fault(protocol.ErrorCodeUnavailable, "invalid Git worktree config")
	}
	return settings, nil
}
