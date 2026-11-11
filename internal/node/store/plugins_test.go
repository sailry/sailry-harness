package store

import (
	"bytes"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func pluginTestInfo(name string, digest byte) protocol.PluginInfo {
	info := protocol.DefaultPluginInfo()
	info.Summary.Name = name
	info.Summary.Digest = strings.Repeat(string(digest), 64)
	version := "0.1.0"
	info.Summary.Version = &version
	info.Origin = &protocol.PluginOrigin{Kind: "bundled"}
	return info
}

// Over 1 s only beside the package's other parallel tests: each opens its own synced store.
func TestPluginInventoryAndSettings(t *testing.T) {
	t.Parallel()
	t.Run("controller authority", testPluginControllerAuthority)
	t.Run("session authority", testPluginSessionAuthority)
	t.Run("captured Plan actions", testPluginPlanActions)
	t.Run("scoped Status contracts", testPluginStatusContracts)
	t.Run("database profiles", testDatabaseProfiles)
	t.Run("connection workspaces", testConnectionWorkspaces)
	t.Run("scoped Activity reads", testPluginActivityReads)
	t.Run("native slash command resolution", testPluginMessageCommandResolution)
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		info := pluginTestInfo("fixture", 'a')
		schema, err := plugins.ValidateSettings([]byte(`{"$schema":"` + plugins.SettingsSchema + `","type":"object","additionalProperties":false,"properties":{"enabled":{"type":"boolean","default":true},"token":{"type":"string","x-sailry-secret":{"origin":"https://api.example.test","header":"Authorization"}}},"required":["enabled","token"]}`))
		if err != nil {
			return err
		}
		info.Settings = &schema
		info, err = InstallPlugin(ctx, db, info, 0, true)
		if err != nil {
			return err
		}
		initial := plugins.PackageReference(info)
		state, err := ReadPluginSettings(ctx, db, initial)
		if err != nil || state.Ready || string(state.Values["enabled"]) != "true" {
			return errors.New("settings defaults or required secret state changed")
		}
		secret := protocol.NewSecret("private-value")
		input := protocol.CommandSavePluginSettingsPayload{Package: initial, Values: map[string]json.RawMessage{"enabled": json.RawMessage("true")},
			Secrets: map[string]protocol.PluginSettingsSecretUpdate{"token": {Kind: "replace", Data: secret}}}
		state, err = SavePluginSettings(ctx, db, input)
		if err != nil {
			return err
		}
		captured := state.Package
		if !state.Ready || len(state.Configured) != 1 {
			return errors.New("configured secret was not projected")
		}
		public, err := json.Marshal(state)
		if err != nil || bytes.Contains(public, []byte(secret.Expose())) {
			return errors.New("private configuration leaked into a public projection")
		}
		input.Package, input.Secrets = state.Package, map[string]protocol.PluginSettingsSecretUpdate{"token": {Kind: "clear"}}
		state, err = SavePluginSettings(ctx, db, input)
		if err != nil {
			return err
		}
		if state.Ready || len(state.Configured) != 0 {
			return errors.New("cleared secret remained ready")
		}
		resolved, err := ResolvePluginSettings(ctx, db, captured)
		if err != nil {
			return err
		}
		if len(resolved.Secrets) != 0 {
			return errors.New("historical capture restored a cleared secret")
		}
		if _, err := SavePluginSettings(ctx, db, input); err == nil {
			return errors.New("stale settings capture was accepted")
		}
		current, err := ReadPlugin(ctx, db, "fixture")
		if err != nil || current == nil {
			return errors.New("installed inventory disappeared")
		}
		disabled, err := SetPluginEnabled(ctx, db, "fixture", current.Summary.Revision, false)
		if err != nil {
			return err
		}
		replacement := pluginTestInfo("fixture", 'b')
		replacement.Settings = &schema
		next := "0.1.1"
		replacement.Summary.Version = &next
		replacement, err = InstallPlugin(ctx, db, replacement, disabled.Summary.Revision, true)
		if err != nil || replacement.Summary.Enabled || replacement.Summary.SettingsRevision != state.Package.SettingsRevision {
			return errors.New("package update lost configuration or enable choice")
		}
		removed, err := RemovePlugin(ctx, db, "fixture", replacement.Summary.Revision)
		if err != nil {
			return err
		}
		recorded, err := PluginRecorded(ctx, db, "fixture")
		if err != nil || !recorded {
			return errors.New("removed package lost its inventory tombstone")
		}
		listed, err := ReadPlugins(ctx, db)
		if err != nil || len(listed) != 0 {
			return errors.New("removed package remained in installed inventory")
		}
		if _, err := ReadPluginVersion(ctx, db, captured); err != nil {
			return err
		}
		reinstalled, err := InstallPlugin(ctx, db, replacement, removed.Revision, true)
		if err != nil || reinstalled.Summary.Revision <= removed.Revision || reinstalled.Summary.SettingsRevision != state.Package.SettingsRevision {
			return errors.New("reinstall reused configuration or inventory revisions")
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestPluginPrivateValuesAndSearch(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	var reference protocol.PluginReference
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		info := pluginTestInfo("storage-fixture", 'a')
		key := "record"
		extension := protocol.DefaultPluginExtension()
		extension.ApiVersion = "v1"
		extension.Storage = &protocol.PluginStorageDeclaration{Collections: []protocol.PluginStorageCollection{{
			Scope: protocol.PluginStorageScopeNode, Key: &key,
			Schema: json.RawMessage(`{"type":"object","properties":{"title":{"type":"string"},"count":{"type":"integer","default":1}}}`),
		}}}
		info.Extension = &extension
		info, err := InstallPlugin(ctx, db, info, 0, true)
		if err != nil {
			return err
		}
		reference = plugins.PackageReference(info)
		index := protocol.PluginStorageIndex{Fields: [2]string{"Review changes", "资料检索"}, Tags: []string{"review"}, Order: 3}
		first, err := WritePluginValue(ctx, db, reference, "record", json.RawMessage(`{"title":"old","unknown":{"retained":true}}`), 0, &index)
		if err != nil {
			return err
		}
		next, err := WritePluginValue(ctx, db, reference, "record", json.RawMessage(`{"title":"new"}`), first.Revision, &index)
		if err != nil || !bytes.Contains(next.Value, []byte(`"unknown"`)) || !bytes.Contains(next.Value, []byte(`"count":1`)) {
			return fmt.Errorf("schema normalization lost data: %s %v", next.Value, err)
		}
		if _, err := WritePluginValue(ctx, db, reference, "record", json.RawMessage("{}"), first.Revision, nil); err == nil {
			return errors.New("stale plugin revision was accepted")
		}
		query := protocol.PluginStorageSearch{Terms: []string{"review"}, Weights: [2]uint16{2, 1}, All: []string{"review"}, Limit: 10}
		results, err := SearchPluginValues(ctx, db, reference, query)
		if err != nil || len(results.Entries) != 1 || results.Entries[0].Key != "record" || results.Mode != protocol.KnowledgeModeKeyword {
			return fmt.Errorf("indexed KV write did not appear atomically: %#v %v", results, err)
		}
		query.Terms = []string{"资料"}
		results, err = SearchPluginValues(ctx, db, reference, query)
		if err != nil || len(results.Entries) != 1 {
			return fmt.Errorf("CJK plugin search failed: %#v %v", results, err)
		}
		removed, err := RemovePluginValue(ctx, db, reference, "record", next.Revision)
		if err != nil || removed.Present || removed.Revision <= next.Revision {
			return errors.New("plugin value tombstone lost its revision")
		}
		results, err = SearchPluginValues(ctx, db, reference, query)
		if err != nil || len(results.Entries) != 0 {
			return errors.New("deleted value remained in its search corpus")
		}
		absent, err := RemovePluginValue(ctx, db, reference, "never-seen", 0)
		if err != nil || absent.Revision != 0 {
			return errors.New("deleting a missing key created a tombstone")
		}
		for _, key := range []string{"prefix/a", "prefix/b", "prefix/c"} {
			if _, err := WritePluginValue(ctx, db, reference, key, json.RawMessage("1"), 0, nil); err != nil {
				return err
			}
		}
		page, err := ListPluginKeys(ctx, db, reference, "prefix/", nil, 2)
		if err != nil || len(page.Keys) != 2 || page.After == nil || *page.After != "prefix/b" {
			return errors.New("plugin key page lost its exclusive continuation")
		}
		page, err = ListPluginKeys(ctx, db, reference, "prefix/", page.After, 2)
		if err != nil || len(page.Keys) != 1 || page.Keys[0] != "prefix/c" || page.After != nil {
			return errors.New("plugin key continuation returned duplicates")
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	sentinel := errors.New("rollback")
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := WritePluginValue(ctx, db, reference, "rollback", json.RawMessage("true"), 0, nil); err != nil {
			return err
		}
		return sentinel
	}); !errors.Is(err, sentinel) {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		entry, err := ReadPluginValue(ctx, db, reference, "rollback")
		if err != nil || entry.Present || entry.Revision != 0 {
			return errors.New("failed writer operation left private KV state")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestPluginConversationCheckpoints(t *testing.T) {
	t.Parallel()
	storage, _, source, turn, _ := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		info, err := InstallPlugin(ctx, db, pluginTestInfo("conversation-fixture", 'a'), 0, true)
		if err != nil {
			return err
		}
		reference := plugins.PackageReference(info)
		entry, err := WritePluginConversationValue(ctx, db, reference, source.ID, &turn, "checkpoint", json.RawMessage(`{"step":1}`), 0)
		if err != nil || entry.Restored || !entry.Present {
			return fmt.Errorf("conversation checkpoint was not stored: %v", err)
		}
		target := protocol.NewSessionID()
		if _, err := db.ExecContext(ctx, "INSERT INTO sessions(id,revision,history_revision) VALUES(?1,1,1)", target.String()); err != nil {
			return err
		}
		if err := ForkPluginConversation(ctx, db, source.ID, target, turn, false); err != nil {
			return err
		}
		inherited, err := ReadPluginConversationValue(ctx, db, reference, target, "checkpoint")
		if err != nil || !inherited.Present || !inherited.Restored || !bytes.Equal(inherited.Value, entry.Value) {
			return errors.New("fork did not select the immutable source checkpoint")
		}
		if _, err := WritePluginConversationValue(ctx, db, reference, target, nil, "checkpoint", json.RawMessage(`{"step":2}`), inherited.Revision); err != nil {
			return err
		}
		original, err := ReadPluginConversationValue(ctx, db, reference, source.ID, "checkpoint")
		if err != nil || !bytes.Equal(original.Value, entry.Value) {
			return errors.New("branch write changed its source checkpoint")
		}
		if err := RewindPluginConversation(ctx, db, source.ID, nil); err != nil {
			return err
		}
		rewound, err := ReadPluginConversationValue(ctx, db, reference, source.ID, "checkpoint")
		if err != nil || rewound.Present || !rewound.Restored || rewound.Revision <= original.Revision {
			return errors.New("rewind did not revoke the old checkpoint revision")
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
