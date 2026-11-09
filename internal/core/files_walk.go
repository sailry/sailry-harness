// Portions adapted from sailry-code 67ae9fa0 (Apache-2.0).

package core

import (
	"context"
	"errors"
	"io/fs"
	"math"
	"os"
	"slices"
	"sort"
	"strings"
	"time"

	"github.com/bmatcuk/doublestar/v4"
	"github.com/go-git/go-git/v5/plumbing/format/gitignore"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type FileWalkReport struct {
	Visited     uint64
	Skipped     uint64
	Truncated   bool
	Partial     bool
	Diagnostics []protocol.SearchDiagnostic
}

func (report *FileWalkReport) notice(relative, message string) {
	if len(report.Diagnostics) < 100 {
		report.Diagnostics = append(report.Diagnostics, protocol.SearchDiagnostic{Path: relative, Message: message})
	}
}

func (report *FileWalkReport) diagnostic(relative, message string) {
	report.Partial, report.Truncated = true, true
	report.Skipped++
	report.notice(relative, message)
}

type FoundFile struct {
	Path     string
	Modified time.Time
}

type FileListing struct {
	Entries []FoundFile
	Report  FileWalkReport
}

type fileGlob struct {
	pattern string
	include bool
}

type FileGlobs struct {
	patterns []fileGlob
}

func CompileFileGlobs(patterns []string) (*FileGlobs, error) {
	matcher := &FileGlobs{}
	for _, pattern := range patterns {
		include := !strings.HasPrefix(pattern, "!")
		if !include {
			pattern = pattern[1:]
		}
		if pattern == "" {
			return nil, fileFault(protocol.ErrorCodeInvalidRequest, "invalid file glob pattern")
		}
		pattern = strings.ReplaceAll(pattern, "\\", "\\\\")
		if !strings.Contains(pattern, "/") {
			pattern = "**/" + pattern
		}
		if !doublestar.ValidatePattern(pattern) {
			return nil, fileFault(protocol.ErrorCodeInvalidRequest, "invalid file glob pattern")
		}
		matcher.patterns = append(matcher.patterns, fileGlob{pattern: pattern, include: include})
	}
	return matcher, nil
}

func (globs *FileGlobs) matched(relative, scope string) int {
	if relative == scope {
		relative = baseFileName(relative)
	} else if after, ok := strings.CutPrefix(relative, scope+"/"); ok {
		relative = after
	}
	matched := -1
	for index, pattern := range globs.patterns {
		if found, _ := doublestar.Match(pattern.pattern, relative); found {
			matched = index
		}
	}
	for parent := fileParentPath(relative); parent != ""; parent = fileParentPath(parent) {
		for index, pattern := range globs.patterns {
			if !pattern.include {
				if found, _ := doublestar.Match(pattern.pattern, parent); found {
					matched = max(matched, index)
				}
			}
		}
	}
	return matched
}

func (globs *FileGlobs) Match(relative, scope string) bool {
	if matched := globs.matched(relative, scope); matched >= 0 {
		return globs.patterns[matched].include
	}
	for _, pattern := range globs.patterns {
		if pattern.include {
			return false
		}
	}
	return true
}

func (globs *FileGlobs) excludesDirectory(relative, scope string) bool {
	matched := globs.matched(relative, scope)
	if matched < 0 || globs.patterns[matched].include {
		return false
	}
	for _, pattern := range globs.patterns[matched+1:] {
		if pattern.include {
			return false
		}
	}
	return true
}

func (files *Files) FindFiles(ctx context.Context, patterns []string, scope string, limit, offset uint64) (protocol.FileNames, error) {
	if len(patterns) == 0 || limit == 0 {
		return protocol.FileNames{}, fileFault(protocol.ErrorCodeInvalidRequest, "file discovery requires nonempty glob patterns and a positive limit")
	}
	globs, err := CompileFileGlobs(patterns)
	if err != nil {
		return protocol.FileNames{}, err
	}
	listing, err := files.CollectFiles(ctx, scope, math.MaxUint64, globs)
	if err != nil {
		return protocol.FileNames{}, err
	}
	sort.Slice(listing.Entries, func(left, right int) bool {
		a, b := listing.Entries[left], listing.Entries[right]
		if a.Modified.Equal(b.Modified) {
			return a.Path < b.Path
		}
		return a.Modified.After(b.Modified)
	})
	total := uint64(len(listing.Entries))
	start := min(offset, total)
	end := start + min(limit, uint64(protocol.MAX_FIND_MATCHES), total-start)
	paths := make([]string, 0, end-start)
	for _, entry := range listing.Entries[start:end] {
		paths = append(paths, entry.Path)
	}
	var next *uint64
	if !listing.Report.Partial && end < total {
		value := end
		next = &value
	}
	return protocol.FileNames{Paths: paths, Visited: listing.Report.Visited, Skipped: listing.Report.Skipped, Truncated: listing.Report.Partial || end < total, Partial: listing.Report.Partial, Offset: offset, NextOffset: next, Diagnostics: listing.Report.Diagnostics}, nil
}

func (files *Files) CollectFiles(ctx context.Context, scope string, limit uint64, globs *FileGlobs) (FileListing, error) {
	listing := FileListing{Entries: []FoundFile{}}
	report, err := files.VisitFiles(ctx, scope, globs, func(_ *os.Root, _, relative string, metadata fs.FileInfo) (bool, error) {
		if uint64(len(listing.Entries)) >= limit {
			return false, fileFault(protocol.ErrorCodeBusy, "file discovery limit reached")
		}
		listing.Entries = append(listing.Entries, FoundFile{Path: relative, Modified: metadata.ModTime()})
		return true, nil
	})
	listing.Report = report
	return listing, err
}

func (files *Files) VisitFiles(ctx context.Context, scope string, globs *FileGlobs, visitor func(*os.Root, string, string, fs.FileInfo) (bool, error)) (FileWalkReport, error) {
	control := newFileControl(ctx)
	if err := control.check(); err != nil {
		return FileWalkReport{}, err
	}
	parts, err := fileComponents(scope, true, files.access, false)
	if err != nil {
		return FileWalkReport{}, err
	}
	for _, part := range parts {
		if strings.EqualFold(part, ".git") {
			return FileWalkReport{}, fileFault(protocol.ErrorCodePermissionDenied, "Git metadata cannot be searched; use the Git branches tool for remote names and URLs")
		}
	}
	if scope == "." {
		scope = ""
	}
	report := FileWalkReport{Diagnostics: []protocol.SearchDiagnostic{}}
	base, err := files.root.OpenRoot(".")
	if err != nil {
		if isFileFault(err, protocol.ErrorCodePermissionDenied) || errors.Is(err, fs.ErrPermission) {
			report.diagnostic(scope, fileIOFault(err).Error())
			return report, nil
		}
		return report, fileIOFault(err)
	}
	defer func() { base.Close() }()
	walker := fileWalker{files: files, scope: scope, globs: globs, control: control, report: &report, visitor: visitor}
	walker.layers = files.baseIgnoreRules(control, &report)
	prefix := ""
	for index, name := range parts {
		walker.layers = append(walker.layers, loadFileIgnore(base, prefix, ".gitignore", control, &report))
		walker.overrides = append(walker.overrides, loadFileIgnore(base, prefix, ".ignore", control, &report))
		relative := fileSearchRelative(prefix, name)
		metadata, err := base.Lstat(name)
		if err != nil {
			if errors.Is(err, fs.ErrPermission) {
				report.diagnostic(relative, fileIOFault(err).Error())
				return report, nil
			}
			return report, fileIOFault(err)
		}
		if metadata.Mode()&os.ModeSymlink != 0 || !metadata.Mode().IsRegular() && !metadata.IsDir() {
			return report, fileFault(protocol.ErrorCodeInvalidRequest, "search path must be a regular file or directory")
		}
		if metadata.Mode().IsRegular() {
			if index != len(parts)-1 {
				return report, fileFault(protocol.ErrorCodeInvalidRequest, "search path has a file as a parent")
			}
			if globs == nil || globs.Match(relative, scope) {
				if _, err := visitor(base, name, relative, metadata); err != nil {
					return report, err
				}
			}
			report.Visited++
			return report, nil
		}
		child, err := descendFiles(base, []string{name})
		if err != nil {
			if isFileFault(err, protocol.ErrorCodePermissionDenied) {
				report.diagnostic(relative, err.Error())
				return report, nil
			}
			return report, err
		}
		base.Close()
		base, prefix = child, relative
	}
	_, err = walker.descend(base, prefix, len(parts))
	if err != nil {
		if isFileFault(err, protocol.ErrorCodePermissionDenied) {
			report.diagnostic(scope, err.Error())
		} else if isFileFault(err, protocol.ErrorCodeCancelled) || isFileFault(err, protocol.ErrorCodeBusy) {
			report.diagnostic(scope, "scan interrupted; results are partial, narrow path before retrying")
		} else {
			return report, err
		}
	}
	return report, nil
}

type fileWalker struct {
	files     *Files
	scope     string
	globs     *FileGlobs
	layers    []fileIgnoreLayer
	overrides []fileIgnoreLayer
	control   fileControl
	report    *FileWalkReport
	visitor   func(*os.Root, string, string, fs.FileInfo) (bool, error)
}

func (walker *fileWalker) descend(directory *os.Root, prefix string, depth int) (bool, error) {
	if err := walker.control.check(); err != nil {
		return false, err
	}
	layers, overrides := len(walker.layers), len(walker.overrides)
	walker.layers = append(walker.layers, loadFileIgnore(directory, prefix, ".gitignore", walker.control, walker.report))
	walker.overrides = append(walker.overrides, loadFileIgnore(directory, prefix, ".ignore", walker.control, walker.report))
	defer func() {
		walker.layers = walker.layers[:layers]
		walker.overrides = walker.overrides[:overrides]
	}()
	file, err := directory.Open(".")
	if err != nil {
		return false, fileIOFault(err)
	}
	entries, err := file.ReadDir(-1)
	file.Close()
	if err != nil {
		return false, fileIOFault(err)
	}
	names := make([]fs.DirEntry, 0, len(entries))
	for _, entry := range entries {
		if err := walker.control.check(); err != nil {
			return false, err
		}
		walker.report.Visited++
		name := entry.Name()
		if strings.EqualFold(name, ".git") || walker.files.access == RestrictedFiles && managedFileDirectory(prefix, name) {
			continue
		}
		if !validFileEntry(name) {
			walker.report.diagnostic(prefix, "unsupported entry name")
			continue
		}
		names = append(names, entry)
	}
	sort.Slice(names, func(left, right int) bool {
		order := func(entry fs.DirEntry) string {
			if entry.IsDir() {
				return entry.Name() + "/"
			}
			return entry.Name()
		}
		return order(names[left]) < order(names[right])
	})
	for _, entry := range names {
		if err := walker.control.check(); err != nil {
			return false, err
		}
		name := entry.Name()
		relative := fileSearchRelative(prefix, name)
		if _, err := fileComponents(relative, false, walker.files.access, false); err != nil {
			return false, err
		}
		proceed, err := walker.visitEntry(directory, prefix, relative, name, depth)
		if err == nil {
			if !proceed {
				return false, nil
			}
			continue
		}
		if isFileFault(err, protocol.ErrorCodeCancelled) || isFileFault(err, protocol.ErrorCodeBusy) {
			return false, err
		}
		walker.report.diagnostic(relative, err.Error())
	}
	return true, nil
}

func (walker *fileWalker) visitEntry(directory *os.Root, prefix, relative, name string, depth int) (bool, error) {
	metadata, err := directory.Lstat(name)
	if err != nil {
		return false, fileIOFault(err)
	}
	if metadata.Mode()&os.ModeSymlink != 0 || !metadata.Mode().IsRegular() && !metadata.IsDir() {
		walker.report.Skipped++
		return true, nil
	}
	if fileIgnored(walker.overrides, walker.layers, relative, metadata.IsDir()) {
		return true, nil
	}
	if metadata.IsDir() {
		if walker.globs != nil && walker.globs.excludesDirectory(relative, walker.scope) {
			return true, nil
		}
		if depth >= 64 {
			walker.report.diagnostic(relative, "directory nesting exceeds the 64-level limit")
			return true, nil
		}
		child, err := descendFiles(directory, []string{name})
		if err != nil {
			return false, err
		}
		defer child.Close()
		return walker.descend(child, relative, depth+1)
	}
	if walker.globs == nil || walker.globs.Match(relative, walker.scope) {
		proceed, err := walker.visitor(directory, name, relative, metadata)
		if !proceed && err == nil {
			walker.report.Truncated = true
		}
		return proceed, err
	}
	return true, nil
}

type fileIgnoreRule struct {
	pattern         gitignore.Pattern
	directoriesOnly bool
	basename        bool
	domain          []string
}

type fileIgnoreLayer []fileIgnoreRule

func fileIgnored(overrides, layers []fileIgnoreLayer, relative string, directory bool) bool {
	parts := strings.Split(relative, "/")
	anchored := append(append([]string{}, parts...), "\x00")
	for _, group := range [][]fileIgnoreLayer{overrides, layers} {
		for layer := len(group) - 1; layer >= 0; layer-- {
			for pattern := len(group[layer]) - 1; pattern >= 0; pattern-- {
				rule := group[layer][pattern]
				if rule.directoriesOnly && !directory {
					continue
				}
				candidate := anchored
				if rule.basename {
					if len(parts) <= len(rule.domain) || !slices.Equal(parts[:len(rule.domain)], rule.domain) {
						continue
					}
					candidate = parts[len(parts)-1:]
				}
				if match := rule.pattern.Match(candidate, directory); match != gitignore.NoMatch {
					return match == gitignore.Exclude
				}
			}
		}
	}
	return false
}

func isFileFault(err error, code protocol.ErrorCode) bool {
	var fault *protocol.Fault
	return errors.As(err, &fault) && fault.Code == code
}
