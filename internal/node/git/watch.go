package git

import (
	"bufio"
	"context"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"sort"
	"strings"

	"github.com/go-git/go-billy/v5/osfs"
	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing/format/gitignore"
)

type WatchFilter struct {
	root       string
	service    *Service
	repository *gogit.Repository
	metadata   []string
	tracked    map[string]bool
	names      []string // tracked paths in byte order, for directory lookups
	matcher    *watchMatcher
	reopen     bool
	indexDirty bool
}

// A filter may outlive thousands of batches. Keep base rules and at most one
// ancestor chain, never an inventory of ignore files elsewhere in the tree.
type watchMatcher struct {
	root, common string
	base         []gitignore.Pattern
	baseLoaded   bool
	parent       string
	cached       gitignore.Matcher
}

func appendIgnore(anchor *os.Root, name string, domain []string, patterns []gitignore.Pattern) ([]gitignore.Pattern, error) {
	info, err := anchor.Lstat(name)
	if errors.Is(err, os.ErrNotExist) || err == nil && !info.Mode().IsRegular() {
		return patterns, nil
	}
	if err != nil {
		return nil, err
	}
	file, err := anchor.Open(name)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		line := scanner.Text()
		if !strings.HasPrefix(line, "#") && strings.TrimSpace(line) != "" {
			patterns = append(patterns, gitignore.ParsePattern(line, domain))
		}
	}
	return patterns, scanner.Err()
}

func (m *watchMatcher) match(parts []string, isDir bool) (bool, error) {
	parent := filepath.Join(m.root, filepath.Join(parts[:len(parts)-1]...))
	if m.cached != nil && m.parent == parent {
		return m.cached.Match(parts, isDir), nil
	}
	root, err := os.OpenRoot(m.root)
	if err != nil {
		return false, err
	}
	defer root.Close()
	if !m.baseLoaded {
		fs := osfs.New(string(filepath.Separator))
		system, _ := gitignore.LoadSystemPatterns(fs)
		global, _ := gitignore.LoadGlobalPatterns(fs)
		common, err := os.OpenRoot(m.common)
		if err != nil {
			return false, err
		}
		patterns, err := appendIgnore(common, filepath.Join("info", "exclude"), nil, append(system, global...))
		common.Close()
		if err != nil {
			return false, err
		}
		m.base, err = appendIgnore(root, ".gitignore", nil, patterns)
		if err != nil {
			return false, err
		}
		m.baseLoaded = true
	}
	patterns := m.base[:len(m.base):len(m.base)]
	for depth := 1; depth < len(parts); depth++ {
		domain := parts[:depth]
		if gitignore.NewMatcher(patterns).Match(domain, true) {
			// Git does not read rules inside an excluded directory.
			return true, nil
		}
		dir := filepath.Join(domain...)
		info, err := root.Lstat(dir)
		if errors.Is(err, os.ErrNotExist) || err == nil && !info.IsDir() {
			break
		}
		if err != nil {
			return false, err
		}
		patterns, err = appendIgnore(root, filepath.Join(dir, ".gitignore"), domain, patterns)
		if err != nil {
			return false, err
		}
	}
	m.parent, m.cached = parent, gitignore.NewMatcher(patterns)
	return m.cached.Match(parts, isDir), nil
}

// Inventory and staging must not admit an untracked file on unreadable rules.
func (m *watchMatcher) Match(parts []string, isDir bool) bool {
	ignored, err := m.match(parts, isDir)
	return err != nil || ignored
}

func watchRoots(root string, r *gogit.Repository) ([]string, error) {
	dir, err := gitDir(root, r)
	if err != nil {
		return nil, err
	}
	dir, err = filepath.EvalSymlinks(dir)
	if err != nil {
		return nil, mapError(err)
	}
	result := []string{dir}
	common, err := commonDir(r)
	if err != nil {
		return nil, err
	}
	if common, err = filepath.EvalSymlinks(common); err != nil {
		return nil, mapError(err)
	}
	if common != dir {
		result = append(result, common)
	}
	return result, nil
}
func NewWatchFilter(root string) (*WatchFilter, error) {
	f := &WatchFilter{root: root}
	return f, f.refresh(context.Background())
}
func (s *Service) NewWatchFilter(ctx context.Context, root string) (*WatchFilter, []string, error) {
	f := &WatchFilter{root: root, service: s}
	if err := f.refresh(ctx); err != nil {
		return nil, nil, err
	}
	return f, slices.Clone(f.metadata), nil
}
func (f *WatchFilter) refresh(ctx context.Context) error {
	var r *gogit.Repository
	var err error
	if f.service == nil {
		r, err = open(f.root)
	} else {
		ctx, cancel := context.WithTimeout(ctx, readTimeout)
		defer cancel()
		r, err = f.service.openReadWorktree(ctx, f.root)
	}
	if err != nil {
		if _, e := os.Lstat(filepath.Join(f.root, ".git")); !os.IsNotExist(e) {
			return err
		}
	}
	var metadata []string
	var matcher *watchMatcher
	if r != nil {
		metadata, err = watchRoots(f.root, r)
		if err != nil {
			return err
		}
		matcher = &watchMatcher{root: f.root, common: metadata[len(metadata)-1]}
	}
	f.repository, f.metadata, f.matcher = r, metadata, matcher
	f.tracked, f.names = nil, nil
	f.reopen, f.indexDirty = false, r != nil
	return nil
}

func (f *WatchFilter) loadIndex() error {
	idx, err := f.repository.Storer.Index()
	if err != nil {
		return mapError(err)
	}
	f.tracked = make(map[string]bool, len(idx.Entries))
	f.names = make([]string, 0, len(idx.Entries))
	for _, entry := range idx.Entries {
		f.tracked[entry.Name] = true
		f.names = append(f.names, entry.Name)
	}
	sort.Strings(f.names)
	f.indexDirty = false
	return nil
}
func (f *WatchFilter) ReferencesChanged(paths []string) bool {
	if len(paths) == 0 {
		return true
	}
	for _, path := range paths {
		switch f.metadataPath(path) {
		case "", "index", "info", "info/exclude":
		default:
			return true
		}
	}
	return false
}
func (f *WatchFilter) Changed(paths []string) bool {
	if len(paths) == 0 {
		f.reopen = true
		return true
	}
	// Process every invalidation before returning, including an index write
	// or a changed ignore file delivered in the same batch as a ref update.
	relevant := false
	for _, path := range paths {
		switch f.metadataPath(path) {
		case "":
			if !pathWithin(path, f.root) || pathWithin(path, filepath.Join(f.root, ".sailry", "worktrees")) {
				continue
			}
			ancestor := path
			if filepath.Base(path) == ".gitignore" {
				ancestor = filepath.Dir(path)
				if f.matcher != nil && ancestor == f.root {
					f.matcher.baseLoaded = false
				}
				relevant = true
			}
			// A directory rename may replace rules without naming .gitignore.
			if f.matcher != nil && f.matcher.cached != nil && pathWithin(f.matcher.parent, ancestor) {
				f.matcher.cached = nil
			}
		case ".", "commondir":
			f.reopen, relevant = true, true
		case "index":
			f.indexDirty, relevant = true, true
		case "config", "config.worktree", "info", "info/exclude":
			if f.matcher != nil {
				f.matcher.baseLoaded, f.matcher.cached = false, nil
			}
			relevant = true
		default:
			relevant = true
		}
	}
	if relevant {
		return true
	}
	for _, path := range paths {
		if !pathWithin(path, f.root) {
			continue
		}
		relative, _ := filepath.Rel(f.root, path)
		relative = filepath.ToSlash(relative)
		if relative == "." {
			return true
		}
		if relative == ".sailry/worktrees" || strings.HasPrefix(relative, ".sailry/worktrees/") {
			continue
		}
		parts := strings.Split(relative, "/")
		isMetadata := false
		for _, p := range parts {
			if p == ".git" {
				isMetadata = true
			}
		}
		if isMetadata {
			continue
		}
		if f.reopen {
			if err := f.refresh(context.Background()); err != nil {
				return true
			}
		}
		if f.repository == nil {
			continue
		}
		ignored, err := f.matcher.match(parts, false)
		if err != nil || !ignored {
			return true
		}
		// Only ignored paths need an index lookup: tracked files and their
		// ancestors still change repository status regardless of ignore rules.
		if f.indexDirty {
			if err := f.loadIndex(); err != nil {
				return true
			}
		}
		if f.tracked[relative] {
			return true
		}
		// Paths below a directory are contiguous from the first name at or after "directory/".
		if i := sort.SearchStrings(f.names, relative+"/"); i < len(f.names) && strings.HasPrefix(f.names[i], relative+"/") {
			return true
		}
	}
	return false
}
func (f *WatchFilter) metadataPath(path string) string {
	if path == f.root || path == filepath.Join(f.root, ".git") {
		return "."
	}
	roots := f.metadata
	if f.reopen && len(roots) == 0 {
		// A formerly plain directory may now be a repository. Its first
		// notifications can arrive before a content event reopens it.
		roots = []string{filepath.Join(f.root, ".git")}
	}
	for _, root := range roots {
		if !pathWithin(path, root) {
			continue
		}
		relative, _ := filepath.Rel(root, path)
		relative = filepath.ToSlash(relative)
		if strings.HasSuffix(relative, ".lock") {
			continue
		}
		if relative == "." || relative == "refs" || strings.HasPrefix(relative, "refs/") {
			return relative
		}
		switch relative {
		case "HEAD", "index", "packed-refs", "config", "config.worktree", "commondir", "shallow", "info", "info/exclude":
			return relative
		}
	}
	return ""
}
