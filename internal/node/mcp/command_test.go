package mcp

import (
	"os/exec"
	"runtime"
	"testing"
)

// A server that exits non-zero when its input closes is gone all the same:
// its exit status is not a cleanup failure.
func TestCommandCloseExitStatus(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("the fixture server is a shell command")
	}
	wire, err := commandTransport{command: exec.Command("/bin/sh", "-c", "cat >/dev/null; exit 3")}.Connect(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err := wire.Close(); err != nil {
		t.Fatalf("an exited server's status failed its cleanup: %v", err)
	}
}
