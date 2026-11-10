package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginRuntimeAdmission(t *testing.T) {
	t.Run("Status scheduling", testPluginStatusScheduling)
	t.Run("native slash commands", testPluginMessageCommandOutputs)
	t.Run("callback storage receipt and read-only authority", func(t *testing.T) {
		node := nodeFixture(t)
		source := t.TempDir()
		entry := "dev.sailry.platform/host/main.js"
		for name, content := range map[string]string{
			"plugin.json":                       `{"$schema":"` + plugins.ManifestSchema + `","name":"runtime-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["storage.read","storage.write"],"settings_schema":"dev.sailry.platform/settings.json","settings_page":{"navigation":{"label":"Settings"}},"host":{"entry":"` + entry + `","resources":["` + entry + `"],"handlers":["write","read","readonlyWrite"],"read_only":["read","readonlyWrite"]}}}}`,
			"dev.sailry.platform/settings.json": `{"$schema":"` + plugins.SettingsSchema + `","type":"object","properties":{},"additionalProperties":false}`,
			entry: `import {getValue,setValue,completeRequest} from "sailry/sdk";
export async function write(input){const previous=await getValue("value");const result=await completeRequest(setValue("value",input,previous.revision));if(result.Err)throw new Error(result.Err.message);return getValue("value");}
export async function read(){return getValue("value");}
export async function readonlyWrite(){return completeRequest(setValue("value",0,1));}`,
		} {
			filename := filepath.Join(source, filepath.FromSlash(name))
			if err := os.MkdirAll(filepath.Dir(filename), 0o700); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filename, []byte(content), 0o600); err != nil {
				t.Fatal(err)
			}
		}
		info, err := node.plugins.manager.Install(t.Context(), source, "", "runtime-fixture")
		if err != nil || info.Extension == nil {
			t.Fatalf("callback package: %#v %v", info.Issues, err)
		}
		err = node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
			var err error
			info, err = store.InstallPlugin(t.Context(), db, info, 0, true)
			return err
		})
		if err != nil {
			t.Fatal(err)
		}
		scope := protocol.DefaultPluginContext()
		scope.Package = plugins.PackageReference(info)
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "call_plugin", Data: protocol.CommandCallPluginPayload{Handler: "write", Input: json.RawMessage(`{"captured":true}`)}})
		request.Plugin = &scope
		for range 2 {
			result := dispatchFixture(t, node, node.ID(), request)
			if result.Fault != nil {
				t.Fatal(result.Fault)
			}
			var value struct {
				Revision string          `json:"revision"`
				Present  bool            `json:"present"`
				Value    json.RawMessage `json:"value"`
			}
			raw, _ := json.Marshal(result.Output.Data)
			if json.Unmarshal(raw, &value) != nil || value.Revision != "1" || !value.Present || string(value.Value) != `{"captured":true}` {
				t.Fatalf("callback receipt replay: %s", raw)
			}
		}
		read := protocol.NewRequest(node.ID(), protocol.Command{Kind: "read_plugin_callback", Data: protocol.CommandReadPluginCallbackPayload{Handler: "read", Input: json.RawMessage("null")}})
		read.Plugin = &scope
		if result := dispatchFixture(t, node, node.ID(), read); result.Fault != nil {
			t.Fatal(result.Fault)
		}
		read.Command.Data = protocol.CommandReadPluginCallbackPayload{Handler: "readonlyWrite", Input: json.RawMessage("null")}
		if result := dispatchFixture(t, node, node.ID(), read); result.Fault == nil {
			t.Fatal("read-only callback admitted a durable mutation")
		}
		invocation := protocol.NewRequestID()
		read.Plugin.Invocation = &invocation
		if _, err := node.Dispatch(t.Context(), node.ID(), read); err == nil {
			t.Fatal("controller impersonated an active callback")
		}
		scope.Invocation = nil
		settingsRequest := protocol.NewRequest(node.ID(), protocol.Command{Kind: "save_plugin_settings", Data: protocol.CommandSavePluginSettingsPayload{
			Package: scope.Package, Values: map[string]json.RawMessage{}, Secrets: map[string]protocol.PluginSettingsSecretUpdate{},
		}})
		settingsScope := scope
		settingsScope.Surface.Kind = "settings"
		settingsRequest.Plugin = &settingsScope
		for range 2 {
			if result := dispatchFixture(t, node, node.ID(), settingsRequest); result.Fault != nil {
				t.Fatalf("settings result replay used the new settings authority: %v", result.Fault)
			}
		}
		if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
			current, err := store.ReadPlugin(t.Context(), db, scope.Package.Name)
			if err != nil {
				return err
			}
			_, err = store.SetPluginEnabled(t.Context(), db, current.Summary.Name, current.Summary.Revision, false)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if result := dispatchFixture(t, node, node.ID(), request); result.Fault != nil {
			t.Fatalf("completed callback replay required the disabled package: %v", result.Fault)
		}
		changed := request
		changed.Command.Data = protocol.CommandCallPluginPayload{Handler: "write", Input: json.RawMessage(`{"changed":true}`)}
		if _, err := node.Dispatch(t.Context(), node.ID(), changed); !pluginMessageFault(err, protocol.ErrorCodeConflict) {
			t.Fatalf("completed callback accepted a different body: %v", err)
		}
		changed.ID = protocol.NewRequestID()
		if _, err := node.Dispatch(t.Context(), node.ID(), changed); err == nil {
			t.Fatal("a new callback bypassed current package authority")
		}
		if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
			current, err := store.ReadPlugin(t.Context(), db, scope.Package.Name)
			if err != nil {
				return err
			}
			_, err = store.RemovePlugin(t.Context(), db, current.Summary.Name, current.Summary.Revision)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if result := dispatchFixture(t, node, node.ID(), request); result.Fault != nil {
			t.Fatalf("completed callback replay required the removed package: %v", result.Fault)
		}
	})
	t.Run("bootstrap removal and settings survive restart", func(t *testing.T) {
		node := nodeFixture(t)
		initial := snapshotFixture(t, node).Plugins
		if len(initial) != len(plugins.BundledDefaults()) {
			t.Fatalf("bundled inventory: %d", len(initial))
		}
		selected := initial[0]
		commandFixture(t, node, protocol.Command{Kind: "remove_plugin", Data: protocol.CommandRemovePluginPayload{Name: selected.Name, ExpectedRevision: selected.Revision}})
		path := node.profile.Ownership.Path
		if err := node.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
		reopened, err := startFixtureNode(t.Context(), path)
		if err != nil {
			t.Fatal(err)
		}
		defer reopened.Stop(context.Background())
		current := snapshotFixture(t, reopened).Plugins
		if len(current) != len(initial)-1 || slices.ContainsFunc(current, func(value protocol.PluginSummary) bool { return value.Name == selected.Name }) {
			t.Fatal("bootstrap resurrected a removed package or changed another package")
		}
		for _, value := range current {
			index := slices.IndexFunc(initial, func(previous protocol.PluginSummary) bool { return previous.Name == value.Name })
			if index < 0 || value.Digest != initial[index].Digest || value.SettingsRevision != initial[index].SettingsRevision || value.Enabled != initial[index].Enabled {
				t.Fatalf("bootstrap changed preserved inventory: %#v", value)
			}
		}
	})
}
