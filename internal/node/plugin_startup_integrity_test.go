package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"testing"
	"time"

	hostassembly "github.com/sailry/sailry-harness/internal/assembly/host"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestStartupIsolatesUnavailablePlugin(t *testing.T) {
	t.Parallel()
	for _, damage := range []string{"missing", "changed"} {
		t.Run(damage, func(t *testing.T) {
			ctx := t.Context()
			owner := nodeFixture(t)
			healthy := installPluginFixture(t, owner, "task-notes")
			profile := owner.profile.Ownership.Path
			session := sessionFixture(t, owner)
			source := t.TempDir()
			const name = "local-startup-fixture"
			const entry = "dev.sailry.platform/host/main.js"
			manifest := `{"$schema":"` + plugins.ManifestSchema + `","name":"` + name + `","version":"1.0.0","extensions":{"dev.sailry.platform":{"api_version":"v1","host":{"entry":"` + entry + `","resources":["` + entry + `"],"handlers":["read"],"read_only":["read"]}}}}`
			if err := os.WriteFile(filepath.Join(source, "plugin.json"), []byte(manifest), 0o600); err != nil {
				t.Fatal(err)
			}
			if err := os.MkdirAll(filepath.Dir(filepath.Join(source, entry)), 0o700); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(source, entry), []byte(`export async function read(){return "fixture";}`), 0o600); err != nil {
				t.Fatal(err)
			}
			info, err := owner.plugins.manager.Install(ctx, source, "", name)
			if err != nil {
				t.Fatal(err)
			}
			if err := owner.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
				info, err = store.InstallPlugin(ctx, db, info, 0, true)
				if err != nil {
					return err
				}
				_, err = store.WritePluginValue(ctx, db, plugins.PackageReference(info), "value", json.RawMessage(`"keep"`), 0, nil)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			reference := plugins.PackageReference(info)
			call := func(owner *Node) protocol.Request {
				request := protocol.NewRequest(owner.ID(), protocol.Command{Kind: "read_plugin_callback", Data: protocol.CommandReadPluginCallbackPayload{Handler: "read", Input: json.RawMessage(`null`)}})
				scope := protocol.DefaultPluginContext()
				scope.Package = reference
				request.Plugin = &scope
				return request
			}
			if response := dispatchFixture(t, owner, owner.ID(), call(owner)); response.Fault != nil {
				t.Fatalf("fixture callback: %v", response.Fault)
			}
			queue := protocol.Command{Kind: "queue_turn", Data: protocol.CommandQueueTurnPayload{
				Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "Keep this input"},
			}}
			original := commandFixture(t, owner, queue).Data.(protocol.QueuedTurn)
			if !slices.Contains(original.Plugins, reference) {
				t.Fatal("healthy fixture was not captured")
			}
			if err := owner.Stop(ctx); err != nil {
				t.Fatal(err)
			}
			path := filepath.Join(profile, "plugins", "packages", info.Summary.Digest)
			if damage == "missing" {
				err = os.RemoveAll(path)
			} else {
				err = os.WriteFile(filepath.Join(path, entry), []byte(`export async function read(){return "changed";}`), 0o600)
			}
			if err != nil {
				t.Fatal(err)
			}
			restarted, err := startFixtureNode(ctx, profile)
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() {
				if err := restarted.Stop(context.Background()); err != nil {
					t.Error(err)
				}
			})
			if _, err := restarted.plugins.manager.CapturedInfo(ctx, reference); err == nil {
				t.Fatal("unavailable package passed capture")
			}
			if response := dispatchFixture(t, restarted, restarted.ID(), call(restarted)); response.Fault == nil || response.Fault.Code != protocol.ErrorCodeNotConfigured {
				t.Fatalf("unavailable callback: %+v", response)
			}
			turn := commandFixture(t, restarted, queue).Data.(protocol.QueuedTurn)
			if slices.Contains(turn.Plugins, reference) || !slices.Contains(turn.Plugins, plugins.PackageReference(healthy)) {
				t.Fatal("new turn did not isolate the broken package")
			}
			readQueued := protocol.Command{Kind: "read_queued_turn", Data: protocol.CommandReadQueuedTurnPayload{Turn: original.ID}}
			queued := commandFixture(t, restarted, readQueued).Data.(protocol.ConversationQueuedMessage)
			commandFixture(t, restarted, protocol.Command{Kind: "send_queued_turn", Data: protocol.CommandSendQueuedTurnPayload{
				Turn: original.ID, ExpectedRevision: queued.Revision,
			}})
			waitForUnavailablePluginTurn(t, restarted, original.ID, name)
			retained := commandFixture(t, restarted, readQueued).Data.(protocol.ConversationQueuedMessage)
			if retained.Message.Text != queued.Message.Text || !slices.Equal(retained.Turn.Plugins, original.Plugins) {
				t.Fatal("failed turn lost its queue input or rewrote its package capture")
			}
			if err := restarted.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				current, err := store.ReadPlugin(ctx, db, name)
				if err != nil {
					return err
				}
				if current == nil || !reflect.DeepEqual(current.Summary, info.Summary) {
					return fmt.Errorf("startup changed the installed package: %+v", current)
				}
				value, err := store.ReadPluginValue(ctx, db, reference, "value")
				if err != nil || !value.Present || string(value.Value) != `"keep"` {
					return fmt.Errorf("startup lost plugin data: %+v, %v", value, err)
				}
				return nil
			}); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func waitForUnavailablePluginTurn(t *testing.T, node *Node, turn protocol.TurnID, name string) {
	t.Helper()
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	tick := time.NewTicker(10 * time.Millisecond)
	defer tick.Stop()
	for {
		var run protocol.ConversationRun
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			run, err = store.ReadAgentRun(ctx, db, turn)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if run.Status != protocol.ConversationStatusQueued {
			if run.StartedMs != nil || run.Status != protocol.ConversationStatusFailed || run.Error == nil || run.Error.Code != protocol.ErrorCodeNotConfigured || run.Error.Message != "plugin package is unavailable: "+name {
				t.Fatalf("unavailable package turn was claimed: %+v", run)
			}
			return
		}
		select {
		case <-ctx.Done():
			t.Fatalf("turn was not settled: %v", ctx.Err())
		case <-tick.C:
		}
	}
}

func TestStartupRestoresExactBundledPackages(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	profile := filepath.Join(t.TempDir(), "profile")
	const missing, changed, removed, olderName = "goals", "reminders", "brief", "files"
	assembly := hostassembly.Harness()
	assembly.BundledDefaults = []plugins.DefaultPackage{
		{Name: missing, Enabled: true},
		{Name: changed, Enabled: true},
		{Name: removed, Enabled: true},
		{Name: olderName, Enabled: true},
	}
	start := func() *Node {
		assembly.BundledPackages = pluginFixtureBundles()
		owner, err := StartWithOptions(ctx, profile, Options{Assembly: assembly, MCP: fixtureMCPOptions()})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() {
			if err := owner.Stop(context.Background()); err != nil {
				t.Error(err)
			}
		})
		return owner
	}
	owner := start()
	if !slices.Contains(owner.pluginRuntime().Modules, "files") {
		t.Fatal("Harness did not assemble its default files module")
	}
	olderSource := t.TempDir()
	if err := os.WriteFile(filepath.Join(olderSource, "plugin.json"), []byte(`{"$schema":"`+plugins.ManifestSchema+`","name":"`+olderName+`","version":"0.0.0"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	older, err := owner.plugins.manager.Install(ctx, olderSource, "", olderName)
	if err != nil {
		t.Fatal(err)
	}
	older.Origin = &protocol.PluginOrigin{Kind: "bundled"}
	var unavailable, damaged protocol.PluginSummary
	var configured protocol.PluginSettingsState
	if err := owner.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		for _, name := range []string{missing, changed, removed, olderName} {
			info, err := store.ReadPlugin(owner.pluginContext(ctx), db, name)
			if err != nil || info == nil {
				return fmt.Errorf("read bundled %s: %v", name, err)
			}
			if !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" {
				return fmt.Errorf("Harness default %s is not enabled and compatible: %+v", name, info.Summary)
			}
			switch name {
			case missing:
				configured, err = store.SavePluginSettings(ctx, db, protocol.CommandSavePluginSettingsPayload{
					Package: plugins.PackageReference(*info), Values: map[string]json.RawMessage{
						"turn_limit": json.RawMessage(`17`), "token_budget": json.RawMessage(`3`), "time_limit": json.RawMessage(`5`),
					},
				})
				if err != nil {
					return err
				}
				info, err = store.ReadPlugin(ctx, db, name)
				if err != nil || info == nil {
					return fmt.Errorf("read configured plugin: %v", err)
				}
				disabled, err := store.SetPluginEnabled(ctx, db, name, info.Summary.Revision, false)
				if err != nil {
					return err
				}
				pinned, err := store.SetPluginPinned(ctx, db, name, disabled.Summary.Revision, true)
				if err != nil {
					return err
				}
				unavailable = pinned.Summary
			case changed:
				damaged = info.Summary
			case removed:
				if _, err := store.RemovePlugin(ctx, db, name, info.Summary.Revision); err != nil {
					return err
				}
			case olderName:
				if !plugins.IsNewer(older.Summary.Version, info.Summary.Version) {
					return fmt.Errorf("fixture must be older than the embedded package: %+v", info.Summary)
				}
				installed, err := store.InstallPlugin(ctx, db, older, info.Summary.Revision, true)
				if err != nil {
					return err
				}
				older, err = store.SetPluginPinned(ctx, db, name, installed.Summary.Revision, true)
				if err != nil {
					return err
				}
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if err := owner.Stop(ctx); err != nil {
		t.Fatal(err)
	}
	packages := filepath.Join(profile, "plugins", "packages")
	for _, digest := range []string{unavailable.Digest, older.Summary.Digest} {
		if err := os.RemoveAll(filepath.Join(packages, digest)); err != nil {
			t.Fatal(err)
		}
	}
	const marker = "damaged-copy"
	if err := os.WriteFile(filepath.Join(packages, damaged.Digest, marker), []byte(marker), 0o600); err != nil {
		t.Fatal(err)
	}
	assembly = hostassembly.Harness()
	restarted := start()
	for _, expected := range []protocol.PluginSummary{unavailable, damaged} {
		if err := restarted.plugins.manager.VerifyPackage(ctx, protocol.PluginReference{Name: expected.Name, Digest: expected.Digest}); err != nil {
			t.Fatalf("restored %s failed verification: %v", expected.Name, err)
		}
	}
	if err := restarted.plugins.manager.VerifyPackage(ctx, plugins.PackageReference(older)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("unavailable pinned version was replaced: %v", err)
	}
	quarantined, err := filepath.Glob(filepath.Join(packages, ".quarantine-"+damaged.Digest+"-*", marker))
	if err != nil || len(quarantined) != 1 {
		t.Fatalf("damaged copy was not quarantined: %v, %v", quarantined, err)
	}
	if content, err := os.ReadFile(quarantined[0]); err != nil || string(content) != marker {
		t.Fatalf("quarantine lost the damaged copy: %q, %v", content, err)
	}
	if err := restarted.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		for _, expected := range []protocol.PluginSummary{unavailable, damaged, older.Summary} {
			current, err := store.ReadPlugin(restarted.pluginContext(ctx), db, expected.Name)
			if err != nil || current == nil || !reflect.DeepEqual(current.Summary, expected) {
				return fmt.Errorf("recovery changed inventory for %s: %+v, %v", expected.Name, current, err)
			}
		}
		current, err := store.ReadPluginSettings(ctx, db, configured.Package)
		if err != nil || !reflect.DeepEqual(current, configured) {
			return fmt.Errorf("recovery changed plugin settings: %+v, %v", current, err)
		}
		recorded, err := store.PluginRecorded(ctx, db, removed)
		if err != nil || !recorded {
			return fmt.Errorf("recovery lost removal record: %v", err)
		}
		if info, err := store.ReadPlugin(ctx, db, removed); err != nil || info != nil {
			return errors.New("recovery reinstalled the removed plugin")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
