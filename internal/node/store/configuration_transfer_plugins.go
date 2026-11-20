package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// ConfigurationPlugin is an installed package's choices and settings. The
// package itself is not carried: an import configures only packages that the
// target already has.
type ConfigurationPlugin struct {
	Name    string                     `json:"name"`
	Digest  string                     `json:"digest"`
	Version *string                    `json:"version,omitempty"`
	Enabled bool                       `json:"enabled"`
	Pinned  bool                       `json:"pinned,omitempty"`
	Values  map[string]json.RawMessage `json:"values,omitempty"`
	Secrets map[string]protocol.Secret `json:"secrets,omitempty"`
}

func exportPlugins(ctx context.Context, db *sql.Conn, _ protocol.NodeID, document *ConfigurationDocument) error {
	summaries, err := ReadPlugins(ctx, db)
	if err != nil {
		return err
	}
	for _, summary := range summaries {
		entry := ConfigurationPlugin{Name: summary.Name, Digest: summary.Digest, Version: summary.Version, Enabled: summary.Enabled, Pinned: summary.Pinned}
		if summary.SettingsRevision != 0 {
			settings, err := ResolvePluginSettings(ctx, db, protocol.PluginReference{Name: summary.Name, Digest: summary.Digest, SettingsRevision: summary.SettingsRevision})
			if err != nil {
				return err
			}
			entry.Values = settings.Values
			if len(settings.Secrets) != 0 {
				entry.Secrets = settings.Secrets
			}
		}
		document.Plugins = append(document.Plugins, entry)
	}
	slices.SortFunc(document.Plugins, func(a, b ConfigurationPlugin) int { return strings.Compare(a.Name, b.Name) })
	return nil
}

func importPlugins(importer *configurationImporter, document ConfigurationDocument) error {
	for _, entry := range document.Plugins {
		if err := importer.plugin(entry); err != nil {
			return err
		}
	}
	return nil
}

// plugin applies a package's enabled and pinned choices when it is
// installed, and its settings only when the installed package is the same
// build: another version may declare different fields and destinations.
func (importer *configurationImporter) plugin(entry ConfigurationPlugin) error {
	ctx, db := importer.ctx, importer.db
	info, err := ReadPlugin(ctx, db, entry.Name)
	if err != nil {
		return err
	}
	if info == nil {
		importer.skip("plugins", entry.Name, "not installed; install it and import again")
		return nil
	}
	if err := importer.entry("plugins", entry.Name, func() (protocol.ConfigurationImportAction, *string, []protocol.Event, error) {
		changed, err := SetPluginEnabled(ctx, db, entry.Name, info.Summary.Revision, entry.Enabled)
		if err != nil {
			return "", nil, nil, err
		}
		changed, err = SetPluginPinned(ctx, db, entry.Name, changed.Summary.Revision, entry.Pinned)
		if err != nil {
			return "", nil, nil, err
		}
		var events []protocol.Event
		if changed.Summary.Revision != info.Summary.Revision {
			events = append(events, protocol.Event{Kind: "plugin_changed", Data: changed.Summary})
		}
		return protocol.ConfigurationImportUpdated, nil, events, nil
	}); err != nil {
		return err
	}
	if len(entry.Values) == 0 && len(entry.Secrets) == 0 {
		return nil
	}
	if info.Summary.Digest != entry.Digest {
		importer.skip("plugin_settings", entry.Name, "different version; settings were not imported")
		return nil
	}
	targets, resolved := importer.targets.Plugins[entry.Name]
	if !resolved {
		importer.skip("plugin_settings", entry.Name, "the package's settings destinations could not be resolved")
		return nil
	}
	return importer.entry("plugin_settings", entry.Name, func() (protocol.ConfigurationImportAction, *string, []protocol.Event, error) {
		current, err := ReadPlugin(ctx, db, entry.Name)
		if err != nil {
			return "", nil, nil, err
		}
		if current.Settings == nil {
			return "", nil, nil, fault(protocol.ErrorCodeNotConfigured, "plugin has no valid settings form")
		}
		input := protocol.CommandSavePluginSettingsPayload{Package: plugins.PackageReference(*current), Values: map[string]json.RawMessage{}, Secrets: map[string]protocol.PluginSettingsSecretUpdate{}}
		for name, value := range entry.Values {
			if field, declared := current.Settings.Properties[name]; declared && field.Secret == nil {
				input.Values[name] = value
			}
		}
		for name, field := range current.Settings.Properties {
			if field.Secret == nil {
				continue
			}
			input.Secrets[name] = protocol.PluginSettingsSecretUpdate{Kind: "keep"}
			if secret, present := entry.Secrets[name]; present {
				input.Secrets[name] = protocol.PluginSettingsSecretUpdate{Kind: "replace", Data: secret}
			}
		}
		state, err := SavePluginSettings(ctx, db, input, targets)
		if err != nil {
			return "", nil, nil, err
		}
		saved, err := ReadPlugin(ctx, db, entry.Name)
		if err != nil {
			return "", nil, nil, err
		}
		events := []protocol.Event{{Kind: "plugin_changed", Data: saved.Summary}}
		dispatch, err := DispatchSettingsChanged(ctx, db, state.Package, time.Now().UnixMilli())
		return protocol.ConfigurationImportUpdated, nil, append(events, dispatch...), err
	})
}
