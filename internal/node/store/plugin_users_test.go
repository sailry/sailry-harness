package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginUserNamespaces(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		local, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		controller := protocol.NodeID{44}
		if err := PairPeerIdentity(ctx, db, node, controller, link.DirectionController, true); err != nil {
			return err
		}
		second, err := CallerUser(ctx, db, node, controller)
		if err != nil {
			return err
		}
		if second.Controller == nil || *second.Controller != controller {
			return fmt.Errorf("fixture did not create a paired controller user")
		}
		contexts := []context.Context{WithUser(ctx, local), WithUser(ctx, second)}
		refs := make([]protocol.PluginReference, 2)
		info := pluginTestInfo("isolated-plugin", 'a')
		schema, err := plugins.ValidateSettings([]byte(`{"$schema":"` + plugins.SettingsSchema + `","type":"object","additionalProperties":false,"properties":{"token":{"type":"string","x-sailry-secret":{"origin":"https://api.example.test","header":"Authorization"}}}}`))
		if err != nil {
			return err
		}
		info.Settings = &schema
		prefix := "shared/"
		extension := protocol.DefaultPluginExtension()
		extension.ApiVersion = "v1"
		extension.Actions = []protocol.PluginAction{protocol.PluginActionReadStorage, protocol.PluginActionWriteStorage}
		extension.Storage = &protocol.PluginStorageDeclaration{Collections: []protocol.PluginStorageCollection{{Scope: protocol.PluginStorageScopeProject, Prefix: &prefix, Schema: json.RawMessage(`{"type":"object","properties":{"owner":{"type":"integer"}}}`)}}}
		info.Extension = &extension
		for index, owner := range contexts {
			installed, err := InstallPlugin(owner, db, info, 0, true)
			if err != nil {
				return err
			}
			saved, err := SavePluginSettings(owner, db, protocol.CommandSavePluginSettingsPayload{Package: plugins.PackageReference(installed), Values: map[string]json.RawMessage{}, Secrets: map[string]protocol.PluginSettingsSecretUpdate{"token": {Kind: "replace", Data: protocol.NewSecret(fmt.Sprint(index))}}})
			if err != nil {
				return err
			}
			refs[index] = saved.Package
			indexed := &protocol.PluginStorageIndex{Fields: [2]string{"isolated record", ""}, Tags: []string{fmt.Sprint(index)}}
			if _, err := WritePluginValue(owner, db, refs[index], "record", json.RawMessage(fmt.Sprintf(`{"owner":%d}`, index)), 0, indexed); err != nil {
				return err
			}
		}
		var replicas int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM plugin_space_values WHERE space LIKE 'controller:%'").Scan(&replicas); err != nil {
			return err
		}
		if replicas != 0 {
			return fmt.Errorf("a paired user wrote to controller replica storage")
		}
		if refs[0] != refs[1] {
			return fmt.Errorf("fixture did not create colliding user revision captures")
		}
		for index, owner := range contexts {
			settings, err := ResolvePluginSettings(owner, db, refs[index])
			if err != nil {
				return err
			}
			if settings.Secrets["token"].Expose() != fmt.Sprint(index) {
				return fmt.Errorf("settings crossed user boundary")
			}
			value, err := ReadPluginValue(owner, db, refs[index], "record")
			if err != nil {
				return err
			}
			if string(value.Value) != fmt.Sprintf(`{"owner":%d}`, index) {
				return fmt.Errorf("personal values crossed user boundary: %s", value.Value)
			}
			query := protocol.DefaultPluginStorageSearch()
			query.Terms, query.Limit, query.Weights = []string{"isolated"}, 10, [2]uint16{2, 1}
			page, err := SearchPluginValues(owner, db, refs[index], query)
			if err != nil {
				return err
			}
			if len(page.Entries) != 1 || string(page.Entries[0].Value) != string(value.Value) {
				return fmt.Errorf("search crossed user boundary: %#v", page)
			}
		}
		captured := WithCapturedUser(contexts[0], local.ID)
		settings, err := ResolvePluginSettings(captured, db, refs[0])
		if err != nil || settings.Secrets["token"].Expose() != "0" {
			return fmt.Errorf("admitted storage principal was lost: %v", err)
		}
		record := ConnectorRecord{ID: protocol.PluginConnectorID(protocol.NewRequestID()), Plugin: info.Summary.Name, Connector: "fixture", Revision: 1, AuthRevision: 1, Method: protocol.PluginConnectorKindSecret, Enabled: true, State: protocol.PluginConnectorStateNotConnected}
		if err := saveConnectorRecord(contexts[0], db, record); err != nil {
			return err
		}
		if _, err := ReadConnectorRecord(contexts[1], db, record.ID); err == nil {
			return fmt.Errorf("connection crossed user boundary")
		}
		if _, err := ReadConnectorRecord(captured, db, record.ID); err != nil {
			return err
		}
		if _, err := RemovePlugin(contexts[0], db, info.Summary.Name, 2); err != nil {
			return err
		}
		other, err := ReadPlugin(contexts[1], db, info.Summary.Name)
		if err != nil || other == nil || !other.Summary.Enabled {
			return fmt.Errorf("removal changed another installation: %v", err)
		}
		project, worktree := protocol.NewProjectID(), protocol.NewWorktreeID()
		if _, err := db.ExecContext(ctx, "INSERT INTO projects(id,name,path,appearance,owner,visibility) VALUES(?1,'Private fixture','/plugin-project','{}',?2,'private')", project.String(), local.ID.String()); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO worktrees(id,project,path,main) VALUES(?1,?2,'/plugin-project',1)", worktree.String(), project.String()); err != nil {
			return err
		}
		request := protocol.NewRequest(node, protocol.Command{Kind: "write_plugin_value", Data: &protocol.CommandWritePluginValuePayload{Key: "shared/record", Value: json.RawMessage(`{"owner":0}`), Scope: protocol.PluginStorageScopeProject}})
		request.Plugin = &protocol.PluginContext{Package: refs[0], Worktree: &worktree}
		if _, _, err := ExecutePluginStorageCommand(contexts[0], db, node, "", node, request); err != nil {
			return err
		}
		request.Command = protocol.Command{Kind: "read_plugin_value", Data: &protocol.CommandReadPluginValuePayload{Key: "shared/record", Scope: protocol.PluginStorageScopeProject}}
		if _, err := ReadPluginCommand(contexts[1], db, request); err == nil {
			return fmt.Errorf("private project storage was visible to another user")
		}
		if _, err := db.ExecContext(ctx, "UPDATE projects SET visibility='shared' WHERE id=?1", project.String()); err != nil {
			return err
		}
		if _, err := ReadPluginCommand(contexts[1], db, request); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "UPDATE projects SET visibility='private' WHERE id=?1", project.String()); err != nil {
			return err
		}
		if _, err := ReadPluginCommand(contexts[1], db, request); err == nil {
			return fmt.Errorf("project storage access survived visibility revocation")
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestLegacyPluginBootstrapPreservesSourcesAndRemoval(t *testing.T) {
	t.Parallel()
	profile, node, storage := schemaFixture(t)
	ctx := t.Context()
	info := pluginTestInfo("legacy-fixture", 'a')
	info.Summary.Revision, info.Summary.SettingsRevision = 1, 1
	info.Summary.Enabled = true
	body, err := encode(info)
	if err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := db.ExecContext(ctx, "DROP TABLE plugin_user_installations"); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO plugins(name,revision,settings_revision,installed,body) VALUES(?1,1,1,1,?2)", info.Summary.Name, body); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO plugin_settings(plugin,revision,body) VALUES(?1,1,?2)", info.Summary.Name, []byte(`{}`)); err != nil {
			return err
		}
		_, err := db.ExecContext(ctx, "INSERT INTO plugin_values(name,key,revision,value,index_data) VALUES(?1,'kept',1,?2,NULL)", info.Summary.Name, []byte(`{"kept":true}`))
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Close(); err != nil {
		t.Fatal(err)
	}
	storage = reopenSchemaFixture(t, profile, node)
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		installed, err := ReadPlugin(ctx, db, info.Summary.Name)
		if err != nil || installed == nil {
			return fmt.Errorf("legacy plugin was not initialized: %v", err)
		}
		var count int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM plugin_space_values WHERE name=?1", info.Summary.Name).Scan(&count); err != nil || count != 1 {
			return fmt.Errorf("legacy value was not initialized: count=%d, %v", count, err)
		}
		if _, err := RemovePlugin(ctx, db, info.Summary.Name, 1); err != nil {
			return err
		}
		for _, statement := range []string{
			"DELETE FROM plugin_user_settings WHERE plugin=?1",
			"DELETE FROM plugin_space_values WHERE name=?1",
		} {
			if _, err := db.ExecContext(ctx, statement, info.Summary.Name); err != nil {
				return err
			}
		}
		// Force a later additive schema update, not only a current-schema restart.
		_, err = db.ExecContext(ctx, "DROP INDEX session_association_identity")
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Close(); err != nil {
		t.Fatal(err)
	}
	storage = reopenSchemaFixture(t, profile, node)
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		installed, err := ReadPlugin(ctx, db, info.Summary.Name)
		if err != nil || installed != nil {
			return fmt.Errorf("removed legacy plugin was reinstalled: %v", err)
		}
		for _, item := range []struct{ table, column string }{
			{"plugins", "name"}, {"plugin_settings", "plugin"}, {"plugin_values", "name"},
			{"plugin_user_settings", "plugin"}, {"plugin_space_values", "name"},
		} {
			var count int
			if err := db.QueryRowContext(ctx, "SELECT count(*) FROM "+item.table+" WHERE "+item.column+"=?1", info.Summary.Name).Scan(&count); err != nil {
				return err
			}
			want := 1
			if item.table == "plugin_user_settings" || item.table == "plugin_space_values" {
				want = 0
			}
			if count != want {
				return fmt.Errorf("%s has %d records, want %d", item.table, count, want)
			}
		}
		var retained []byte
		if err := db.QueryRowContext(ctx, "SELECT body FROM plugins WHERE name=?1", info.Summary.Name).Scan(&retained); err != nil {
			return err
		}
		if string(retained) != string(body) {
			return fmt.Errorf("legacy plugin source changed")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
