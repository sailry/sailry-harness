package mcp

import (
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"testing"
)

func TestStdioCommandUsesNodePath(t *testing.T) {
	t.Setenv("PATH", t.TempDir())
	bin := t.TempDir()
	const name = "mcp-fixture"
	path := filepath.Join(bin, name)
	pathName := "PATH"
	if runtime.GOOS == "windows" {
		path += ".exe"
		pathName = "Path"
	}
	if err := os.WriteFile(path, []byte("fixture executable"), 0700); err != nil {
		t.Fatal(err)
	}
	directory := t.TempDir()
	launch := Launch{Command: name, Args: []string{"serve", "--mcp"}, Directory: directory,
		Environment: []string{pathName + "=" + bin}, Env: map[string]string{"PATH": directory}}
	command, err := stdioCommand(launch)
	if err != nil {
		t.Fatal(err)
	}
	expected, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	resolved, err := os.Stat(command.Path)
	if err != nil {
		t.Fatal(err)
	}
	if !os.SameFile(expected, resolved) || command.Dir != directory || !slices.Equal(command.Args, []string{name, "serve", "--mcp"}) {
		t.Fatalf("unexpected MCP invocation: path %q, directory %q, argv %q", command.Path, command.Dir, command.Args)
	}
	if !slices.Contains(command.Environ(), "PATH="+directory) {
		t.Fatal("the server's PATH override did not reach the child")
	}
}

// A server that exits non-zero when its input closes is gone all the same:
// its exit status is not a cleanup failure.
func TestCommandCloseExitStatus(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("the fixture server is a shell command")
	}
	command, err := stdioCommand(Launch{Command: "/bin/sh", Args: []string{"-c", "cat >/dev/null; exit 3"}})
	if err != nil {
		t.Fatal(err)
	}
	wire, err := commandTransport{command: command}.Connect(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err := wire.Close(); err != nil {
		t.Fatalf("an exited server's status failed its cleanup: %v", err)
	}
}
