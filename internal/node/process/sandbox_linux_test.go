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

// The terminal subtest sets a process variable, so this test is not parallel.
func TestSandboxArgsLinux(t *testing.T) {
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
