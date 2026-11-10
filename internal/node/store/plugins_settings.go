package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"reflect"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type storedPluginSettings struct {
	Schema         *protocol.PluginSettingsSchema `json:"schema"`
	Values         map[string]json.RawMessage     `json:"values"`
	Slots          map[string]*protocol.Secret    `json:"slots"`
	McpTargets     map[string]string              `json:"mcp_targets"`
	Authorizations json.RawMessage                `json:"authorizations,omitempty"`
	Mcp            json.RawMessage                `json:"mcp"`
	McpAllowed     map[string]bool                `json:"mcp_allowed,omitempty"`
	Github         json.RawMessage                `json:"github,omitempty"`
	Oauth          json.RawMessage                `json:"oauth,omitempty"`
}

type ResolvedPluginSettings struct {
	Values     map[string]json.RawMessage
	Secrets    map[string]protocol.Secret
	MCP        json.RawMessage
	MCPAllowed map[string]bool
	MCPTargets map[string]string
}

func readStoredPluginSettings(ctx context.Context, db *sql.Conn, info protocol.PluginInfo) (storedPluginSettings, error) {
	if info.Summary.SettingsRevision == 0 {
		return storedPluginSettings{
			Schema: info.Settings, Values: plugins.SettingDefaults(info.Settings), Slots: map[string]*protocol.Secret{},
			McpTargets: map[string]string{}, Mcp: json.RawMessage("null"), McpAllowed: map[string]bool{},
		}, nil
	}
	body, err := PluginSettingsBody(ctx, db, info.Summary.Name, info.Summary.SettingsRevision)
	if err != nil {
		return storedPluginSettings{}, err
	}
	var stored storedPluginSettings
	if err := json.Unmarshal(body, &stored); err != nil {
		return stored, invalid("stored plugin configuration is invalid")
	}
	if stored.Values == nil {
		stored.Values = map[string]json.RawMessage{}
	}
	if stored.Slots == nil {
		stored.Slots = map[string]*protocol.Secret{}
	}
	if stored.McpTargets == nil {
		stored.McpTargets = map[string]string{}
	}
	if stored.McpAllowed == nil {
		stored.McpAllowed = map[string]bool{}
	}
	if len(stored.Mcp) == 0 {
		stored.Mcp = json.RawMessage("null")
	}
	return stored, nil
}

func settingsBinding(schema *protocol.PluginSettingsSchema, name string) *protocol.PluginSettingsBinding {
	if schema == nil {
		return nil
	}
	return schema.Properties[name].Secret
}

func sameSettingsBinding(stored storedPluginSettings, schema *protocol.PluginSettingsSchema, name string, targets map[string]string) bool {
	before, after := settingsBinding(stored.Schema, name), settingsBinding(schema, name)
	if before == nil || after == nil || before.Unsupported || after.Unsupported || !reflect.DeepEqual(before, after) {
		return false
	}
	if after.Server != nil {
		previous, exists := stored.McpTargets[name]
		return exists && previous == targets[name]
	}
	return true
}

func projectPluginSettings(info protocol.PluginInfo, stored storedPluginSettings, targets map[string]string) protocol.PluginSettingsState {
	state := protocol.DefaultPluginSettingsState()
	state.Package = plugins.PackageReference(info)
	state.Values = plugins.SettingDefaults(info.Settings)
	if stored.Schema != nil {
		for name := range stored.Schema.Properties {
			delete(state.Values, name)
		}
	}
	if info.Settings == nil {
		return state
	}
	for name, value := range stored.Values {
		if field, exists := info.Settings.Properties[name]; exists && field.Secret == nil {
			state.Values[name] = value
		}
	}
	for name, field := range info.Settings.Properties {
		if field.Secret == nil {
			continue
		}
		same := sameSettingsBinding(stored, info.Settings, name, targets)
		if same && stored.Slots[name] != nil {
			state.Configured = append(state.Configured, name)
		}
		if same || stored.Slots[name] == nil {
			state.Keepable = append(state.Keepable, name)
		}
	}
	slices.Sort(state.Configured)
	slices.Sort(state.Keepable)
	state.Ready = reflect.DeepEqual(stored.Schema, info.Settings) && plugins.ValidatePublicSettings(*info.Settings, state.Values) == nil
	for _, name := range info.Settings.Required {
		if info.Settings.Properties[name].Secret != nil && !slices.Contains(state.Configured, name) {
			state.Ready = false
		}
	}
	return state
}

func ReadPluginSettings(ctx context.Context, db *sql.Conn, reference protocol.PluginReference, targets ...map[string]string) (protocol.PluginSettingsState, error) {
	info, err := ReadPlugin(ctx, db, reference.Name)
	if err != nil {
		return protocol.PluginSettingsState{}, err
	}
	if info == nil {
		return protocol.PluginSettingsState{}, fault(protocol.ErrorCodeNotFound, "plugin is not installed")
	}
	if plugins.PackageReference(*info) != reference {
		return protocol.PluginSettingsState{}, fault(protocol.ErrorCodeRevisionConflict, "plugin configuration changed")
	}
	if info.Settings == nil {
		return protocol.PluginSettingsState{}, fault(protocol.ErrorCodeNotConfigured, "plugin has no valid settings form")
	}
	stored, err := readStoredPluginSettings(ctx, db, *info)
	if err != nil {
		return protocol.PluginSettingsState{}, err
	}
	destinations := map[string]string{}
	if len(targets) != 0 {
		destinations = targets[0]
	}
	return projectPluginSettings(*info, stored, destinations), nil
}

func SavePluginSettings(ctx context.Context, db *sql.Conn, input protocol.CommandSavePluginSettingsPayload, targets ...map[string]string) (protocol.PluginSettingsState, error) {
	info, err := ReadPlugin(ctx, db, input.Package.Name)
	if err != nil {
		return protocol.PluginSettingsState{}, err
	}
	if info == nil {
		return protocol.PluginSettingsState{}, fault(protocol.ErrorCodeNotFound, "plugin is not installed")
	}
	if plugins.PackageReference(*info) != input.Package {
		return protocol.PluginSettingsState{}, fault(protocol.ErrorCodeRevisionConflict, "plugin configuration changed")
	}
	if info.Settings == nil {
		return protocol.PluginSettingsState{}, fault(protocol.ErrorCodeNotConfigured, "plugin has no valid settings form")
	}
	if err := plugins.ValidatePublicSettings(*info.Settings, input.Values); err != nil {
		return protocol.PluginSettingsState{}, err
	}
	stored, err := readStoredPluginSettings(ctx, db, *info)
	if err != nil {
		return protocol.PluginSettingsState{}, err
	}
	destinations := map[string]string{}
	if len(targets) != 0 {
		destinations = targets[0]
	}
	for name := range input.Secrets {
		if settingsBinding(info.Settings, name) == nil {
			return protocol.PluginSettingsState{}, invalid("secret update references an undeclared field")
		}
	}
	nextSlots := map[string]*protocol.Secret{}
	cleared := []string{}
	for name, field := range info.Settings.Properties {
		if field.Secret == nil {
			continue
		}
		update, exists := input.Secrets[name]
		if !exists {
			return protocol.PluginSettingsState{}, invalid("every secret setting requires an explicit update")
		}
		switch update.Kind {
		case "keep":
			if stored.Slots[name] != nil && !sameSettingsBinding(stored, info.Settings, name, destinations) {
				return protocol.PluginSettingsState{}, fault(protocol.ErrorCodeRevisionConflict, "secret destination changed; replace or clear its value")
			}
			nextSlots[name] = stored.Slots[name]
		case "replace":
			var secret protocol.Secret
			switch value := update.Data.(type) {
			case protocol.Secret:
				secret = value
			case *protocol.Secret:
				if value == nil {
					return protocol.PluginSettingsState{}, invalid("secret replacement requires a value")
				}
				secret = *value
			default:
				return protocol.PluginSettingsState{}, invalid("secret replacement requires a value")
			}
			if secret.Expose() == "" || strings.ContainsRune(secret.Expose(), 0) || len(secret.Expose()) > 4096 {
				return protocol.PluginSettingsState{}, invalid("invalid secret replacement")
			}
			publicField := field
			publicField.Secret = nil
			publicField.Default = nil
			schema := protocol.PluginSettingsSchema{Schema: plugins.SettingsSchema, Kind: protocol.PluginSettingsObjectObject,
				Properties: map[string]protocol.PluginSettingsField{name: publicField}, Required: []string{name}}
			raw, _ := json.Marshal(secret.Expose())
			if err := plugins.ValidatePublicSettings(schema, map[string]json.RawMessage{name: raw}); err != nil {
				return protocol.PluginSettingsState{}, invalid("secret replacement violates its declared constraints")
			}
			nextSlots[name] = &secret
		case "clear":
			nextSlots[name] = nil
			cleared = append(cleared, name)
		default:
			return protocol.PluginSettingsState{}, invalid("unsupported secret update")
		}
	}
	for _, name := range cleared {
		if err := clearPluginSecretHistory(ctx, db, info.Summary.Name, name); err != nil {
			return protocol.PluginSettingsState{}, err
		}
	}
	stored.Schema, stored.Values, stored.Slots, stored.McpTargets = info.Settings, input.Values, nextSlots, destinations
	revision, err := nextRevision(info.Summary.SettingsRevision)
	if err != nil {
		return protocol.PluginSettingsState{}, err
	}
	info.Summary.SettingsRevision = revision
	info.Summary.Revision, err = nextRevision(info.Summary.Revision)
	if err != nil {
		return protocol.PluginSettingsState{}, err
	}
	body, err := encode(stored)
	if err != nil {
		return protocol.PluginSettingsState{}, err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO plugin_settings(plugin,revision,body) VALUES(?1,?2,?3)", info.Summary.Name, revision, body); err != nil {
		return protocol.PluginSettingsState{}, err
	}
	if err := savePluginInventory(ctx, db, *info, true); err != nil {
		return protocol.PluginSettingsState{}, err
	}
	return projectPluginSettings(*info, stored, destinations), nil
}

func clearPluginSecretHistory(ctx context.Context, db *sql.Conn, plugin, name string) error {
	rows, err := db.QueryContext(ctx, "SELECT revision,body FROM plugin_settings WHERE plugin=?1", plugin)
	if err != nil {
		return err
	}
	type update struct {
		revision uint64
		body     []byte
	}
	updates := []update{}
	for rows.Next() {
		var revision uint64
		var body []byte
		if err := rows.Scan(&revision, &body); err != nil {
			rows.Close()
			return err
		}
		var fields map[string]json.RawMessage
		if err := json.Unmarshal(body, &fields); err != nil {
			rows.Close()
			return err
		}
		var slots map[string]json.RawMessage
		if err := json.Unmarshal(fields["slots"], &slots); err != nil {
			rows.Close()
			return err
		}
		if _, exists := slots[name]; exists {
			slots[name] = json.RawMessage("null")
			fields["slots"], err = json.Marshal(slots)
			if err != nil {
				rows.Close()
				return err
			}
			body, err = json.Marshal(fields)
			if err != nil {
				rows.Close()
				return err
			}
			updates = append(updates, update{revision: revision, body: body})
		}
	}
	err = rows.Err()
	rows.Close()
	if err != nil {
		return err
	}
	for _, update := range updates {
		if _, err := db.ExecContext(ctx, "UPDATE plugin_settings SET body=?1 WHERE plugin=?2 AND revision=?3", update.body, plugin, update.revision); err != nil {
			return err
		}
	}
	return nil
}

func ResolvePluginSettings(ctx context.Context, db *sql.Conn, reference protocol.PluginReference) (ResolvedPluginSettings, error) {
	info, err := ResolvePluginInfo(ctx, db, reference)
	if err != nil {
		return ResolvedPluginSettings{}, err
	}
	stored, err := readStoredPluginSettings(ctx, db, info)
	if err != nil {
		return ResolvedPluginSettings{}, err
	}
	result := ResolvedPluginSettings{Values: plugins.SettingDefaults(info.Settings), Secrets: map[string]protocol.Secret{}, MCP: stored.Mcp, MCPAllowed: stored.McpAllowed, MCPTargets: stored.McpTargets}
	for name, value := range stored.Values {
		result.Values[name] = value
	}
	current, err := ReadPlugin(ctx, db, reference.Name)
	if err != nil {
		return ResolvedPluginSettings{}, err
	}
	for name, secret := range stored.Slots {
		if secret == nil || current == nil || settingsBinding(current.Settings, name) == nil {
			continue
		}
		if sameSettingsBinding(stored, info.Settings, name, stored.McpTargets) &&
			reflect.DeepEqual(settingsBinding(info.Settings, name), settingsBinding(current.Settings, name)) {
			result.Secrets[name] = *secret
		}
	}
	return result, nil
}
