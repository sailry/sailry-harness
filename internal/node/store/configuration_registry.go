package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"strings"
	"sync"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type ConfigurationExport struct {
	Node         protocol.NodeID
	Secrets      bool
	Declarations map[string]protocol.PluginMcpConfiguration
}

// ConfigurationKind owns one complete synchronization contract. A new personal
// category registers its exporter, validation and projections in one place.
type ConfigurationKind struct {
	Name            string
	Events          []string
	Runtime         bool
	PersonalSetting string
	Export          func(context.Context, *sql.Conn, ConfigurationExport) ([]protocol.ConfigurationFollowRecord, error)
	Validate        func(context.Context, *sql.Conn, protocol.ConfigurationFollowRecord) error
	Changed         func(context.Context, *sql.Conn, []protocol.ConfigurationFollowRecord) ([]protocol.Event, error)
	Removed         func(string) (*protocol.Event, error)
}

var configurationRegistry = struct {
	sync.RWMutex
	kinds  map[string]ConfigurationKind
	events map[string][]string
}{kinds: map[string]ConfigurationKind{}, events: map[string][]string{}}

var ConfigurationCategories []string

// RegisterConfigurationKind is called during startup, before Nodes run.
func RegisterConfigurationKind(kind ConfigurationKind) {
	configurationRegistry.Lock()
	defer configurationRegistry.Unlock()
	if kind.Name == "" || kind.Export == nil {
		panic("configuration registration requires a name and exporter")
	}
	if _, exists := configurationRegistry.kinds[kind.Name]; exists {
		panic("duplicate configuration kind: " + kind.Name)
	}
	configurationRegistry.kinds[kind.Name] = kind
	ConfigurationCategories = append(ConfigurationCategories, kind.Name)
	for _, event := range kind.Events {
		configurationRegistry.events[event] = append(configurationRegistry.events[event], kind.Name)
	}
}

func configurationKind(name string) (ConfigurationKind, bool) {
	configurationRegistry.RLock()
	defer configurationRegistry.RUnlock()
	kind, found := configurationRegistry.kinds[name]
	return kind, found
}

func ConfigurationChanged(event string) bool {
	configurationRegistry.RLock()
	defer configurationRegistry.RUnlock()
	return len(configurationRegistry.events[event]) != 0
}

func ConfigurationDirtyKinds(event string) []string {
	configurationRegistry.RLock()
	defer configurationRegistry.RUnlock()
	return append([]string(nil), configurationRegistry.events[event]...)
}

func RuntimeCredentialEvent(event string) bool {
	configurationRegistry.RLock()
	defer configurationRegistry.RUnlock()
	for _, name := range configurationRegistry.events[event] {
		if configurationRegistry.kinds[name].Runtime {
			return true
		}
	}
	return false
}

func personalConfigurationKind(name string) (ConfigurationKind, bool) {
	configurationRegistry.RLock()
	defer configurationRegistry.RUnlock()
	for _, kind := range configurationRegistry.kinds {
		if kind.PersonalSetting == name {
			return kind, true
		}
	}
	return ConfigurationKind{}, false
}

func changedConfiguration[T any](event string) func(context.Context, *sql.Conn, []protocol.ConfigurationFollowRecord) ([]protocol.Event, error) {
	return func(_ context.Context, _ *sql.Conn, records []protocol.ConfigurationFollowRecord) ([]protocol.Event, error) {
		changes := make([]protocol.Event, 0, len(records))
		for _, record := range records {
			if len(record.Body) == 0 {
				continue
			}
			var value T
			if err := json.Unmarshal(record.Body, &value); err != nil {
				return nil, err
			}
			changes = append(changes, protocol.Event{Kind: event, Data: value})
		}
		return changes, nil
	}
}

func changedRevision[T any](event string, makeValue func(uint64) T) func(context.Context, *sql.Conn, []protocol.ConfigurationFollowRecord) ([]protocol.Event, error) {
	return func(_ context.Context, _ *sql.Conn, records []protocol.ConfigurationFollowRecord) ([]protocol.Event, error) {
		changes := []protocol.Event{}
		for _, record := range records {
			changes = append(changes, protocol.Event{Kind: event, Data: makeValue(record.Revision)})
		}
		return changes, nil
	}
}

func removedConfiguration[T any](event string, parse func(string) (T, error), payload func(T) any) func(string) (*protocol.Event, error) {
	return func(id string) (*protocol.Event, error) {
		value, err := parse(id)
		if err != nil {
			return nil, err
		}
		return &protocol.Event{Kind: event, Data: payload(value)}, nil
	}
}

func validateConfigurationIdentity[T any](identity func(T) string) func(context.Context, *sql.Conn, protocol.ConfigurationFollowRecord) error {
	return func(_ context.Context, _ *sql.Conn, record protocol.ConfigurationFollowRecord) error {
		var value T
		if err := json.Unmarshal(record.Body, &value); err != nil {
			return err
		}
		if identity(value) != record.ID {
			return invalid("configuration identity does not match its manifest")
		}
		return nil
	}
}

func init() {
	RegisterConfigurationKind(ConfigurationKind{
		Name: "plugins", Events: []string{"plugin_changed", "plugin_removed"}, Runtime: true, Export: exportPluginConfiguration,
		Validate: validateConfigurationIdentity(func(value protocol.PluginInfo) string { return value.Summary.Name }),
		Changed: func(_ context.Context, _ *sql.Conn, records []protocol.ConfigurationFollowRecord) ([]protocol.Event, error) {
			changes := []protocol.Event{}
			for _, record := range records {
				var value protocol.PluginInfo
				if len(record.Body) != 0 {
					if err := json.Unmarshal(record.Body, &value); err != nil {
						return nil, err
					}
					changes = append(changes, protocol.Event{Kind: "plugin_changed", Data: value.Summary})
				}
			}
			return changes, nil
		},
		Removed: func(name string) (*protocol.Event, error) {
			if !plugins.ValidateName(name) {
				return nil, invalid("invalid removed package name")
			}
			return &protocol.Event{Kind: "plugin_removed", Data: protocol.EventPluginRemovedPayload{Name: name}}, nil
		},
	})
	RegisterConfigurationKind(ConfigurationKind{
		Name: "providers", Events: []string{"provider_changed", "provider_removed"}, Runtime: true, Export: exportProviderConfiguration,
		Validate: validateConfigurationIdentity(func(value protocol.ConversationProvider) string { return value.ID.String() }),
		Changed:  changedConfiguration[protocol.ConversationProvider]("provider_changed"),
		Removed:  removedConfiguration("provider_removed", protocol.ParseProviderID, func(id protocol.ProviderID) any { return protocol.EventProviderRemovedPayload{ID: id} }),
	})
	RegisterConfigurationKind(ConfigurationKind{
		Name: "roles", Events: []string{"role_changed", "role_removed"}, Export: exportRoleConfiguration,
		Validate: validateConfigurationIdentity(func(value protocol.RoleProfile) string { return value.ID.String() }),
		Changed:  changedConfiguration[protocol.RoleProfile]("role_changed"),
		Removed: removedConfiguration("role_removed", protocol.ParseRoleID, func(id protocol.RoleID) any {
			return protocol.EventRoleRemovedPayload{ID: id, Teams: []protocol.TeamProfile{}}
		}),
	})
	RegisterConfigurationKind(ConfigurationKind{
		Name: "teams", Events: []string{"team_changed", "team_removed"}, Export: exportTeamConfiguration,
		Validate: validateConfigurationIdentity(func(value protocol.TeamProfile) string { return value.ID.String() }),
		Changed:  changedConfiguration[protocol.TeamProfile]("team_changed"),
		Removed:  removedConfiguration("team_removed", protocol.ParseTeamID, func(id protocol.TeamID) any { return protocol.EventTeamRemovedPayload{ID: id} }),
	})
	RegisterConfigurationKind(ConfigurationKind{
		Name: "ssh_profiles", Events: []string{"ssh_changed", "ssh_removed"}, Export: exportSshConfiguration,
		Validate: validateConfigurationIdentity(func(value protocol.SshProfile) string { return value.ID.String() }),
		Changed:  changedConfiguration[protocol.SshProfile]("ssh_changed"),
		Removed:  removedConfiguration("ssh_removed", protocol.ParseSshID, func(id protocol.SshID) any { return protocol.EventSshRemovedPayload{ID: id} }),
	})
	RegisterConfigurationKind(ConfigurationKind{
		Name: "database_profiles", Events: []string{"database_changed", "database_removed"}, Export: exportDatabaseConfiguration,
		Validate: validateConfigurationIdentity(func(value protocol.DatabaseProfile) string { return value.ID.String() }),
		Changed:  changedConfiguration[protocol.DatabaseProfile]("database_changed"),
		Removed:  removedConfiguration("database_removed", protocol.ParseDatabaseID, func(id protocol.DatabaseID) any { return protocol.EventDatabaseRemovedPayload{ID: id} }),
	})
	RegisterConfigurationKind(ConfigurationKind{
		Name: "defaults", Events: []string{"defaults_changed"}, Export: exportDefaultsConfiguration,
		Changed: changedConfiguration[protocol.Defaults]("defaults_changed"),
	})
	RegisterConfigurationKind(ConfigurationKind{
		Name: "team_settings", Events: []string{"team_settings_changed"}, Export: exportTeamSettingsConfiguration,
		Changed: changedRevision("team_settings_changed", func(revision uint64) any { return protocol.EventTeamSettingsChangedPayload{Revision: revision} }),
	})
	RegisterConfigurationKind(ConfigurationKind{
		Name: "plugin_settings", Events: []string{"plugin_changed", "plugin_removed"}, Runtime: true, Export: exportPluginSettingsConfiguration,
		Validate: validateConfigurationIdentity(func(value pluginFollowSettings) string { return pluginSettingsCacheID(value.Package) }),
	})
	RegisterConfigurationKind(ConfigurationKind{
		Name: "connections", Events: []string{"connections_changed"}, Runtime: true, Export: exportConnectionConfiguration,
		Validate: validateConfigurationIdentity(func(value pluginFollowConnection) string { return value.ID.String() }),
		Changed:  changedConnections,
	})
	RegisterConfigurationKind(ConfigurationKind{
		Name: "runtime_credentials", Events: []string{"credential_changed", "provider_changed", "provider_removed", "connections_changed"},
		Runtime: true, Export: func(ctx context.Context, db *sql.Conn, options ConfigurationExport) ([]protocol.ConfigurationFollowRecord, error) {
			return ExportRuntimeCredentials(ctx, db, options.Node)
		},
		Validate: validateRuntimeConfiguration, Changed: changedRuntimeCredentials,
		Removed: func(id string) (*protocol.Event, error) {
			if !strings.HasPrefix(id, "provider:") {
				return nil, nil
			}
			credential, err := protocol.ParseCredentialID(strings.TrimPrefix(id, "provider:"))
			if err != nil {
				return nil, err
			}
			// The surrounding cache event supplies the authenticated controller
			// scope; the removed identifier wakes the captured generation.
			return &protocol.Event{Kind: "credential_changed", Data: protocol.EventCredentialChangedPayload{Credential: protocol.CredentialRef{ID: credential}, Revision: 1}}, nil
		},
	})
	registerPersonalConfiguration("media", "media_settings_changed", nil)
	registerPersonalConfiguration("compaction", "compaction_settings_changed", func(revision uint64) any { return protocol.EventCompactionSettingsChangedPayload{Revision: revision} })
	registerPersonalConfiguration("search", "search_settings_changed", func(revision uint64) any { return protocol.EventSearchSettingsChangedPayload{Revision: revision} })
	// Terminal shell/environment paths remain execution-machine settings.
}

func registerPersonalConfiguration(name, event string, payload func(uint64) any) {
	changed := changedRevision(event, payload)
	if payload == nil {
		changed = changedConfiguration[protocol.MediaSettings](event)
	}
	RegisterConfigurationKind(ConfigurationKind{
		Name: "personal:" + name, PersonalSetting: name, Events: []string{event},
		Export: func(ctx context.Context, db *sql.Conn, options ConfigurationExport) ([]protocol.ConfigurationFollowRecord, error) {
			return exportPersonalConfiguration(ctx, db, name, options)
		},
		Changed: func(ctx context.Context, db *sql.Conn, records []protocol.ConfigurationFollowRecord) ([]protocol.Event, error) {
			projected := append([]protocol.ConfigurationFollowRecord(nil), records...)
			for index := range projected {
				projected[index].Revision--
			}
			return changed(ctx, db, projected)
		},
	})
}

func changedConnections(ctx context.Context, db *sql.Conn, _ []protocol.ConfigurationFollowRecord) ([]protocol.Event, error) {
	value := protocol.DefaultPluginConnectorChanges()
	var err error
	value.Types, err = ConnectorTypes(ctx, db)
	if err == nil {
		value.Connections, err = Connections(ctx, db)
	}
	if err != nil {
		return nil, err
	}
	return []protocol.Event{{Kind: "connections_changed", Data: value}}, nil
}

func validateRuntimeConfiguration(ctx context.Context, db *sql.Conn, record protocol.ConfigurationFollowRecord) error {
	var value protocol.RuntimeCredential
	if err := json.Unmarshal(record.Body, &value); err != nil {
		return err
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	if user.Controller == nil || value.Controller != *user.Controller || value.CacheID() != record.ID ||
		value.Revision != record.Revision || value.Generation == "" {
		return invalid("runtime credential identity does not match its source")
	}
	return nil
}

func changedRuntimeCredentials(ctx context.Context, db *sql.Conn, records []protocol.ConfigurationFollowRecord) ([]protocol.Event, error) {
	changes, err := changedConnections(ctx, db, nil)
	if err != nil {
		return nil, err
	}
	for _, record := range records {
		if len(record.Body) == 0 {
			continue
		}
		var value protocol.RuntimeCredential
		if err := json.Unmarshal(record.Body, &value); err != nil {
			return nil, err
		}
		if value.Kind == "provider" {
			id, err := protocol.ParseCredentialID(value.ID)
			if err != nil {
				return nil, err
			}
			changes = append(changes, protocol.Event{Kind: "credential_changed", Data: protocol.EventCredentialChangedPayload{
				Credential: protocol.CredentialRef{Node: value.Controller, ID: id}, Revision: value.Revision,
			}})
		}
	}
	return changes, nil
}

func configurationRecord(id string, revision uint64, value any, secret *protocol.Secret) (protocol.ConfigurationFollowRecord, error) {
	body, err := json.Marshal(value)
	if err != nil {
		return protocol.ConfigurationFollowRecord{}, err
	}
	record := protocol.ConfigurationFollowRecord{ID: id, Revision: max(1, revision), Body: body, Secret: secret}
	record.Digest, err = configurationDigest(record)
	return record, err
}

func exportPersonalConfiguration(ctx context.Context, db *sql.Conn, name string, _ ConfigurationExport) ([]protocol.ConfigurationFollowRecord, error) {
	value, err := readPersonalSetting[map[string]json.RawMessage](ctx, db, name)
	if err != nil || value == nil {
		return nil, err
	}
	var revision uint64
	if name == "search" {
		var settings protocol.SearchSettings
		if err := json.Unmarshal((*value)["settings"], &settings); err != nil {
			return nil, err
		}
		revision = settings.Revision
	} else if err := json.Unmarshal((*value)["revision"], &revision); err != nil {
		return nil, fmt.Errorf("invalid personal settings revision: %w", err)
	}
	revision, err = nextRevision(revision)
	if err != nil {
		return nil, err
	}
	record, err := configurationRecord("default", revision, value, nil)
	return []protocol.ConfigurationFollowRecord{record}, err
}
