package node

import (
	"bufio"
	"database/sql"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/program"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginVersionProgramFixture(t *testing.T) {
	if os.Getenv("PLUGIN_VERSION_PROGRAM_FIXTURE") != "1" {
		return
	}
	fmt.Fprintln(os.Stdout, "ready")
	_, _ = io.Copy(io.Discard, os.Stdin)
}

func TestPluginReplacementKeepsCapturedProgram(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("the program fixture uses a symbolic link")
	}
	node := nodeFixture(t)
	self, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	bin := t.TempDir()
	if err := os.Symlink(self, filepath.Join(bin, "version-program")); err != nil {
		t.Fatal(err)
	}
	var user protocol.User
	if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		var err error
		user, err = store.LocalUser(t.Context(), db)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	entry, err := node.plugins.programs.Lease(pluginProgramOwner(user.ID, "version-fixture"), func(*program.Entry) int { return 1 }, func() (*program.Process, any, error) {
		value, err := program.Start(program.Spec{Command: "version-program", Args: []string{"-test.run=^TestPluginVersionProgramFixture$"},
			Env: map[string]string{"PLUGIN_VERSION_PROGRAM_FIXTURE": "1"}, Host: []string{"PATH=" + bin}, Dir: t.TempDir(), Log: "version-fixture"})
		return value, nil, err
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { node.plugins.programs.Release(entry, false) })
	line, err := bufio.NewReader(entry.Process.Stdout).ReadString('\n')
	if err != nil || line != "ready\n" {
		t.Fatalf("fixture did not start: %q %v", line, err)
	}
	summary := protocol.DefaultPluginSummary()
	summary.Name, summary.Enabled, summary.Revision = "version-fixture", true, 1
	change := []protocol.Event{{Kind: "plugin_changed", Data: summary}}
	node.pluginCommitted(change, t.Context(), true)
	select {
	case <-entry.Process.Done():
		t.Fatal("code replacement killed a program serving captured input")
	default:
	}
	summary.Enabled, summary.Revision = false, 2
	change[0].Data = summary
	node.pluginCommitted(change, t.Context(), false)
	select {
	case <-entry.Process.Done():
	case <-time.After(10 * time.Second):
		t.Fatal("disabling a package did not stop its program")
	}
}
