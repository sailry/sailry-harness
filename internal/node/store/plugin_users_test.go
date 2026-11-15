package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"testing"

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
		second, err := CreateUser(ctx, db, "Second user")
		if err != nil {
			return err
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
