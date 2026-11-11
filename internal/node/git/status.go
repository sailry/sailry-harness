package git

import (
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"sort"
	"strings"
	"time"
	"unicode/utf8"

	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/config"
	idxfmt "github.com/go-git/go-git/v5/plumbing/format/index"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func StatusArgs(drivers []string) []string {
	args := []string{"--no-optional-locks", "-c", "core.fsmonitor=false"}
	drivers = append([]string(nil), drivers...)
	sort.Strings(drivers)
	for _, driver := range drivers {
		for _, setting := range []string{"clean=", "process=", "required=false"} {
			args = append(args, "-c", "filter."+driver+"."+setting)
		}
	}
	return append(args, "status", "--porcelain=v2", "-z", "--branch", "--untracked-files=all", "--no-renames")
}
func filterDrivers(r *gogit.Repository) ([]string, error) {
	config, err := r.ConfigScoped(config.GlobalScope)
	if err != nil {
		return nil, mapError(err)
	}
	seen := map[string]bool{}
	if config.Raw != nil {
		for _, section := range config.Raw.Sections {
			if section.Name == "filter" {
				for _, sub := range section.Subsections {
					for _, option := range sub.Options {
						if option.Key == "clean" || option.Key == "process" || option.Key == "required" {
							seen[sub.Name] = true
						}
					}
				}
			}
		}
	}
	result := []string{}
	for name := range seen {
		result = append(result, name)
	}
	sort.Strings(result)
	return result, nil
}
func (s *Service) Status(ctx context.Context, root string) (protocol.GitStatus, error) {
	s.mu.Lock()
	slot := s.statuses[root]
	if slot == nil || !slot.expires.IsZero() && !s.now().Before(slot.expires) {
		owned, cancel := context.WithCancel(context.Background())
		slot = &statusSlot{done: make(chan struct{}), cancel: cancel}
		s.statuses[root] = slot
		go func(slot *statusSlot) {
			value, err := s.status(owned, root)
			s.mu.Lock()
			slot.value, slot.err = value, err
			slot.expires = s.now().Add(time.Second)
			if err != nil && s.statuses[root] == slot {
				delete(s.statuses, root)
			}
			close(slot.done)
			slot.cancel()
			s.mu.Unlock()
		}(slot)
	}
	slot.observers++
	s.mu.Unlock()
	defer func() {
		s.mu.Lock()
		slot.observers--
		if slot.observers == 0 && slot.expires.IsZero() {
			slot.cancel()
		}
		s.mu.Unlock()
	}()
	select {
	case <-ctx.Done():
		return protocol.GitStatus{}, mapError(ctx.Err())
	case <-slot.done:
		result := slot.value
		result.Entries = append([]protocol.GitEntry(nil), result.Entries...)
		result.Repositories = append([]string(nil), result.Repositories...)
		return result, slot.err
	}
}
func (s *Service) status(ctx context.Context, root string) (protocol.GitStatus, error) {
	r, err := open(root)
	if err != nil {
		if _, e := os.Lstat(filepath.Join(root, ".git")); errors.Is(e, os.ErrNotExist) {
			return protocol.GitStatus{Kind: protocol.RepositoryKindDirectory, Entries: []protocol.GitEntry{}, Repositories: childRepositories(ctx, root)}, nil
		}
		return protocol.GitStatus{}, err
	}
	beforeHead, err := head(r)
	if err != nil {
		return protocol.GitStatus{}, err
	}
	revision, err := indexRevision(root, r)
	if err != nil {
		return protocol.GitStatus{}, err
	}
	drivers, err := filterDrivers(r)
	if err != nil {
		return protocol.GitStatus{}, err
	}
	limited, cancel := context.WithTimeout(ctx, readTimeout)
	defer cancel()
	capture, err := s.command(limited, root, StatusArgs(drivers), nil, true)
	if err != nil {
		return protocol.GitStatus{}, err
	}
	result, err := ParseStatus(capture.Stdout, capture.Truncated)
	if err != nil {
		return protocol.GitStatus{}, err
	}
	next, err := indexRevision(root, r)
	if err != nil {
		return protocol.GitStatus{}, err
	}
	if next != revision {
		return protocol.GitStatus{}, fault(protocol.ErrorCodeConflict, "Git metadata changed during status inspection")
	}
	afterHead, err := head(r)
	if err != nil {
		return protocol.GitStatus{}, err
	}
	if !sameString(beforeHead.Head, afterHead.Head) || !sameString(beforeHead.Branch, afterHead.Branch) {
		return protocol.GitStatus{}, fault(protocol.ErrorCodeConflict, "Git metadata changed during status inspection")
	}
	result.IndexRevision = ptr(revision)
	// Line statistics read the HEAD tree and the index once for the whole status.
	var tree *object.Tree
	var staged map[string]*idxfmt.Entry
	var treeErr, indexErr error
	if slices.ContainsFunc(result.Entries, func(entry protocol.GitEntry) bool { return !entry.Conflicted }) {
		tree, treeErr = headTree(r)
		var idx *idxfmt.Index
		if idx, indexErr = r.Storer.Index(); indexErr == nil {
			staged = make(map[string]*idxfmt.Entry, len(idx.Entries))
			for _, item := range idx.Entries {
				if staged[item.Name] == nil {
					staged[item.Name] = item
				}
			}
		}
	}
	times := map[string]int64{}
	filtered := result.Entries[:0]
	for _, entry := range result.Entries {
		if err := checkContext(limited); err != nil {
			return protocol.GitStatus{}, err
		}
		path, e := safePath(root, entry.Path)
		if e != nil {
			result.OmittedPaths++
			continue
		}
		if info, e := os.Lstat(path); e == nil {
			times[entry.Path] = info.ModTime().UnixMilli()
			if info.ModTime().UnixMilli() >= 0 {
				entry.ModifiedMs = ptr(uint64(info.ModTime().UnixMilli()))
			}
		}
		if !entry.Conflicted {
			committed, committedErr := content{}, treeErr
			if committedErr == nil {
				committed, committedErr = treeContent(r, tree, entry.Path, 1024*1024)
			}
			indexed, indexedErr := content{}, indexErr
			if item := staged[entry.Path]; indexedErr == nil && item != nil {
				indexed, indexedErr = blobContent(r, item.Hash, item.Mode, 1024*1024)
			}
			working, workingErr := workingContent(root, entry.Path, 1024*1024)
			entry.StagedDiff = lineStats(entry.Path, protocol.GitDiffScopeStaged, committed, indexed, committedErr, indexedErr)
			entry.UnstagedDiff = lineStats(entry.Path, protocol.GitDiffScopeUnstaged, indexed, working, indexedErr, workingErr)
			entry.Diff = lineStats(entry.Path, protocol.GitDiffScopeAll, committed, working, committedErr, workingErr)
		}
		filtered = append(filtered, entry)
	}
	result.Entries = filtered
	s.mu.Lock()
	s.times[root] = times
	s.mu.Unlock()
	return result, nil
}

// lineStats counts one scope of a status entry from sides that were already
// read. A side that could not be read leaves the scope uncounted.
func lineStats(path string, scope protocol.GitDiffScope, before, after content, beforeErr, afterErr error) protocol.GitLineStats {
	if beforeErr != nil || afterErr != nil {
		return protocol.GitLineStats{Truncated: true}
	}
	diff, err := renderDiff(path, scope, before, after)
	if err != nil {
		return protocol.GitLineStats{Truncated: true}
	}
	return protocol.GitLineStats{Additions: diff.Additions, Deletions: diff.Deletions, Truncated: diff.Truncated}
}

// ParseStatus reads source-native porcelain v2, including its NUL-framed paths.
func ParseStatus(data []byte, truncated bool) (protocol.GitStatus, error) {
	result := protocol.GitStatus{Kind: protocol.RepositoryKindUnborn, Entries: []protocol.GitEntry{}, Truncated: truncated, Repositories: []string{}}
	if !utf8.Valid(data) {
		return result, fault(protocol.ErrorCodeUnavailable, "Git status contains unsupported path encoding")
	}
	if truncated {
		result.OmittedPaths++
		if end := bytes.LastIndexByte(data, 0); end >= 0 {
			data = data[:end+1]
		} else {
			data = nil
		}
	} else if len(data) == 0 || data[len(data)-1] != 0 {
		return result, fault(protocol.ErrorCodeUnavailable, "incomplete Git status output")
	}
	headers := 0
	for _, record := range strings.Split(strings.TrimSuffix(string(data), "\x00"), "\x00") {
		if h, ok := strings.CutPrefix(record, "# branch.oid "); ok {
			headers++
			if h != "(initial)" {
				result.Head = ptr(h)
				result.Kind = protocol.RepositoryKindReady
			}
			continue
		}
		if b, ok := strings.CutPrefix(record, "# branch.head "); ok {
			headers++
			if b != "(detached)" {
				result.Branch = ptr(b)
			}
			continue
		}
		if strings.HasPrefix(record, "# ") {
			continue
		}
		entry := protocol.GitEntry{Diff: protocol.GitLineStats{Truncated: true}}
		if path, ok := strings.CutPrefix(record, "? "); ok {
			entry.Path = path
			entry.Untracked = true
			entry.UnstagedDiff.Truncated = true
		} else {
			count := 9
			if strings.HasPrefix(record, "u ") {
				count = 11
				entry.Conflicted = true
			} else if !strings.HasPrefix(record, "1 ") {
				return result, fault(protocol.ErrorCodeUnavailable, "invalid Git status output")
			}
			fields := strings.SplitN(record, " ", count)
			if len(fields) != count || len(fields[1]) != 2 {
				return result, fault(protocol.ErrorCodeUnavailable, "incomplete Git status record")
			}
			entry.Path = fields[count-1]
			if !entry.Conflicted {
				var e error
				entry.Staged, e = statusKind(fields[1][0])
				if e != nil {
					return result, e
				}
				entry.Unstaged, e = statusKind(fields[1][1])
				if fields[1][1] == 'A' {
					entry.Staged = ptr(protocol.GitChangeKindAdded)
					entry.Unstaged = ptr(protocol.GitChangeKindModified)
				}
				if e != nil {
					return result, e
				}
			}
			entry.StagedDiff.Truncated = entry.Staged != nil || entry.Conflicted
			entry.UnstagedDiff.Truncated = entry.Unstaged != nil || entry.Conflicted
		}
		if validPath(entry.Path) != nil {
			result.OmittedPaths++
			continue
		}
		if len(result.Entries) == maxEntries {
			result.Truncated = true
			result.OmittedPaths++
			continue
		}
		result.Entries = append(result.Entries, entry)
	}
	if headers != 2 {
		return result, fault(protocol.ErrorCodeUnavailable, "incomplete Git status headers")
	}
	sort.Slice(result.Entries, func(i, j int) bool { return result.Entries[i].Path < result.Entries[j].Path })
	return result, nil
}
func statusKind(code byte) (*protocol.GitChangeKind, error) {
	switch code {
	case '.':
		return nil, nil
	case 'M':
		return ptr(protocol.GitChangeKindModified), nil
	case 'T':
		return ptr(protocol.GitChangeKindTypeChanged), nil
	case 'D':
		return ptr(protocol.GitChangeKindDeleted), nil
	case 'A':
		return ptr(protocol.GitChangeKindAdded), nil
	}
	return nil, fault(protocol.ErrorCodeUnavailable, "unsupported Git status classification")
}

func childRepositories(ctx context.Context, root string) []string {
	found := []string{}
	anchor, err := os.OpenRoot(root)
	if err != nil {
		return found
	}
	defer anchor.Close()
	directory, err := anchor.Open(".")
	if err != nil {
		return found
	}
	defer directory.Close()
	children, _ := directory.ReadDir(2000)
	for _, entry := range children {
		if checkContext(ctx) != nil {
			break
		}
		name := entry.Name()
		if strings.HasPrefix(name, ".") || !entry.IsDir() || validPath(name) != nil {
			continue
		}
		child, err := anchor.OpenRoot(name)
		if err != nil {
			continue
		}
		metadata, err := child.Lstat(".git")
		child.Close()
		if err == nil && (metadata.IsDir() || metadata.Mode().IsRegular()) {
			found = append(found, name)
		}
	}
	sort.Strings(found)
	return found[:min(len(found), 50)]
}
