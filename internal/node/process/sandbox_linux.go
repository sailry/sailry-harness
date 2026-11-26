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
	return wrapper, append(append(sandboxArgsLinux(policy, program), "--", program), args...), nil
}

func sandboxArgsLinux(policy sandboxPolicy, _ string) []string {
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
	for _, path := range slices.Compact(denied) {
		info, err := os.Stat(path)
		if err != nil {
			continue
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
	return args
}
