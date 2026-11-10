// Portions adapted from ADK-Rust 675c02b6c0df6d632b953a06a5a12d1f814b7780.
// Copyright 2026 Zavora Technologies Ltd.; Apache-2.0. See third_party_licenses/adk-LICENSE.
package process

import (
	"maps"
	"path/filepath"
	"slices"
)

func sandboxCommand(policy sandboxPolicy, program string, args []string, _ string) (string, []string, error) {
	wrapper, err := trustedWrapper("/usr/bin/bwrap", "/bin/bwrap")
	if err != nil {
		return "", nil, err
	}
	return wrapper, append(append(sandboxArgsLinux(policy, program), "--", program), args...), nil
}

func sandboxArgsLinux(policy sandboxPolicy, program string) []string {
	args := []string{"--die-with-parent", "--unshare-pid", "--unshare-net", "--new-session"}
	paths := slices.Clone(policy.paths)
	for _, path := range pinnedPaths(policy.paths) {
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
	args = append(args, "--proc", "/proc", "--dev", "/dev")
	for _, alias := range []string{"/bin", "/sbin", "/lib", "/lib64"} {
		if target, err := filepath.EvalSymlinks(alias); err == nil && target != alias {
			args = append(args, "--ro-bind", target, alias)
		}
	}
	policy.linkAliases(program)
	for _, path := range slices.Sorted(maps.Keys(policy.links)) {
		args = append(args, "--symlink", policy.links[path], path)
	}
	protected := map[string]bool{}
	for _, path := range policy.protected {
		if resolved, err := filepath.EvalSymlinks(path); err == nil {
			protected[resolved] = true
		}
	}
	for _, path := range slices.Sorted(maps.Keys(protected)) {
		args = append(args, "--ro-bind", path, path)
	}
	return args
}
