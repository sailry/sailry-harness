//go:build !sailry_no_code_index

package codeindex

import (
	"cmp"
	"errors"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	cacheMaxAge        = 14 * 24 * time.Hour
	cacheMaxBytes      = 512 << 20
	cachePruneInterval = time.Hour
	discardedSuffix    = ".discarded"
)

type cacheRecord struct {
	id   protocol.WorktreeID
	used time.Time
	size int64
}

// touchCache records use even when a query did not write any database pages.
func touchCache(directory string) time.Time {
	now := time.Now()
	parent, err := os.OpenRoot(filepath.Dir(directory))
	if err != nil {
		return now
	}
	defer parent.Close()
	dir, err := openCacheDirectory(parent, filepath.Base(directory))
	if err != nil {
		return now
	}
	defer dir.Close()
	_ = dir.Chtimes(".", now, now)
	return now
}

func openCacheDirectory(root *os.Root, name string) (*os.Root, error) {
	info, err := root.Lstat(name)
	if err != nil {
		return nil, err
	}
	if !info.IsDir() {
		return nil, &fs.PathError{Op: "open code index cache", Path: name, Err: fs.ErrInvalid}
	}
	dir, err := root.OpenRoot(name)
	if err != nil {
		return nil, err
	}
	opened, err := dir.Stat(".")
	if err != nil {
		_ = dir.Close()
		return nil, err
	}
	if !os.SameFile(info, opened) {
		_ = dir.Close()
		return nil, &fs.PathError{Op: "open code index cache", Path: name, Err: fs.ErrInvalid}
	}
	return dir, nil
}

func (s *Service) cacheRoot() (*os.Root, error) {
	profile, err := os.OpenRoot(s.profile)
	if err != nil {
		return nil, err
	}
	defer profile.Close()
	cache, err := openCacheDirectory(profile, "cache")
	if err != nil {
		return nil, err
	}
	defer cache.Close()
	return openCacheDirectory(cache, "code-index")
}

// cacheFiles accepts only regular members of the derived database family.
// An unexpected item is enough to leave the directory alone.
func cacheFiles(dir *os.Root) ([]string, time.Time, int64, error) {
	info, err := dir.Stat(".")
	if err != nil {
		return nil, time.Time{}, 0, err
	}
	file, err := dir.Open(".")
	if err != nil {
		return nil, time.Time{}, 0, err
	}
	defer file.Close()
	known := core.DatabaseFiles("index.turso")
	entries, err := file.ReadDir(len(known) + 1)
	if err != nil && !errors.Is(err, io.EOF) {
		return nil, time.Time{}, 0, err
	}
	used := info.ModTime()
	var names []string
	var size int64
	for _, entry := range entries {
		if !slices.Contains(known, entry.Name()) {
			return nil, time.Time{}, 0, fs.ErrInvalid
		}
		member, err := dir.Lstat(entry.Name())
		if err != nil {
			return nil, time.Time{}, 0, err
		}
		if !member.Mode().IsRegular() {
			return nil, time.Time{}, 0, fs.ErrInvalid
		}
		names = append(names, entry.Name())
		if member.ModTime().After(used) {
			used = member.ModTime()
		}
		size += member.Size()
	}
	return names, used, size, nil
}

func cacheStats(cache *os.Root, name string) (time.Time, int64, error) {
	dir, err := openCacheDirectory(cache, name)
	if err != nil {
		return time.Time{}, 0, err
	}
	defer dir.Close()
	_, used, size, err := cacheFiles(dir)
	return used, size, err
}

func removeDiscarded(cache *os.Root, name string) error {
	dir, err := openCacheDirectory(cache, name)
	if errors.Is(err, fs.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	names, _, _, err := cacheFiles(dir)
	if err == nil {
		for _, member := range names {
			if err = dir.Remove(member); err != nil {
				break
			}
		}
	}
	if closeErr := dir.Close(); err == nil {
		err = closeErr
	}
	if err != nil {
		return err
	}
	return cache.Remove(name)
}

// discardCache detaches the entire database atomically. A partial cleanup
// cannot leave a later query with only some of its database files.
func discardCache(cache *os.Root, name string) error {
	discarded := name + discardedSuffix
	if err := removeDiscarded(cache, discarded); err != nil {
		return err
	}
	if _, _, err := cacheStats(cache, name); err != nil {
		if errors.Is(err, fs.ErrNotExist) {
			return nil
		}
		return err
	}
	if err := cache.Rename(name, discarded); err != nil {
		return err
	}
	return removeDiscarded(cache, discarded)
}

func (s *Service) removeCache(id protocol.WorktreeID) error {
	cache, err := s.cacheRoot()
	if errors.Is(err, fs.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	defer cache.Close()
	return discardCache(cache, id.String())
}

// prune reclaims cold caches first. Admission only locks while checking and
// deleting one candidate, so inventory can run alongside queries.
func (s *Service) prune(now time.Time, maxAge time.Duration, maxBytes int64) {
	s.maintenance.Lock()
	defer s.maintenance.Unlock()
	if s.closed.Err() != nil {
		return
	}
	cache, err := s.cacheRoot()
	if err != nil {
		return
	}
	defer cache.Close()
	entries, err := fs.ReadDir(cache.FS(), ".")
	if err != nil {
		return
	}
	var candidates []cacheRecord
	var total int64
	for _, entry := range entries {
		if s.closed.Err() != nil {
			return
		}
		if !entry.IsDir() {
			continue
		}
		name, discarded := strings.CutSuffix(entry.Name(), discardedSuffix)
		id, err := protocol.ParseWorktreeID(name)
		if err != nil || id.String() != name {
			continue
		}
		if discarded {
			_ = removeDiscarded(cache, entry.Name())
			continue
		}
		used, size, err := cacheStats(cache, name)
		if err == nil {
			candidates = append(candidates, cacheRecord{id: id, used: used, size: size})
			total += size
		}
	}
	slices.SortFunc(candidates, func(a, b cacheRecord) int {
		return cmp.Or(a.used.Compare(b.used), cmp.Compare(a.id.String(), b.id.String()))
	})
	for _, entry := range candidates {
		if s.closed.Err() != nil || now.Sub(entry.used) < maxAge && total <= maxBytes {
			return
		}
		s.mutex.Lock()
		pinned := s.cacheInUseLocked(entry.id)
		if !pinned && discardCache(cache, entry.id.String()) == nil {
			total -= entry.size
		}
		s.mutex.Unlock()
	}
}

func (s *Service) cacheInUseLocked(id protocol.WorktreeID) bool {
	w := s.worktrees[id]
	if w == nil {
		return false
	}
	if !w.access.TryLock() {
		return true
	}
	defer w.access.Unlock()
	return w.index != nil
}
