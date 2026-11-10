//go:build darwin || linux

package process

import (
	"io"
	"log/slog"
	"maps"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/go-git/go-git/v5/plumbing/format/gitignore"
)

func (policy *sandboxPolicy) captureRepositories(root string) {
	paths := map[string]bool{}
	visited := map[string]bool{}
	skipped := 0
	var target func(string)
	var common func(string)
	common = func(path string) {
		if value, ok := repositoryText(filepath.Join(path, "commondir")); ok && strings.TrimSpace(value) != "" {
			target(absoluteJoin(path, strings.TrimSpace(value)))
		}
	}
	target = func(path string) {
		resolved, err := filepath.EvalSymlinks(path)
		if err != nil || within(resolved, root) || visited[resolved] {
			return
		}
		visited[resolved] = true
		head, err := os.Lstat(filepath.Join(resolved, "HEAD"))
		if err != nil || !head.Mode().IsRegular() {
			return
		}
		objects, objectsErr := os.Lstat(filepath.Join(resolved, "objects"))
		commondir, commonErr := os.Lstat(filepath.Join(resolved, "commondir"))
		if !(objectsErr == nil && objects.IsDir() || commonErr == nil && commondir.Mode().IsRegular()) {
			return
		}
		paths[resolved] = true
		common(resolved)
	}
	inspect := func(path string) bool {
		entry := filepath.Join(path, ".git")
		info, err := os.Lstat(entry)
		if err != nil {
			if !os.IsNotExist(err) {
				skipped++
			}
			return false
		}
		paths[entry] = true
		if info.Mode().IsRegular() {
			if value, ok := repositoryText(entry); ok {
				if pointer, ok := strings.CutPrefix(strings.TrimSpace(value), "gitdir: "); ok {
					target(absoluteJoin(path, pointer))
				}
			}
		} else if info.IsDir() {
			common(entry)
		} else if info.Mode()&os.ModeSymlink != 0 {
			target(entry)
		}
		return true
	}
	for ancestor := root; ; ancestor = filepath.Dir(ancestor) {
		if inspect(ancestor) || filepath.Dir(ancestor) == ancestor {
			break
		}
	}
	var directory func(*os.Root, string, int, []gitignore.Pattern)
	directory = func(anchor *os.Root, relative string, depth int, patterns []gitignore.Pattern) {
		path := filepath.Join(root, relative)
		inspect(path)
		if text, ok := repositoryIgnore(anchor); ok {
			domain := []string{}
			if relative != "" {
				domain = strings.Split(filepath.ToSlash(relative), "/")
			}
			patterns = slices.Clone(patterns)
			for _, line := range strings.Split(text, "\n") {
				if line != "" && !strings.HasPrefix(line, "#") {
					patterns = append(patterns, gitignore.ParsePattern(line, domain))
				}
			}
		}
		matcher := gitignore.NewMatcher(patterns)
		file, err := anchor.Open(".")
		if err != nil {
			skipped++
			return
		}
		entries, err := file.ReadDir(-1)
		file.Close()
		if err != nil {
			skipped++
			return
		}
		for _, entry := range entries {
			if strings.EqualFold(entry.Name(), ".git") {
				continue
			}
			info, err := anchor.Lstat(entry.Name())
			if err != nil {
				skipped++
				continue
			}
			if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
				continue
			}
			childRelative := filepath.Join(relative, entry.Name())
			if matcher.Match(strings.Split(filepath.ToSlash(childRelative), "/"), true) {
				continue
			}
			if depth >= 64 {
				skipped++
				continue
			}
			child, err := anchor.OpenRoot(entry.Name())
			if err != nil {
				skipped++
				continue
			}
			opened, err := child.Stat(".")
			if err != nil || !os.SameFile(info, opened) {
				skipped++
				child.Close()
				continue
			}
			directory(child, childRelative, depth+1, patterns)
			child.Close()
		}
	}
	anchor, err := os.OpenRoot(root)
	if err == nil {
		directory(anchor, "", 0, nil)
		anchor.Close()
	} else {
		skipped++
	}
	for _, path := range slices.Sorted(maps.Keys(paths)) {
		if within(root, path) {
			policy.protected = append(policy.protected, path)
		} else if path = policy.read(path); path != "" {
			policy.protected = append(policy.protected, path)
		}
	}
	if skipped > 0 {
		slog.Warn("Git metadata discovery omitted unreadable or unsupported entries; discovered metadata remains protected", "omitted", skipped, "root", root)
	}
}

func absoluteJoin(parent, path string) string {
	if filepath.IsAbs(path) {
		return path
	}
	return filepath.Join(parent, path)
}

func repositoryText(path string) (string, bool) {
	parent, err := os.OpenRoot(filepath.Dir(path))
	if err != nil {
		return "", false
	}
	defer parent.Close()
	info, err := parent.Lstat(filepath.Base(path))
	if err != nil || !info.Mode().IsRegular() {
		return "", false
	}
	file, err := parent.Open(filepath.Base(path))
	if err != nil {
		return "", false
	}
	defer file.Close()
	opened, err := file.Stat()
	if err != nil || !os.SameFile(info, opened) {
		return "", false
	}
	value, err := io.ReadAll(io.LimitReader(file, 4097))
	return string(value), err == nil && len(value) <= 4096 && !strings.ContainsRune(string(value), 0)
}

func repositoryIgnore(anchor *os.Root) (string, bool) {
	info, err := anchor.Lstat(".gitignore")
	if err != nil || !info.Mode().IsRegular() {
		return "", false
	}
	file, err := anchor.Open(".gitignore")
	if err != nil {
		return "", false
	}
	defer file.Close()
	opened, err := file.Stat()
	if err != nil || !os.SameFile(info, opened) {
		return "", false
	}
	value, err := io.ReadAll(io.LimitReader(file, 1024*1024+1))
	return string(value), err == nil && len(value) <= 1024*1024
}
