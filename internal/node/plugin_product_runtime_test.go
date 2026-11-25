package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"path/filepath"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginProductModule string

func (module pluginProductModule) ID() string             { return string(module) }
func (pluginProductModule) ReplacesPlugins() []string     { return nil }
func (pluginProductModule) Catalog() []moduleapi.ToolMeta { return nil }
func (pluginProductModule) ToolsForTurn(context.Context, moduleapi.Host, *store.AgentInvocation) ([]engine.Tool, error) {
	return nil, nil
}

func TestPluginProductUsesAvailableModuleServices(t *testing.T) {
	t.Parallel()
	owner := &Node{productID: "code", ctx: context.Background(), modules: moduleapi.Assembly{Modules: []moduleapi.Module{
		pluginProductModule("files"), pluginProductModule("ssh"), pluginProductModule("databases"),
		pluginProductModule("git"), pluginProductModule("worktrees"), pluginProductModule("terminal"),
	}}}
	runtime := owner.pluginRuntime()
	if runtime.ProductID != "code" || !slices.Equal(runtime.Modules, []string{"files"}) {
		t.Fatalf("uninitialized services were declared available: %+v", runtime)
	}
	foreign := plugins.WithRuntime(t.Context(), plugins.Runtime{ProductID: "harness", Modules: []string{"ssh"}})
	ctx := owner.commandContext(commandJob{scope: foreign})
	if actual := plugins.RuntimeFromContext(ctx); actual.ProductID != "code" || !slices.Equal(actual.Modules, []string{"files"}) {
		t.Fatalf("admitted command inherited caller runtime: %+v", actual)
	}
}

// Over 1 s: starts Nodes and verifies the selected embedded package captures.
func TestPluginProductDefaultsAndBackgroundAdmission(t *testing.T) {
	t.Parallel()
	for _, scenario := range []struct {
		name     string
		defaults []plugins.DefaultPackage
		want     int
	}{
		{"platform nil", nil, len(plugins.BundledDefaults())},
		{"explicit empty", []plugins.DefaultPackage{}, 0},
		{"incompatible default", []plugins.DefaultPackage{{Name: "files", Enabled: true}, {Name: "ssh", Enabled: true}}, 1},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			assembly := moduleapi.Assembly{BundledDefaults: scenario.defaults, Modules: []moduleapi.Module{pluginProductModule("files"), pluginProductModule("ssh")}}
			owner, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{ProductID: "code", Assembly: assembly, MCP: fixtureMCPOptions()})
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() {
				if err := owner.Stop(context.Background()); err != nil {
					t.Error(err)
				}
			})
			snapshot, err := owner.Snapshot(t.Context())
			if err != nil || len(snapshot.Plugins) != scenario.want {
				t.Fatalf("defaults=%+v inventory=%+v err=%v", scenario.defaults, snapshot.Plugins, err)
			}
			foreign := plugins.WithRuntime(t.Context(), plugins.Runtime{ProductID: "harness", Modules: []string{"files", "ssh"}})
			request := protocol.NewRequest(owner.ID(), protocol.Command{Kind: "install_bundled_plugin", Data: protocol.CommandInstallBundledPluginPayload{Name: "ssh"}})
			accepted, err := owner.Dispatch(foreign, owner.ID(), request)
			if err != nil {
				t.Fatal(err)
			}
			select {
			case result := <-accepted.Completion:
				if result.Fault == nil {
					t.Fatal("background installation used the caller's unavailable SSH service")
				}
			case <-t.Context().Done():
				t.Fatal(t.Context().Err())
			}
			if runtime := plugins.RuntimeFromContext(owner.ctx); runtime.ProductID != "code" || !slices.Equal(runtime.Modules, []string{"files"}) {
				t.Fatalf("Node background context lost actual runtime: %+v", runtime)
			}
		})
	}
}

func TestPluginProductPreservesRemovedDefaultInventory(t *testing.T) {
	t.Parallel()
	owner, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{ProductID: "harness", Assembly: moduleapi.Assembly{BundledDefaults: []plugins.DefaultPackage{}}, MCP: fixtureMCPOptions()})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := owner.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	legacy := t.Context()
	info, err := owner.plugins.manager.InstallBundled(legacy, "ssh")
	if err != nil {
		t.Fatal(err)
	}
	var user protocol.User
	if err := owner.profile.Store.Node.Write(legacy, func(db *sql.Conn) error {
		var err error
		user, err = store.LocalUser(legacy, db)
		if err != nil {
			return err
		}
		info, err = store.InstallPlugin(legacy, db, info, 0, true)
		if err != nil {
			return err
		}
		_, err = store.WritePluginValue(legacy, db, plugins.PackageReference(info), "preserved", json.RawMessage(`"keep"`), 0, nil)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := owner.InitializeUserPlugins(t.Context(), user); err != nil {
		t.Fatal(err)
	}
	if err := owner.profile.Store.Node.Read(owner.ctx, func(db *sql.Conn) error {
		current, err := store.ReadPlugin(owner.ctx, db, "ssh")
		if err != nil || current == nil || current.Summary.Digest != info.Summary.Digest || current.Summary.Revision != info.Summary.Revision ||
			current.Summary.SettingsRevision != info.Summary.SettingsRevision || !current.Summary.Enabled || current.Summary.Compatibility.Kind != "unsupported" {
			return fmt.Errorf("removed default changed saved choice: %+v %v", current, err)
		}
		value, err := store.ReadPluginValue(legacy, db, plugins.PackageReference(info), "preserved")
		if err != nil || !value.Present || string(value.Value) != `"keep"` {
			return fmt.Errorf("removed default lost data: %+v %v", value, err)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
