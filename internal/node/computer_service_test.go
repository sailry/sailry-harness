package node

import (
	"database/sql"
	"os"
	"path/filepath"
	"runtime"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/computer"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
)

// A helper launch that joined the cursor overlay still validates: the session
// variable startComputer adds after authorization is not a changed credential.
func TestComputerSessionKeepsMCPAuthorization(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("the helper stand-in is a shell script")
	}
	// A stand-in helper that answers the overlay's session commands.
	helpers := t.TempDir()
	helper := filepath.Join(helpers, "agent-desktop")
	if runtime.GOOS == "darwin" {
		helper = filepath.Join(helpers, "Sailry Computer Use.app", "Contents", "MacOS", "agent-desktop")
	}
	script := `#!/bin/sh
if [ "$2" = start ]; then
  /bin/mkdir -p "$AGENT_DESKTOP_HOME/sessions/run-1"
  printf '{"id":"run-1","ended_at":null}' > "$AGENT_DESKTOP_HOME/sessions/run-1/session.json"
  echo '{"ok":true,"data":{"session_id":"run-1"}}'
else
  echo '{"ok":true,"data":{}}'
fi
`
	if err := os.MkdirAll(filepath.Dir(helper), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(helper, []byte(script), 0o700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("SAILRY_HELPERS_DIR", helpers)
	node := nodeFixture(t)
	ctx := t.Context()
	source := t.TempDir()
	documents := map[string]string{
		"plugin.json": `{"$schema":"` + plugins.ManifestSchema + `","name":"computer-fixture","version":"0.1.0"}`,
		"mcp.json":    `{"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"desktop":{"type":"stdio","command":"${SAILRY_COMPUTER_USE}","args":["mcp"],"env":{"AGENT_DESKTOP_HOME":"${PLUGIN_DATA}/agent-desktop"},"reuse":false}}}`,
	}
	for name, document := range documents {
		if err := os.WriteFile(filepath.Join(source, name), []byte(document), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	info, err := node.plugins.manager.Install(ctx, source, "", "computer-fixture")
	if err != nil || len(info.Issues) != 0 {
		t.Fatalf("computer fixture package: %v, %+v", err, info.Issues)
	}
	err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		info, err = store.InstallPlugin(ctx, db, info, 0, true)
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	info, settings, configuration, err := node.capturedPluginMCP(ctx, plugins.PackageReference(info), true)
	if err != nil {
		t.Fatal(err)
	}
	server := configuration.Servers["desktop"]
	prepared, err := node.prepareMCPLaunch(ctx, info, settings, "desktop", server)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = prepared.Release() }()
	node.startComputer(info, server, &prepared)
	if prepared.Launch.Env[computer.SessionVariable] != "run-1" {
		t.Fatalf("the helper launch did not join the overlay session: %v", prepared.Launch.Env)
	}
	if err := prepared.Validate(ctx); err != nil {
		t.Fatalf("a helper launch in an overlay session failed validation: %v", err)
	}
}
