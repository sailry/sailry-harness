package terminal

import (
	"errors"
	"os"
	"path/filepath"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// testShellCommand checks how saved settings become a terminal program. It is
// a helper for a grouped test to call as a subtest; it starts no process.
func TestShellCommand(t *testing.T) {
	t.Helper()
	if command, variables, directory, err := shellCommand("linux", protocol.TerminalSettings{}, "/work"); err != nil || command != nil || variables != nil || directory != "/work" {
		t.Fatalf("default shell = %v %v %q %v", command, variables, directory, err)
	}
	shell := filepath.Join(t.TempDir(), "shell")
	if _, _, _, err := shellCommand("linux", protocol.TerminalSettings{Shell: shell}, "/work"); err == nil {
		t.Fatal("a missing shell was accepted")
	}
	if err := os.WriteFile(shell, []byte("#!/bin/sh\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	command, variables, _, err := shellCommand("linux", protocol.TerminalSettings{Shell: shell}, "/work")
	if err != nil || !slices.Equal(command, []string{shell, "-l"}) || !slices.Equal(variables, []string{"SHELL=" + shell}) {
		t.Fatalf("login shell = %v %v %v", command, variables, err)
	}
	command, _, _, err = shellCommand("linux", protocol.TerminalSettings{Shell: shell, ShellArguments: []string{"-i"}}, "/work")
	if err != nil || !slices.Equal(command, []string{shell, "-i"}) {
		t.Fatalf("shell with arguments = %v %v", command, err)
	}
	if values := Environment([]string{"A=1", "B=2"}, protocol.TerminalSettings{Environment: map[string]string{"B": "3", "C": "4"}}); !slices.Equal(values, []string{"A=1", "B=2", "B=3", "C=4"}) {
		t.Fatalf("environment = %v", values)
	}

	const launcher = `C:\Windows\System32\wsl.exe`
	for _, test := range []struct {
		name      string
		shell     string
		arguments []string
		root      string
		command   []string
		directory string
		message   string
	}{
		{"plain shell", `C:\Program Files\PowerShell\7\pwsh.exe`, []string{"-NoLogo"}, `C:\work`, []string{`C:\Program Files\PowerShell\7\pwsh.exe`, "-NoLogo"}, `C:\work`, ""},
		{"local drive", launcher, nil, `C:\work`, []string{launcher, "--cd", `C:\work`}, `C:\work`, ""},
		{"verbatim drive", launcher, nil, `\\?\C:\work`, []string{launcher, "--cd", `C:\work`}, `C:\work`, ""},
		{"own share", launcher, []string{"--distribution", "Ubuntu"}, `\\wsl$\ubuntu\home\me\work`, []string{launcher, "--distribution", "Ubuntu", "--cd", "/home/me/work"}, `C:\Windows\System32`, ""},
		{"localhost share root", launcher, []string{"-d", "Ubuntu"}, `\\wsl.localhost\Ubuntu`, []string{launcher, "-d", "Ubuntu", "--cd", "/"}, `C:\Windows\System32`, ""},
		{"another distribution", launcher, []string{"-d", "Debian"}, `\\wsl$\Ubuntu\home`, nil, "", "WSL worktree requires its matching distribution"},
		{"no distribution", launcher, nil, `\\wsl$\Ubuntu\home`, nil, "", "WSL worktree requires its matching distribution"},
		{"network share", launcher, nil, `\\server\share\work`, nil, "", "WSL worktree requires a local drive or matching distribution path"},
	} {
		command, _, directory, err := shellCommand("windows", protocol.TerminalSettings{Shell: test.shell, ShellArguments: test.arguments}, test.root)
		message := ""
		var failure *protocol.Fault
		if errors.As(err, &failure) {
			message = failure.Message
		}
		if message != test.message || !slices.Equal(command, test.command) || directory != test.directory {
			t.Fatalf("%s = %v %q %v", test.name, command, directory, err)
		}
	}
}
