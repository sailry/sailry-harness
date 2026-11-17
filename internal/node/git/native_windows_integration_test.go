//go:build windows && integration

package git

import (
	"bytes"
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/sys/windows"
)

func TestWindowsNativeGitBackgroundCommands(t *testing.T) {
	const probe = "SAILRY_TEST_GIT_NO_CONSOLE"
	if os.Getenv(probe) == "1" {
		getConsoleWindow := windows.NewLazySystemDLL("kernel32.dll").NewProc("GetConsoleWindow")
		if err := getConsoleWindow.Find(); err != nil {
			t.Fatal(err)
		}
		console, _, _ := getConsoleWindow.Call()
		if console != 0 {
			t.Fatalf("background Git process has a console window: %#x", console)
		}
		fmt.Fprintln(os.Stdout, "background process has no console")
		return
	}

	t.Run("no_console", func(t *testing.T) {
		ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
		defer cancel()
		command := exec.CommandContext(ctx, os.Args[0], "-test.run=^TestWindowsNativeGitBackgroundCommands$")
		command.Env = append(os.Environ(), probe+"=1")
		var output bytes.Buffer
		command.Stdout, command.Stderr = &output, &output
		command.WaitDelay = stopGrace
		cleanup, err := prepareNative(command)
		if err != nil {
			t.Fatal(err)
		}
		defer cleanup()
		command.Cancel = func() error { killNative(command); return nil }
		if err := command.Start(); err != nil {
			t.Fatal(err)
		}
		if err := ownNative(command); err != nil {
			_ = command.Process.Kill()
			_ = command.Wait()
			t.Fatal(err)
		}
		if err := command.Wait(); err != nil {
			t.Fatalf("console probe failed: %v\n%s", err, output.String())
		}
		if !strings.Contains(output.String(), "background process has no console") {
			t.Fatalf("console probe did not report its result: %s", output.String())
		}
	})

	if _, err := exec.LookPath("git"); err != nil {
		t.Fatalf("Git executable is required: %v", err)
	}
	for _, test := range []struct {
		name      string
		committed bool
		kind      protocol.RepositoryKind
	}{
		{name: "unborn_local_repository", kind: protocol.RepositoryKindUnborn},
		{name: "committed_local_repository", committed: true, kind: protocol.RepositoryKindReady},
	} {
		t.Run(test.name, func(t *testing.T) {
			home := t.TempDir()
			t.Setenv("HOME", home)
			t.Setenv("USERPROFILE", home)
			t.Setenv("XDG_CONFIG_HOME", filepath.Join(home, ".config"))
			root := t.TempDir()
			service := New(Options{Environment: os.Environ()})
			run := func(args ...string) {
				t.Helper()
				result, err := service.command(t.Context(), root, args, nil, false)
				if err != nil {
					t.Fatalf("Git %v: %v\n%s", args, err, result.Stderr)
				}
			}
			run("init", "--initial-branch=main", "--template=")
			if test.committed {
				run("-c", "user.name=Fixture", "-c", "user.email=fixture@example.test", "-c", "commit.gpgsign=false", "commit", "--allow-empty", "--message", "Initial fixture commit")
			}
			for refresh := 0; refresh < 2; refresh++ {
				service.Invalidate(root)
				status, err := service.Status(t.Context(), root)
				if err != nil {
					t.Fatalf("status refresh %d: %v\n%s", refresh, err, service.Output(root))
				}
				if status.Kind != test.kind || status.Branch == nil || *status.Branch != "main" || len(status.Entries) != 0 || (status.Head != nil) != test.committed {
					t.Fatalf("status refresh %d: %+v", refresh, status)
				}
				// Branch metadata includes the native stash listing used when a
				// newly created conversation opens its workspace.
				branches, err := service.Branches(t.Context(), root)
				if err != nil {
					t.Fatalf("branch refresh %d: %v\n%s", refresh, err, service.Output(root))
				}
				if branches.Kind != test.kind || branches.Current == nil || *branches.Current != "main" || len(branches.Sync.Remotes) != 0 || len(branches.Sync.Stashes) != 0 {
					t.Fatalf("branch refresh %d: %+v", refresh, branches)
				}
			}
		})
	}
}
