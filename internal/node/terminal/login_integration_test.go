//go:build integration && (darwin || linux)

package terminal

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestTerminalCapturedLoginEnvironmentIntegration(t *testing.T) {
	zsh, err := exec.LookPath("zsh")
	if err != nil {
		t.Skip("zsh is not installed")
	}
	t.Setenv("SHELL", "/bin/sh")
	t.Setenv("ZDOTDIR", filepath.Join(t.TempDir(), "editor-hooks"))
	for _, mode := range []string{"account home", "startup-selected directory", "explicit directory"} {
		t.Run(mode, func(t *testing.T) {
			testTerminalLoginEnvironment(t, zsh, mode)
		})
	}
}

func testTerminalLoginEnvironment(t *testing.T, zsh, mode string) {
	t.Helper()
	home := t.TempDir()
	directory := home
	if mode != "account home" {
		directory = filepath.Join(home, "config")
		if err := os.Mkdir(directory, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	bin := filepath.Join(home, "login-bin")
	for name, source := range map[string]string{
		".zshenv":   "export SAILRY_LOGIN_TRACE=env\n",
		".zprofile": "export SAILRY_LOGIN_TRACE=${SAILRY_LOGIN_TRACE}:profile\nexport PATH='" + strings.ReplaceAll(bin, "'", "'\\''") + "':$PATH\n",
		".zshrc":    "export SAILRY_LOGIN_TRACE=${SAILRY_LOGIN_TRACE}:rc\nPROMPT='sailry-configured-prompt> '\n",
		".zlogin":   "export SAILRY_LOGIN_TRACE=${SAILRY_LOGIN_TRACE}:login\n",
	} {
		if err := os.WriteFile(filepath.Join(directory, name), []byte(source), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	if mode == "startup-selected directory" {
		source := "export SAILRY_LOGIN_TRACE=env\nexport ZDOTDIR='" + strings.ReplaceAll(directory, "'", "'\\''") + "'\n"
		if err := os.WriteFile(filepath.Join(home, ".zshenv"), []byte(source), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	environment := process.NewEnvironment([]string{"HOME=" + home, "SHELL=" + zsh, "PATH=/usr/bin:/bin"}).TerminalValues()
	if mode == "explicit directory" {
		environment = append(environment, "ZDOTDIR="+directory)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 20*time.Second)
	defer cancel()
	service := New(ctx, protocol.NodeID{1}, nil)
	// Service shutdown waits until the shell has finished writing exit files.
	defer service.Close()
	caller := protocol.NodeID{2}
	info, err := service.Start(ctx, Launch{
		Caller: caller, Worktree: protocol.WorktreeID{3}, Root: home,
		Environment: environment,
		Access:      protocol.ProcessAccessUnrestricted, Viewport: protocol.TerminalViewport{Columns: 500, Rows: 24},
	})
	if err != nil {
		t.Fatal(err)
	}
	value, err := service.get(info.ID)
	if err != nil {
		t.Fatal(err)
	}
	value.mu.Lock()
	terminal := value.terminal
	value.mu.Unlock()
	if terminal == nil {
		t.Fatal("login shell exited before reading its environment")
	}
	command := "printf 'sailry-login:%s\\n' \"$SAILRY_LOGIN_TRACE\"; printf 'sailry-path:%s\\n' \"$PATH\"; printf 'sailry-shell:%s:%s\\n' \"$SHELL\" \"$options[login]\"\r"
	if _, err := service.Input(caller, protocol.CommandInputTerminalPayload{Terminal: info.ID, Revision: info.Revision, Bytes: []byte(command)}); err != nil {
		t.Fatal(err)
	}
	want := []string{"sailry-login:env:profile:rc:login", "sailry-path:" + bin + ":", "sailry-shell:" + zsh + ":on", "sailry-configured-prompt> "}
	ticker := time.NewTicker(10 * time.Millisecond)
	defer ticker.Stop()
	var screen string
	for {
		screen = terminal.Text()
		matched := true
		for _, needle := range want {
			matched = matched && strings.Contains(screen, needle)
		}
		if matched {
			return
		}
		select {
		case <-ctx.Done():
			t.Fatalf("captured login environment was not loaded: %v; screen: %q", ctx.Err(), screen)
		case <-ticker.C:
		}
	}
}
