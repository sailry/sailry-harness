package store

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func catalogFixtureInfo(version string, digest byte) protocol.PluginInfo {
	info := pluginTestInfo("version-fixture", digest)
	info.Summary.Version = &version
	info.UpdateSource = &protocol.PluginOrigin{Kind: "official"}
	return info
}

func catalogStorageExtension(collections []protocol.PluginStorageCollection) *protocol.PluginExtension {
	extension := protocol.DefaultPluginExtension()
	extension.ApiVersion = "v1"
	extension.Storage = &protocol.PluginStorageDeclaration{Collections: collections}
	return &extension
}

func TestCatalogSwitchChecksMCPSecretDestination(t *testing.T) {
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		declared := protocol.PluginMcpConfiguration{Schema: pluginMCPSchema, Servers: map[string]protocol.PluginMcpServer{"server": {
			Kind: "streamable-http", Data: &protocol.PluginMcpServerHttpPayload{Url: "https://first.example.test/mcp", Headers: map[string]string{"Authorization": ""}, Reuse: true},
		}}}
		schema, err := plugins.ValidateSettings([]byte(`{"$schema":"`+plugins.SettingsSchema+`","type":"object","additionalProperties":false,"properties":{"token":{"type":"string","x-sailry-secret":{"server":"server","header":"Authorization"}}},"required":["token"]}`), declared.Servers)
		if err != nil {
			return err
		}
		initial := catalogFixtureInfo("2.0.0", 'a')
		initial.Settings = &schema
		initial.Mcp = []protocol.PluginMcp{{Name: "server", Transport: protocol.PluginMcpTransportStreamableHttp, Allowed: true, Availability: protocol.PluginMcpAvailabilityAvailable}}
		initial, err = InstallPlugin(ctx, db, initial, 0, true)
		if err != nil {
			return err
		}
		if _, err := SavePluginSettings(ctx, db, protocol.CommandSavePluginSettingsPayload{Package: plugins.PackageReference(initial), Values: map[string]json.RawMessage{},
			Secrets: map[string]protocol.PluginSettingsSecretUpdate{"token": {Kind: "replace", Data: protocol.NewSecret("fixture-secret")}}}, MCPSettingsTargets(initial.Settings, declared)); err != nil {
			return err
		}
		current, err := ReadPlugin(ctx, db, initial.Summary.Name)
		if err != nil || current == nil {
			return fmt.Errorf("read configured package: %v", err)
		}
		next := catalogFixtureInfo("1.0.0", 'b')
		next.Settings, next.Mcp = &schema, initial.Mcp
		changed := protocol.PluginMcpConfiguration{Schema: pluginMCPSchema, Servers: map[string]protocol.PluginMcpServer{"server": {
			Kind: "streamable-http", Data: &protocol.PluginMcpServerHttpPayload{Url: "https://second.example.test/mcp", Headers: map[string]string{"Authorization": ""}, Reuse: true},
		}}}
		if _, err := InstallCatalogPlugin(ctx, db, next, current.Summary.Revision, protocol.PluginInstallModeSwitch, changed); err == nil {
			return fmt.Errorf("changed MCP destination accepted the retained credential")
		}
		kept, err := ReadPlugin(ctx, db, initial.Summary.Name)
		if err != nil || kept == nil || !reflect.DeepEqual(kept.Summary, current.Summary) {
			return fmt.Errorf("rejected MCP switch changed installed state: %v", err)
		}
		switched, err := InstallCatalogPlugin(ctx, db, next, current.Summary.Revision, protocol.PluginInstallModeSwitch, declared)
		if err != nil {
			return err
		}
		resolved, err := ResolvePluginSettings(ctx, db, plugins.PackageReference(switched))
		if err != nil || resolved.Secrets["token"].Expose() != "fixture-secret" {
			return fmt.Errorf("compatible switch lost the retained credential: %v", err)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestCatalogDowngradeUsesResourceAccess(t *testing.T) {
	for _, scenario := range []string{"shared conversation", "owned project conversation", "revoked conversation", "removed project", "private foreign project"} {
		t.Run(scenario, func(t *testing.T) {
			_, _, storage := schemaFixture(t)
			ctx := t.Context()
			if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				local, err := LocalUser(ctx, db)
				if err != nil {
					return err
				}
				other, err := CreateUser(ctx, db, "Other fixture user")
				if err != nil {
					return err
				}
				owner, audience, sessionOwner := other.ID, "private", other.ID
				switch scenario {
				case "shared conversation":
					audience = "shared"
				case "owned project conversation", "removed project":
					owner = local.ID
				case "revoked conversation":
					sessionOwner = local.ID
				}
				project, session := protocol.NewProjectID(), protocol.NewSessionID()
				if _, err := db.ExecContext(ctx, "INSERT INTO projects(id,name,path,appearance,owner,visibility) VALUES(?1,'Fixture',?2,?3,?4,?5)", project.String(), filepath.Join(t.TempDir(), "project"), []byte(`{}`), owner.String(), audience); err != nil {
					return err
				}
				initial, err := InstallPlugin(ctx, db, catalogFixtureInfo("2.0.0", 'a'), 0, true)
				if err != nil {
					return err
				}
				if scenario == "removed project" || scenario == "private foreign project" {
					if _, err := db.ExecContext(ctx, "INSERT INTO plugin_space_values(space,name,key,revision,value) VALUES(?1,?2,'state',1,?3)", "project:"+project.String(), initial.Summary.Name, []byte(`{"format":2}`)); err != nil {
						return err
					}
					if scenario == "removed project" {
						if _, err := db.ExecContext(ctx, "INSERT INTO removed_projects(project) VALUES(?1)", project.String()); err != nil {
							return err
						}
					}
				} else {
					if _, err := db.ExecContext(ctx, "INSERT INTO sessions(id,project,owner,revision,history_revision) VALUES(?1,?2,?3,1,1)", session.String(), project.String(), sessionOwner.String()); err != nil {
						return err
					}
					if _, err := db.ExecContext(ctx, "INSERT INTO plugin_conversation_records(name,key,value) VALUES(?1,'state',?2)", initial.Summary.Name, []byte(`{"format":2}`)); err != nil {
						return err
					}
					if _, err := db.ExecContext(ctx, "INSERT INTO plugin_conversation_values(name,session,key,revision,record,restored) VALUES(?1,?2,'state',1,last_insert_rowid(),0)", initial.Summary.Name, session.String()); err != nil {
						return err
					}
				}
				key := "state"
				next := catalogFixtureInfo("1.0.0", 'b')
				next.Extension = catalogStorageExtension([]protocol.PluginStorageCollection{
					{Scope: protocol.PluginStorageScopeProject, Key: &key, Schema: json.RawMessage(`{"type":"object","properties":{"format":{"enum":[1]}}}`)},
					{Scope: protocol.PluginStorageScopeConversation, Key: &key, Schema: json.RawMessage(`{"type":"object","properties":{"format":{"enum":[1]}}}`)},
				})
				_, err = InstallCatalogPlugin(ctx, db, next, initial.Summary.Revision, protocol.PluginInstallModeSwitch, protocol.PluginMcpConfiguration{})
				mustReject := scenario == "shared conversation" || scenario == "owned project conversation"
				if mustReject && err == nil {
					return fmt.Errorf("accessible incompatible data did not prevent downgrade")
				}
				if !mustReject && err != nil {
					return fmt.Errorf("inaccessible data blocked downgrade: %w", err)
				}
				return nil
			}); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestCatalogVersionSwitchPreservesState(t *testing.T) {
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		schema, err := plugins.ValidateSettings([]byte(`{"$schema":"` + plugins.SettingsSchema + `","type":"object","properties":{"label":{"type":"string"}},"additionalProperties":false}`))
		if err != nil {
			return err
		}
		initial := catalogFixtureInfo("1.5.2", 'a')
		initial.Settings = &schema
		initial, err = InstallPlugin(ctx, db, initial, 0, false)
		if err != nil {
			return err
		}
		settings, err := SavePluginSettings(ctx, db, protocol.CommandSavePluginSettingsPayload{
			Package: plugins.PackageReference(initial), Values: map[string]json.RawMessage{"label": json.RawMessage(`"kept"`)},
		})
		if err != nil {
			return err
		}
		current, err := ReadPlugin(ctx, db, initial.Summary.Name)
		if err != nil || current == nil {
			return fmt.Errorf("read configured package: %v", err)
		}
		captured := plugins.PackageReference(*current)
		removed, err := InstallPlugin(ctx, db, pluginTestInfo("removed-fixture", 'd'), 0, true)
		if err != nil {
			return err
		}
		if _, err = RemovePlugin(ctx, db, removed.Summary.Name, removed.Summary.Revision); err != nil {
			return err
		}
		older := catalogFixtureInfo("1.4.0", 'b')
		older.Settings = &schema
		switched, err := InstallCatalogPlugin(ctx, db, older, current.Summary.Revision, protocol.PluginInstallModeSwitch, protocol.PluginMcpConfiguration{})
		if err != nil {
			return err
		}
		if !switched.Summary.Pinned || switched.Summary.Enabled || switched.Summary.SettingsRevision != settings.Package.SettingsRevision {
			return fmt.Errorf("switch lost preferences or settings: %+v", switched.Summary)
		}
		preserved, err := ReadPluginSettings(ctx, db, plugins.PackageReference(switched))
		if err != nil || string(preserved.Values["label"]) != `"kept"` {
			return fmt.Errorf("switch changed configuration: %v", err)
		}
		previous, err := ReadPluginVersion(ctx, db, captured)
		if err != nil || previous.Summary.Digest != captured.Digest || *previous.Summary.Version != "1.5.2" {
			return fmt.Errorf("switch lost admitted package: %v", err)
		}
		if recorded, err := PluginRecorded(ctx, db, removed.Summary.Name); err != nil || !recorded {
			return fmt.Errorf("switch lost a removal record: %v", err)
		}
		if visible, err := ReadPlugin(ctx, db, removed.Summary.Name); err != nil || visible != nil {
			return fmt.Errorf("switch reinstalled a removed package: %v", err)
		}
		newer := catalogFixtureInfo("1.6.0", 'c')
		newer.Settings = &schema
		if _, err := InstallCatalogPlugin(ctx, db, newer, switched.Summary.Revision, protocol.PluginInstallModeUpdate, protocol.PluginMcpConfiguration{}); !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
			return fmt.Errorf("pinned package accepted an update: %v", err)
		}
		following, err := SetPluginPinned(ctx, db, switched.Summary.Name, switched.Summary.Revision, false)
		if err != nil || following.Summary.Pinned || following.Summary.Digest != switched.Summary.Digest {
			return fmt.Errorf("resume following changed code: %v", err)
		}
		if _, err := SetPluginPinned(ctx, db, following.Summary.Name, switched.Summary.Revision, true); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
			return fmt.Errorf("stale pin revision was accepted: %v", err)
		}
		if _, err := InstallCatalogPlugin(ctx, db, older, switched.Summary.Revision, protocol.PluginInstallModeSwitch, protocol.PluginMcpConfiguration{}); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
			return fmt.Errorf("stale switch revision was accepted: %v", err)
		}
		updated, err := InstallCatalogPlugin(ctx, db, newer, following.Summary.Revision, protocol.PluginInstallModeUpdate, protocol.PluginMcpConfiguration{})
		if err != nil || updated.Summary.Pinned || updated.Summary.Enabled || updated.Summary.SettingsRevision != settings.Package.SettingsRevision {
			return fmt.Errorf("following update changed preserved state: %v", err)
		}
		for _, version := range []string{"1.6.0", "1.6.0+build.2", "1.5.0"} {
			next := catalogFixtureInfo(version, 'e')
			next.Settings = &schema
			if _, err := InstallCatalogPlugin(ctx, db, next, updated.Summary.Revision, protocol.PluginInstallModeUpdate, protocol.PluginMcpConfiguration{}); !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
				return fmt.Errorf("update accepted version %q: %v", version, err)
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestCatalogSwitchRejectsIncompatibleState(t *testing.T) {
	for _, scenario := range []string{"settings type", "removed settings", "stored data", "unsupported API"} {
		t.Run(scenario, func(t *testing.T) {
			_, _, storage := schemaFixture(t)
			ctx := t.Context()
			if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				schema, err := plugins.ValidateSettings([]byte(`{"$schema":"` + plugins.SettingsSchema + `","type":"object","properties":{"label":{"type":"string"}},"additionalProperties":false}`))
				if err != nil {
					return err
				}
				initial := catalogFixtureInfo("2.0.0", 'a')
				initial.Settings = &schema
				initial, err = InstallPlugin(ctx, db, initial, 0, true)
				if err != nil {
					return err
				}
				if _, err := SavePluginSettings(ctx, db, protocol.CommandSavePluginSettingsPayload{Package: plugins.PackageReference(initial), Values: map[string]json.RawMessage{"label": json.RawMessage(`"kept"`)}}); err != nil {
					return err
				}
				current, err := ReadPlugin(ctx, db, initial.Summary.Name)
				if err != nil || current == nil {
					return fmt.Errorf("read current version: %v", err)
				}
				next := catalogFixtureInfo("1.0.0", 'b')
				next.Settings = &schema
				switch scenario {
				case "settings type":
					target := schema
					target.Properties = map[string]protocol.PluginSettingsField{}
					field := schema.Properties["label"]
					field.Kind = protocol.PluginSettingsKindInteger
					target.Properties["label"] = field
					next.Settings = &target
				case "removed settings":
					next.Settings = nil
				case "stored data":
					if _, err := WritePluginValue(ctx, db, plugins.PackageReference(*current), "state", json.RawMessage(`{"format":2}`), 0, nil); err != nil {
						return err
					}
					key := "state"
					next.Extension = catalogStorageExtension([]protocol.PluginStorageCollection{{Scope: protocol.PluginStorageScopeNode, Key: &key, Schema: json.RawMessage(`{"type":"object","properties":{"format":{"enum":[1]}}}`)}})
				case "unsupported API":
					next.Requirements = &protocol.PluginRequirements{APIVersion: "v999"}
				}
				if _, err := InstallCatalogPlugin(ctx, db, next, current.Summary.Revision, protocol.PluginInstallModeSwitch, protocol.PluginMcpConfiguration{}); err == nil {
					return fmt.Errorf("incompatible %s switch was accepted", scenario)
				}
				kept, err := ReadPlugin(ctx, db, initial.Summary.Name)
				if err != nil || kept == nil || !reflect.DeepEqual(kept.Summary, current.Summary) {
					return fmt.Errorf("failed switch changed installed state: %v", err)
				}
				state, err := ReadPluginSettings(ctx, db, plugins.PackageReference(*kept))
				if err != nil || string(state.Values["label"]) != `"kept"` {
					return fmt.Errorf("failed switch changed configuration: %v", err)
				}
				return nil
			}); err != nil {
				t.Fatal(err)
			}
		})
	}
}
