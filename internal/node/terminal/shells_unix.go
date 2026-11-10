//go:build !windows

package terminal

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func shellCandidates(environment []string) []string {
	candidates := []string{}
	if body, err := os.ReadFile("/etc/shells"); err == nil {
		for line := range strings.Lines(string(body)) {
			path, _, _ := strings.Cut(line, "#")
			if path = strings.TrimSpace(path); strings.HasPrefix(path, "/") {
				candidates = append(candidates, path)
			}
		}
	}
	if login := environmentValue(environment, "SHELL"); filepath.IsAbs(login) {
		candidates = append(candidates, login)
	}
	candidates = append(candidates, "/bin/sh")
	if runtime.GOOS == "darwin" {
		for _, directory := range []string{"/bin", "/usr/bin", "/opt/homebrew/bin", "/usr/local/bin"} {
			for _, name := range []string{"sh", "bash", "zsh", "fish", "nu", "pwsh", "csh", "tcsh", "ksh"} {
				candidates = append(candidates, filepath.Join(directory, name))
			}
		}
	}
	for _, name := range []string{"sh", "bash", "zsh", "fish", "nu", "pwsh"} {
		if path, found := findExecutable("/", environment, name); found {
			candidates = append(candidates, path)
		}
	}
	return candidates
}

func shellIdentity(path string) (string, bool) { return path, true }

func wslShells([]string, []protocol.WslDistribution) []protocol.TerminalShell { return nil }
