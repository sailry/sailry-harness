package git

import (
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/go-git/go-billy/v5/osfs"
	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing/format/gitignore"
)

type WatchFilter struct {
	root       string
	repository *gogit.Repository
	metadata   []string
	tracked    map[string]bool
	names      []string // tracked paths in byte order, for directory lookups
	matcher    gitignore.Matcher
}

func WatchRoots(root string) ([]string, error) {
	r, err := open(root)
	if err != nil {
		if _, e := os.Lstat(filepath.Join(root, ".git")); os.IsNotExist(e) {
			return []string{}, nil
		}
		return nil, err
	}
	dir, err := gitDir(root, r)
	if err != nil {
		return nil, err
	}
	dir, err = filepath.EvalSymlinks(dir)
	if err != nil {
		return nil, mapError(err)
	}
	result := []string{dir}
	if data, e := os.ReadFile(filepath.Join(dir, "commondir")); e == nil {
		common, err := filepath.EvalSymlinks(filepath.Join(dir, strings.TrimSpace(string(data))))
		if err != nil {
			return nil, mapError(err)
		}
		if common != dir {
			result = append(result, common)
		}
	}
	return result, nil
}
func NewWatchFilter(root string) (*WatchFilter, error) {
	f := &WatchFilter{root: root}
	return f, f.refresh()
}
func (f *WatchFilter) refresh() error {
	metadata, err := WatchRoots(f.root)
	if err != nil {
		return err
	}
	f.metadata = metadata
	f.tracked = map[string]bool{}
	f.names = nil
	f.repository = nil
	f.matcher = nil
	if len(metadata) == 0 {
		return nil
	}
	r, err := open(f.root)
	if err != nil {
		return err
	}
	f.repository = r
	idx, err := r.Storer.Index()
	if err != nil {
		return mapError(err)
	}
	for _, entry := range idx.Entries {
		f.tracked[entry.Name] = true
		f.names = append(f.names, entry.Name)
	}
	sort.Strings(f.names)
	worktree, err := r.Worktree()
	if err != nil {
		return mapError(err)
	}
	patterns, err := gitignore.ReadPatterns(worktree.Filesystem, nil)
	if err != nil {
		return mapError(err)
	}
	system, _ := gitignore.LoadSystemPatterns(osfs.New(string(filepath.Separator)))
	global, _ := gitignore.LoadGlobalPatterns(osfs.New(string(filepath.Separator)))
	patterns = append(append(system, global...), patterns...)
	f.matcher = gitignore.NewMatcher(patterns)
	return nil
}
func (f *WatchFilter) ReferencesChanged(paths []string) bool {
	if len(paths) == 0 {
		return true
	}
	for _, path := range paths {
		if path == f.root || path == filepath.Join(f.root, ".git") {
			return true
		}
		for _, root := range f.metadata {
			if !pathWithin(path, root) {
				continue
			}
			relative, _ := filepath.Rel(root, path)
			relative = filepath.ToSlash(relative)
			if strings.HasSuffix(relative, ".lock") {
				continue
			}
			if relative == "." || relative == "refs" || strings.HasPrefix(relative, "refs/") {
				return true
			}
			switch relative {
			case "HEAD", "packed-refs", "config", "config.worktree", "commondir", "shallow":
				return true
			}
		}
	}
	return false
}
func (f *WatchFilter) Changed(paths []string) bool {
	if len(paths) == 0 {
		return true
	}
	for _, path := range paths {
		if f.metadataChange(path) || pathWithin(path, f.root) && filepath.Base(path) == ".gitignore" {
			_ = f.refresh()
			return true
		}
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
		if f.tracked[relative] {
			return true
		}
		// Paths below a directory are contiguous from the first name at or after "directory/".
		if i := sort.SearchStrings(f.names, relative+"/"); i < len(f.names) && strings.HasPrefix(f.names[i], relative+"/") {
			return true
		}
		if f.repository != nil && (f.matcher == nil || !f.matcher.Match(parts, false)) {
			return true
		}
	}
	return false
}
func (f *WatchFilter) metadataChange(path string) bool {
	if path == filepath.Join(f.root, ".git") {
		return true
	}
	for _, root := range f.metadata {
		if !pathWithin(path, root) {
			continue
		}
		relative, _ := filepath.Rel(root, path)
		relative = filepath.ToSlash(relative)
		if strings.HasSuffix(relative, ".lock") {
			continue
		}
		if relative == "." || relative == "refs" || strings.HasPrefix(relative, "refs/") {
			return true
		}
		switch relative {
		case "HEAD", "index", "packed-refs", "config", "config.worktree", "commondir", "shallow", "info/exclude":
			return true
		}
	}
	return false
}
