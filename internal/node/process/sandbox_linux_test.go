package process

import (
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestMissingCredentialParentView(t *testing.T) {
	t.Parallel()
	root := t.TempDir()
	missing := filepath.Join(root, "config", "git", "credentials")
	sibling := filepath.Join(root, "tools")
	if err := os.Mkdir(sibling, 0700); err != nil {
		t.Fatal(err)
	}
	args, err := maskedParentViews([]string{missing, filepath.Join(root, ".netrc")})
	if err != nil {
		t.Fatal(err)
	}
	if !slices.Equal(args, []string{"--tmpfs", root, "--ro-bind", sibling, sibling, "--remount-ro", root}) {
		t.Fatalf("unexpected parent view: %v", args)
	}
}

// The terminal subtest sets a process variable, so this test is not parallel.
func TestSandboxArgsLinux(t *testing.T) {
	secretDir := t.TempDir()
	secretFile := filepath.Join(t.TempDir(), ".npmrc")
	if err := os.WriteFile(secretFile, []byte("token"), 0o600); err != nil {
		t.Fatal(err)
	}
	policy := sandboxPolicy{
		paths:       []allowedPath{{"/workspace/a/.git", false}, {"/workspace", true}, {"/usr", false}},
		deniedReads: []string{secretDir, secretFile},
	}
	args, err := sandboxArgsLinux(policy)
	if err != nil {
		t.Fatal(err)
	}
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
	if index("--ro-bind", "/") < 0 {
		t.Fatal("missing root read-only bind", args)
	}
	if tmpfs := slices.Index(args, secretDir); tmpfs < 1 || args[tmpfs-1] != "--tmpfs" {
		t.Fatal("denied secret directory not masked with tmpfs", args)
	}
	maskedFile := false
	for i := 0; i+2 < len(args); i++ {
		if args[i] == "--ro-bind" && args[i+1] == "/dev/null" && args[i+2] == secretFile {
			maskedFile = true
			break
		}
	}
	if !maskedFile {
		t.Fatal("denied secret file not masked with /dev/null", args)
	}
	root, parent, metadata := index("--bind", "/workspace"), index("--bind", "/workspace/a"), index("--ro-bind", "/workspace/a/.git")
	if root < 0 || parent < root || metadata < parent {
		t.Fatal("nested mounts and pinned parents changed", args)
	}
	t.Run("workspace terminal is wrapped with private state", func(t *testing.T) {
		workspace, profile := t.TempDir(), t.TempDir()
		t.Setenv("LD_PRELOAD", "/tmp/inherited.so")
		service, err := New(Options{Profile: profile, Environment: NewEnvironment([]string{"PATH=/usr/bin:/bin", "HOME=" + t.TempDir()}), CaptureManaged: func(string) (string, error) { return "", nil }})
		if err != nil {
			t.Fatal(err)
		}
		defer service.Close()
		command, variables, release, err := service.SandboxTerminal(workspace, []string{"/bin/sh", "-l"}, []string{"LD_AUDIT=/tmp/saved.so"})
		if _, missing := trustedWrapper("/usr/bin/bwrap", "/bin/bwrap"); missing != nil {
			var fault *protocol.Fault
			if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeUnavailable {
				t.Fatalf("a missing wrapper returned %v", err)
			}
			return
		}
		if err != nil {
			t.Fatal(err)
		}
		separator := slices.Index(command, "--")
		if separator < 0 || !slices.Equal(command[separator+1:], []string{"/bin/sh", "-l"}) || filepath.Base(command[0]) != "bwrap" || !slices.Contains(command, "--unshare-net") {
			t.Fatalf("terminal command %v", command)
		}
		resolved, _ := filepath.EvalSymlinks(workspace)
		if index := slices.Index(command, resolved); index < 1 || command[index-1] != "--bind" {
			t.Fatalf("the workspace is not writable in %v", command)
		}
		state := ""
		for _, entry := range variables {
			if value, found := strings.CutPrefix(entry, "HOME="); found {
				state = value
			}
		}
		stored, _ := filepath.EvalSymlinks(profile)
		if !strings.HasPrefix(state, filepath.Join(stored, "tmp")+string(filepath.Separator)) || !slices.Contains(variables, "LD_PRELOAD=") || !slices.Contains(variables, "LD_AUDIT=") {
			t.Fatalf("terminal variables %v", variables)
		}
		release()
		if _, err := os.Stat(state); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("released state remains: %v", err)
		}
	})
}

func TestSandboxProtectedParentsLinux(t *testing.T) {
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	protected := []string{
		filepath.Join(root, "nested", "repository", ".git"),
		filepath.Join(root, "nested", "worktree", ".git"),
		filepath.Join(root, ".sailry", "worktrees"),
	}
	for _, path := range protected {
		if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
			t.Fatal(err)
		}
		if filepath.Base(filepath.Dir(path)) == "worktree" {
			if err := os.WriteFile(path, []byte("fixture"), 0600); err != nil {
				t.Fatal(err)
			}
		} else if err := os.Mkdir(path, 0755); err != nil {
			t.Fatal(err)
		}
	}
	args, err := sandboxArgsLinux(sandboxPolicy{paths: []allowedPath{{root, true}}, protected: protected})
	if err != nil {
		t.Fatal(err)
	}
	index := func(flag, path string) int {
		for i := 0; i+2 < len(args); i++ {
			if args[i] == flag && args[i+1] == path && args[i+2] == path {
				return i
			}
		}
		return -1
	}
	for _, path := range protected {
		child := index("--ro-bind", path)
		if child < 0 {
			t.Fatalf("protected path is not read-only: %s, %v", path, args)
		}
		for parent := filepath.Dir(path); within(root, parent); parent = filepath.Dir(parent) {
			position := index("--bind", parent)
			if position < 0 || position >= child {
				t.Fatalf("protected parent is not pinned before its child: %s, %v", parent, args)
			}
		}
	}
}

func TestSandboxArgsLinuxReadOnly(t *testing.T) {
	t.Parallel()
	root, scratch := t.TempDir(), t.TempDir()
	args, err := sandboxArgsLinux(sandboxPolicy{
		paths:     []allowedPath{{root, false}, {scratch, true}},
		protected: []string{filepath.Join(root, ".git")},
	})
	if err != nil {
		t.Fatal(err)
	}
	index := func(flag, path string) int {
		for i := 0; i+2 < len(args); i++ {
			if args[i] == flag && args[i+1] == path && args[i+2] == path {
				return i
			}
		}
		return -1
	}
	if index("--bind", root) >= 0 {
		t.Fatalf("read-only sandbox mounted workspace writable: %v", args)
	}
	if index("--ro-bind", root) < 0 {
		t.Fatalf("read-only sandbox did not mount workspace read-only: %v", args)
	}
	if index("--bind", scratch) < 0 {
		t.Fatalf("read-only sandbox did not mount scratch writable: %v", args)
	}
}
