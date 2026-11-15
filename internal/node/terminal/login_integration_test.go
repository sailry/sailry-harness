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

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestTerminalCapturedLoginEnvironmentIntegration(t *testing.T) {
	zsh, err := exec.LookPath("zsh")
	if err != nil {
		t.Skip("zsh is not installed")
	}
	// The inherited shell deliberately disagrees with the Node's capture.
	// Every startup file belongs to this test, including zsh integration's
	// restored ZDOTDIR; no account configuration is read or modified.
	t.Setenv("SHELL", "/bin/sh")
	home := t.TempDir()
	bin := filepath.Join(home, "login-bin")
	for name, source := range map[string]string{
		".zshenv":   "export SAILRY_LOGIN_TRACE=env\n",
		".zprofile": "export SAILRY_LOGIN_TRACE=${SAILRY_LOGIN_TRACE}:profile\nexport PATH='" + strings.ReplaceAll(bin, "'", "'\\''") + "':$PATH\n",
		".zshrc":    "export SAILRY_LOGIN_TRACE=${SAILRY_LOGIN_TRACE}:rc\n",
		".zlogin":   "export SAILRY_LOGIN_TRACE=${SAILRY_LOGIN_TRACE}:login\n",
	} {
		if err := os.WriteFile(filepath.Join(home, name), []byte(source), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	ctx, cancel := context.WithTimeout(t.Context(), 20*time.Second)
	defer cancel()
	service := New(ctx, protocol.NodeID{1}, nil)
	defer service.Close()
	caller := protocol.NodeID{2}
	info, err := service.Start(ctx, Launch{
		Caller: caller, Worktree: protocol.WorktreeID{3}, Root: home,
		Environment: []string{"HOME=" + home, "ZDOTDIR=" + home, "SHELL=" + zsh, "PATH=/usr/bin:/bin"},
		Access:      protocol.ProcessAccessUnrestricted, Viewport: protocol.TerminalViewport{Columns: 500, Rows: 24},
	})
	if err != nil {
		t.Fatal(err)
	}
	command := "printf 'sailry-login:%s\\n' \"$SAILRY_LOGIN_TRACE\"; printf 'sailry-path:%s\\n' \"$PATH\"; printf 'sailry-shell:%s:%s\\n' \"$SHELL\" \"$options[login]\"\r"
	if _, err := service.Input(caller, protocol.CommandInputTerminalPayload{Terminal: info.ID, Revision: info.Revision, Bytes: []byte(command)}); err != nil {
		t.Fatal(err)
	}
	value, err := service.get(info.ID)
	if err != nil {
		t.Fatal(err)
	}
	want := []string{"sailry-login:env:profile:rc:login", "sailry-path:" + bin + ":", "sailry-shell:" + zsh + ":on"}
	ticker := time.NewTicker(10 * time.Millisecond)
	defer ticker.Stop()
	var screen string
	for {
		screen = value.terminal.Text()
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
