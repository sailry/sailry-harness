package git

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/sys/windows/registry"
)

func TestMain(m *testing.M) {
	if head := os.Getenv("SAILRY_TEST_WINDOWS_GIT_HEAD"); head != "" && slices.Equal(os.Args[1:], append([]string{"--no-pager"}, StatusArgs(nil)...)) {
		fmt.Fprintf(os.Stdout, "# branch.oid %s\x00# branch.head main\x00", head)
		os.Exit(0)
	}
	os.Exit(m.Run())
}

func TestWindowsGitExecutable(t *testing.T) {
	for _, key := range []string{"PATH", "ProgramW6432", "ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"} {
		t.Setenv(key, "")
	}
	t.Setenv("PATHEXT", ".EXE")
	t.Setenv("NoDefaultCurrentDirectoryInExePath", "1")
	t.Chdir(t.TempDir())
	writeExe := func(t *testing.T, dir string) string {
		t.Helper()
		if err := os.MkdirAll(dir, 0700); err != nil {
			t.Fatal(err)
		}
		path := filepath.Join(dir, "git.exe")
		if err := os.WriteFile(path, []byte("not a Windows executable"), 0600); err != nil {
			t.Fatal(err)
		}
		return path
	}
	for _, tc := range []struct{ key, dir string }{
		{"ProgramW6432", `Git\cmd`}, {"ProgramFiles", `Git\cmd`},
		{"ProgramFiles(x86)", `Git\bin`}, {"LOCALAPPDATA", `Programs\Git\cmd`},
	} {
		t.Run(tc.key, func(t *testing.T) {
			base := t.TempDir()
			t.Setenv(tc.key, base)
			want := writeExe(t, filepath.Join(base, tc.dir))
			cmd, err := nativeGitCommand("status")
			if err != nil || cmd.Err != nil || cmd.Path != want {
				t.Fatalf("command = %v, error = %v; want %s", cmd, err, want)
			}
		})
	}
	t.Run("inspect_git without PATH", func(t *testing.T) {
		base := t.TempDir()
		t.Setenv("ProgramFiles", base)
		destination := writeExe(t, filepath.Join(base, "Git", "cmd"))
		self, err := os.Executable()
		if err != nil {
			t.Fatal(err)
		}
		body, err := os.ReadFile(self)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(destination, body, 0700); err != nil {
			t.Fatal(err)
		}
		root, _, head := fixture(t)
		t.Setenv("SAILRY_TEST_WINDOWS_GIT_HEAD", head)
		out, err := New(Options{}).Read(t.Context(), root, protocol.Command{Kind: "inspect_git"})
		status, ok := out.Data.(protocol.GitStatus)
		if err != nil || !ok || status.Kind != protocol.RepositoryKindReady || status.Head == nil || *status.Head != head || status.Branch == nil || *status.Branch != "main" {
			t.Fatalf("inspect_git status = %+v, error = %v", status, err)
		}
	})
	t.Run("PATH precedence and start failure", func(t *testing.T) {
		base := t.TempDir()
		t.Setenv("ProgramFiles", base)
		writeExe(t, filepath.Join(base, "Git", "cmd"))
		dir := t.TempDir()
		t.Setenv("PATH", dir)
		want := writeExe(t, dir)
		cmd, err := nativeGitCommand("status")
		if err != nil || cmd.Err != nil || cmd.Path != want {
			t.Fatalf("command = %v, error = %v; want %s", cmd, err, want)
		}
		root, _, _ := fixture(t)
		_, err = New(Options{}).Read(t.Context(), root, protocol.Command{Kind: "inspect_git"})
		var f *protocol.Fault
		if !errors.As(err, &f) || f.Code != protocol.ErrorCodeUnavailable || f.Message != "Could not start the native Git command on execution Node" {
			t.Fatalf("inspect_git error = %v", err)
		}
	})
	t.Run("registry", func(t *testing.T) {
		root := t.TempDir()
		want := writeExe(t, filepath.Join(root, "cmd"))
		got, err := windowsGitPath("git", exec.ErrNotFound, func(hive registry.Key, view uint32) (string, error) {
			if hive == registry.LOCAL_MACHINE && view == registry.WOW64_32KEY {
				return root, nil
			}
			return "", registry.ErrNotExist
		})
		if err != nil || got != want {
			t.Fatalf("path = %q, error = %v; want %s", got, err, want)
		}
	})
	t.Run("no worktree-relative installation", func(t *testing.T) {
		t.Setenv("ProgramFiles", ".")
		writeExe(t, `Git\cmd`)
		_, err := windowsGitPath("git", exec.ErrNotFound, func(registry.Key, uint32) (string, error) { return "Git", nil })
		if !errors.Is(err, exec.ErrNotFound) {
			t.Fatalf("lookup error = %v", err)
		}
		requireFault(t, gitLookupFault(err), protocol.ErrorCodeNotConfigured)
	})
	t.Run("unsafe or denied lookup does not try an installation", func(t *testing.T) {
		base := t.TempDir()
		t.Setenv("ProgramFiles", base)
		writeExe(t, filepath.Join(base, "Git", "cmd"))
		for _, tc := range []struct{ initial, want error }{
			{exec.ErrDot, exec.ErrDot}, {nil, exec.ErrDot}, {os.ErrPermission, os.ErrPermission},
		} {
			_, err := windowsGitPath(`.\git.exe`, tc.initial, nil)
			if !errors.Is(err, tc.want) {
				t.Fatalf("lookup error = %v; want %v", err, tc.want)
			}
			requireFault(t, gitLookupFault(err), protocol.ErrorCodePermissionDenied)
		}
	})
}
