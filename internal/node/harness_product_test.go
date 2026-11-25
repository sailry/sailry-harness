package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"path/filepath"
	"reflect"
	"slices"
	"testing"

	hostassembly "github.com/sailry/sailry-harness/internal/assembly/host"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHarnessHostPreservesExplicitPluginChoices(t *testing.T) {
	t.Parallel()
	profile := filepath.Join(t.TempDir(), "profile")
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin", "HOME=" + t.TempDir()})
	start := func() *Node {
		t.Helper()
		owner, err := StartWithOptions(t.Context(), profile, Options{
			ProductID: "harness", Headless: true, Assembly: hostassembly.Harness(),
			Environment: &environment, MCP: fixtureMCPOptions(),
		})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() {
			if err := owner.Stop(context.Background()); err != nil {
				t.Error(err)
			}
		})
		if runtime := owner.pluginRuntime(); runtime.ProductID != "harness" || !slices.Contains(runtime.Modules, "files") {
			t.Fatalf("Harness Host lost its native files service: %+v", runtime)
		}
		for _, name := range []string{"read_file", "write_file", "list_directory", "find_files", "search_files"} {
			if _, ok := owner.modules.LookupTool(name); !ok {
				t.Errorf("Harness Host has no native %s tool", name)
			}
		}
		return owner
	}
	owner := start()
	if inventory := snapshotFixture(t, owner).Plugins; len(inventory) != 0 {
		t.Fatalf("a fresh Harness Host installed plugins: %+v", inventory)
	}

	selected := installPluginFixture(t, owner, "goals")
	installPluginFixture(t, owner, "reminders")
	removed := installPluginFixture(t, owner, "brief")
	settings := commandFixture(t, owner, protocol.Command{Kind: "save_plugin_settings", Data: protocol.CommandSavePluginSettingsPayload{
		Package: plugins.PackageReference(selected), Values: map[string]json.RawMessage{
			"turn_limit": json.RawMessage(`17`), "token_budget": json.RawMessage(`3`), "time_limit": json.RawMessage(`5`),
		},
	}}).Data.(protocol.PluginSettingsState)
	selected = commandFixture(t, owner, protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: selected.Summary.Name}}).Data.(protocol.PluginInfo)
	selected = commandFixture(t, owner, protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{
		Name: selected.Summary.Name, ExpectedRevision: selected.Summary.Revision, Enabled: false,
	}}).Data.(protocol.PluginInfo)
	selected = commandFixture(t, owner, protocol.Command{Kind: "set_plugin_pinned", Data: protocol.CommandSetPluginPinnedPayload{
		Name: selected.Summary.Name, ExpectedRevision: selected.Summary.Revision, Pinned: true,
	}}).Data.(protocol.PluginInfo)
	commandFixture(t, owner, protocol.Command{Kind: "remove_plugin", Data: protocol.CommandRemovePluginPayload{
		Name: removed.Summary.Name, ExpectedRevision: removed.Summary.Revision,
	}})
	before := snapshotFixture(t, owner).Plugins
	if len(before) != 2 || selected.Summary.Enabled || !selected.Summary.Pinned {
		t.Fatalf("plugin choices were not applied before restart: %+v", before)
	}
	if err := owner.Stop(context.Background()); err != nil {
		t.Fatal(err)
	}

	restarted := start()
	if after := snapshotFixture(t, restarted).Plugins; !reflect.DeepEqual(after, before) {
		t.Fatalf("Harness Host restart changed explicit plugin choices: before %+v, after %+v", before, after)
	}
	retained := commandFixture(t, restarted, protocol.Command{Kind: "read_plugin_settings", Data: protocol.CommandReadPluginSettingsPayload{
		Package: plugins.PackageReference(selected),
	}}).Data.(protocol.PluginSettingsState)
	if !reflect.DeepEqual(retained, settings) {
		t.Fatalf("Harness Host restart changed plugin settings: before %+v, after %+v", settings, retained)
	}
	if err := restarted.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		recorded, err := store.PluginRecorded(t.Context(), db, removed.Summary.Name)
		if err == nil && !recorded {
			t.Error("Harness Host restart lost the explicit plugin removal record")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
}
