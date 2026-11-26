// Portions adapted from ADK-Rust 675c02b6c0df6d632b953a06a5a12d1f814b7780.
// Copyright 2026 Zavora Technologies Ltd.; Apache-2.0. See third_party_licenses/adk-LICENSE.
package process

import (
	"maps"
	"os"
	"path/filepath"
	"slices"
	"strings"
)

func sandboxCommand(policy sandboxPolicy, program string, args []string, _ string) (string, []string, error) {
	wrapper, err := trustedWrapper("/usr/bin/bwrap", "/bin/bwrap")
	if err != nil {
		return "", nil, err
	}
	mounts, err := sandboxArgsLinux(policy)
	if err != nil {
		return "", nil, sandboxFailure(err)
	}
	return wrapper, append(append(mounts, "--", program), args...), nil
}

func sandboxArgsLinux(policy sandboxPolicy) ([]string, error) {
	// Expose the host filesystem read-only so installed toolchains and caches
	// work without guesswork, then mask sensitive credentials and bind only
	// the workspace and scratch writable.
	args := []string{"--die-with-parent", "--unshare-pid", "--unshare-net", "--new-session", "--ro-bind", "/", "/", "--proc", "/proc", "--dev", "/dev"}
	denied := slices.Clone(policy.deniedReads)
	slices.SortFunc(denied, func(a, b string) int {
		x, y := len(strings.Split(a, string(filepath.Separator))), len(strings.Split(b, string(filepath.Separator)))
		if x != y {
			return x - y
		}
		return strings.Compare(a, b)
	})
	missing, err := maskedParentViews(denied)
	if err != nil {
		return nil, err
	}
	args = append(args, missing...)
	for _, path := range slices.Compact(denied) {
		info, err := os.Stat(path)
		if os.IsNotExist(err) {
			continue // Its first missing component is absent from the private parent.
		}
		if err != nil {
			return nil, err
		}
		if info.IsDir() {
			args = append(args, "--tmpfs", path)
		} else if info.Mode().IsRegular() {
			args = append(args, "--ro-bind", "/dev/null", path)
		}
	}
	protected := map[string]bool{}
	boundaries := slices.Clone(policy.paths)
	for _, path := range policy.protected {
		if resolved, err := filepath.EvalSymlinks(path); err == nil {
			protected[resolved] = true
			boundaries = append(boundaries, allowedPath{resolved, false})
		}
	}
	paths := slices.Clone(policy.paths)
	// A protected child mount can otherwise move with a writable ancestor.
	for _, path := range pinnedPaths(boundaries) {
		if !slices.ContainsFunc(paths, func(entry allowedPath) bool { return entry.path == path }) {
			paths = append(paths, allowedPath{path, true})
		}
	}
	for _, entry := range orderedPaths(paths) {
		flag := "--ro-bind"
		if entry.write {
			flag = "--bind"
		}
		args = append(args, flag, entry.path, entry.path)
	}
	for _, path := range slices.Sorted(maps.Keys(protected)) {
		args = append(args, "--ro-bind", path, path)
	}
	return args, nil
}

// A read-only bind is a live view: a credential that did not exist at launch
// can appear later. For each first missing component, project its parent's
// existing siblings into a private directory rather than modifying the host.
func maskedParentViews(denied []string) ([]string, error) {
	parents := map[string]map[string]bool{}
	for _, path := range denied {
		if _, err := os.Stat(path); !os.IsNotExist(err) {
			if err != nil {
				return nil, err
			}
			continue
		}
		for child := path; ; child = filepath.Dir(child) {
			parent := filepath.Dir(child)
			if _, err := os.Stat(parent); os.IsNotExist(err) {
				continue
			} else if err != nil {
				return nil, err
			}
			parent, err := filepath.EvalSymlinks(parent)
			if err != nil {
				return nil, err
			}
			if parents[parent] == nil {
				parents[parent] = map[string]bool{}
			}
			parents[parent][filepath.Base(child)] = true
			break
		}
	}
	var args []string
	for _, parent := range slices.Sorted(maps.Keys(parents)) {
		entries, err := os.ReadDir(parent)
		if err != nil {
			return nil, err
		}
		args = append(args, "--tmpfs", parent)
		for _, entry := range entries {
			if parents[parent][entry.Name()] {
				continue
			}
			path := filepath.Join(parent, entry.Name())
			if entry.Type()&os.ModeSymlink != 0 {
				target, err := os.Readlink(path)
				if err != nil {
					return nil, err
				}
				args = append(args, "--symlink", target, path)
			} else {
				args = append(args, "--ro-bind", path, path)
			}
		}
		args = append(args, "--remount-ro", parent)
	}
	return args, nil
}
