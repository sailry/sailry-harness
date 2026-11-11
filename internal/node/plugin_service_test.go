package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/shirou/gopsutil/v4/process"
)

// Over 1 s: each Node start installs and verifies the bundled packages.
func TestPluginRuntimeAdmission(t *testing.T) {
	t.Run("Status scheduling", testPluginStatusScheduling)
	t.Run("native slash commands", testPluginMessageCommandOutputs)
	t.Run("ssh resources", testSSHResourceRules)
	t.Run("connection tools", testConnectionTools)
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
		// The installed package's settings scope, for the controller-only checks.
		var scope protocol.PluginContext
		t.Run("controller reads a saved secret slot", func(t *testing.T) {
			source := t.TempDir()
			for name, content := range map[string]string{
				"plugin.json":                       `{"$schema":"` + plugins.ManifestSchema + `","name":"secret-fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["storage.read"],"settings_schema":"dev.sailry.platform/settings.json","settings_page":{"navigation":{"label":"Settings"}}}}}`,
				"dev.sailry.platform/settings.json": `{"$schema":"` + plugins.SettingsSchema + `","type":"object","properties":{"label":{"type":"string"},"token":{"type":"string","x-sailry-secret":{"origin":"https://api.example.test","header":"Authorization"}}},"additionalProperties":false}`,
			} {
				filename := filepath.Join(source, filepath.FromSlash(name))
				if err := os.MkdirAll(filepath.Dir(filename), 0o700); err != nil {
					t.Fatal(err)
				}
				if err := os.WriteFile(filename, []byte(content), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			info, err := reopened.plugins.manager.Install(t.Context(), source, "", "secret-fixture")
			if err != nil || info.Settings == nil {
				t.Fatalf("secret package: %#v %v", info.Issues, err)
			}
			if err := reopened.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
				var err error
				info, err = store.InstallPlugin(t.Context(), db, info, 0, true)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			read := func(reference protocol.PluginReference, field string, scope *protocol.PluginContext) (string, *protocol.Fault) {
				request := protocol.NewRequest(reopened.ID(), protocol.Command{Kind: "read_plugin_secret", Data: protocol.CommandReadPluginSecretPayload{Package: reference, Field: field}})
				request.Plugin = scope
				accepted, err := reopened.Dispatch(t.Context(), reopened.ID(), request)
				if err != nil {
					return "", nodeFault(err).(*protocol.Fault)
				}
				result := <-accepted.Completion
				if result.Fault != nil {
					return "", result.Fault
				}
				if accepted.Receipt.Durable || result.Output.Kind != "plugin_secret" {
					t.Fatalf("secret read: durable=%v output=%s", accepted.Receipt.Durable, result.Output.Kind)
				}
				raw, _ := json.Marshal(result.Output.Data)
				return string(raw), nil
			}
			reference := plugins.PackageReference(info)
			if value, fault := read(reference, "token", nil); fault != nil || value != "null" {
				t.Fatalf("unsaved secret slot: %s %v", value, fault)
			}
			saved := commandFixture(t, reopened, protocol.Command{Kind: "save_plugin_settings", Data: protocol.CommandSavePluginSettingsPayload{
				Package: reference, Values: map[string]json.RawMessage{},
				Secrets: map[string]protocol.PluginSettingsSecretUpdate{"token": {Kind: "replace", Data: protocol.NewSecret("fixture-token")}},
			}})
			state, ok := dispatchPayload[protocol.PluginSettingsState](saved.Data)
			if !ok || !slices.Contains(state.Configured, "token") {
				t.Fatalf("saved settings: %#v", saved.Data)
			}
			if _, fault := read(reference, "token", nil); fault == nil || fault.Code != protocol.ErrorCodeRevisionConflict {
				t.Fatalf("stale package reference read a secret: %v", fault)
			}
			if value, fault := read(state.Package, "token", nil); fault != nil || value != `"fixture-token"` {
				t.Fatalf("saved secret slot: %s %v", value, fault)
			}
			for _, field := range []string{"label", "missing"} {
				if _, fault := read(state.Package, field, nil); fault == nil || fault.Code != protocol.ErrorCodeInvalidRequest {
					t.Fatalf("undeclared secret slot %q: %v", field, fault)
				}
			}
			scope = protocol.DefaultPluginContext()
			scope.Package = state.Package
			scope.Surface.Kind = "settings"
			if _, fault := read(state.Package, "token", &scope); fault == nil || fault.Code != protocol.ErrorCodePermissionDenied {
				t.Fatalf("plugin action read credential contents: %v", fault)
			}
		})
		t.Run("Host process signal keeps the listed identity", func(t *testing.T) {
			signal := func(pid uint32, started uint64, scope *protocol.PluginContext) *protocol.Fault {
				request := protocol.NewRequest(reopened.ID(), protocol.Command{Kind: "stop_host_process", Data: protocol.CommandStopHostProcessPayload{Pid: pid, StartedAtSecs: started}})
				request.Plugin = scope
				accepted, err := reopened.Dispatch(t.Context(), reopened.ID(), request)
				if err != nil {
					return nodeFault(err).(*protocol.Fault)
				}
				if !accepted.Receipt.Durable {
					t.Fatal("process signal was not admitted durably")
				}
				return (<-accepted.Completion).Fault
			}
			for name, row := range map[string]struct {
				pid  uint32
				code protocol.ErrorCode
			}{
				"system":         {1, protocol.ErrorCodePermissionDenied},
				"execution Node": {uint32(os.Getpid()), protocol.ErrorCodePermissionDenied},
				"reused ID":      {uint32(os.Getppid()), protocol.ErrorCodeConflict},
			} {
				if fault := signal(row.pid, 0, nil); fault == nil || fault.Code != row.code {
					t.Fatalf("%s: %v", name, fault)
				}
			}
			if fault := signal(uint32(os.Getppid()), 0, &scope); fault == nil || fault.Code != protocol.ErrorCodePermissionDenied {
				t.Fatalf("plugin action signalled a Host process: %v", fault)
			}
			sleep, err := exec.LookPath("sleep")
			if err != nil || runtime.GOOS == "windows" {
				return // Windows has no termination request.
			}
			child := exec.Command(sleep, "60")
			if err := child.Start(); err != nil {
				t.Fatal(err)
			}
			defer child.Process.Kill()
			listed, err := process.NewProcess(int32(child.Process.Pid))
			if err != nil {
				t.Fatal(err)
			}
			created, err := listed.CreateTime()
			if err != nil {
				t.Fatal(err)
			}
			if fault := signal(uint32(child.Process.Pid), uint64(created/1000)+1, nil); fault == nil || fault.Code != protocol.ErrorCodeConflict {
				t.Fatalf("another start time signalled the process: %v", fault)
			}
			if fault := signal(uint32(child.Process.Pid), uint64(created/1000), nil); fault != nil {
				t.Fatal(fault)
			}
			var exit *exec.ExitError
			if err := child.Wait(); !errors.As(err, &exit) || exit.ExitCode() != -1 {
				t.Fatalf("listed process was not terminated: %v", err)
			}
		})
		t.Run("connector CLI keeps the Node's search path and home", func(t *testing.T) {
			if runtime.GOOS == "windows" {
				t.Skip("the fixture commands are shell scripts")
			}
			declared := t.TempDir()
			for _, name := range []string{"sh", "sailry-fixture-cli"} {
				if err := os.WriteFile(filepath.Join(declared, name), []byte("#!/bin/sh\nprintf declared\n"), 0o700); err != nil {
					t.Fatal(err)
				}
			}
			home := ""
			for _, entry := range reopened.environment.TerminalValues() {
				if value, found := strings.CutPrefix(entry, "HOME="); found {
					home = value
				}
			}
			declaration := protocol.PluginConnectorAuthLocalCliPayload{Command: "sh", TimeoutMs: 5_000, MaxOutput: 1024,
				Env: map[string]string{"PATH": declared, "HOME": declared, "FIXTURE": "kept"}}
			output, err := reopened.runConnectorCLI(t.Context(), declaration, []string{"-c", `printf '%s|%s' "$HOME" "$FIXTURE"`})
			if err != nil || string(output) != home+"|kept" {
				t.Fatalf("declared search path or home reached the command: %q, %v", output, err)
			}
			declaration.Command = "sailry-fixture-cli"
			if output, err := reopened.runConnectorCLI(t.Context(), declaration, nil); err == nil {
				t.Fatalf("a command on the declared search path alone ran: %q", output)
			}
		})
	})
}
