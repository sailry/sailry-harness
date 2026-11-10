package process

import (
	"slices"
	"testing"
)

func TestSandboxArgsLinux(t *testing.T) {
	t.Parallel()
	policy := sandboxPolicy{paths: []allowedPath{{"/workspace/a/.git", false}, {"/workspace", true}, {"/usr", false}}, links: map[string]string{}}
	args := sandboxArgsLinux(policy, "/usr/bin/sh")
	for _, required := range []string{"--die-with-parent", "--unshare-pid", "--unshare-net", "--new-session"} {
		if !slices.Contains(args, required) {
			t.Fatal(args)
		}
	}
	index := func(flag, path string) int {
		for i := 0; i+2 < len(args); i++ {
			if args[i] == flag && args[i+1] == path && args[i+2] == path {
				return i
			}
		}
		return -1
	}
	root, parent, metadata := index("--bind", "/workspace"), index("--bind", "/workspace/a"), index("--ro-bind", "/workspace/a/.git")
	if root < 0 || parent < root || metadata < parent {
		t.Fatal("nested mounts and pinned parents changed", args)
	}
}
