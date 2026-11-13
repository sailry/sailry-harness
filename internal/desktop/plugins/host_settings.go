package plugins

import (
	"context"
	"encoding/json"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type pendingHostSettings struct {
	identity protocol.RequestID
	source   protocol.PluginReference
}

type hostSettings struct {
	current   protocol.PluginReference
	surface   bool
	pending   *pendingHostSettings
	live      *protocol.PluginSummary
	connected bool
	invalid   bool
}

func (settings *hostSettings) observe(rows []protocol.PluginSummary, connected bool) {
	if !settings.surface || settings.invalid {
		return
	}
	settings.connected = connected
	var live *protocol.PluginSummary
	for _, row := range rows {
		if row.Name == settings.current.Name {
			copy := row
			live = &copy
			break
		}
	}
	if !connected || live == nil || live.Digest != settings.current.Digest {
		settings.invalid = true
		return
	}
	if live.SettingsRevision < settings.current.SettingsRevision || settings.live != nil && settings.live.Revision > live.Revision {
		return
	}
	changedEnable := settings.live != nil && settings.live.Enabled != live.Enabled
	if settings.pending != nil {
		source := settings.pending.source.SettingsRevision
		if changedEnable || source != ^uint64(0) && live.SettingsRevision > source+1 {
			settings.invalid = true
		}
	} else if live.SettingsRevision > settings.current.SettingsRevision {
		// Unlike the Rust Host, which ends the page here, a newer revision
		// is adopted: the page stays mounted (RetainSettings), including
		// across a Client refresh racing its construction.
		settings.current.SettingsRevision = live.SettingsRevision
	}
	settings.live = live
}

func (settings *hostSettings) prepare(request protocol.Request, connected bool) error {
	if request.Command.Kind == "save_plugin_settings" {
		body, _ := json.Marshal(request.Command.Data)
		var data protocol.CommandSavePluginSettingsPayload
		if json.Unmarshal(body, &data) != nil || !settings.surface || settings.invalid || !connected || settings.pending != nil || data.Package != settings.current {
			return hostInvalid("settings save is unavailable or already pending")
		}
		settings.pending = &pendingHostSettings{identity: request.ID, source: data.Package}
	} else if settings.pending != nil {
		return hostInvalid("settings save is pending")
	}
	return nil
}

func (settings *hostSettings) finish(identity protocol.RequestID, output protocol.Output, failure error) error {
	if settings.pending == nil || settings.pending.identity != identity {
		return nil
	}
	if failure != nil {
		code := hostFault(failure).Code
		if code != protocol.ErrorCodeUnavailable && code != protocol.ErrorCodeOutcomeUnknown {
			settings.pending = nil
			if settings.live != nil && settings.live.SettingsRevision > settings.current.SettingsRevision {
				settings.invalid = true
				return hostInvalid("plugin configuration changed after the settings save")
			}
		}
		return nil
	}
	if output.Kind != "plugin_settings" {
		return nil
	}
	body, _ := json.Marshal(output.Data)
	var saved protocol.PluginSettingsState
	if json.Unmarshal(body, &saved) != nil || saved.Package.Name != settings.pending.source.Name ||
		saved.Package.Digest != settings.pending.source.Digest ||
		settings.pending.source.SettingsRevision == ^uint64(0) ||
		saved.Package.SettingsRevision != settings.pending.source.SettingsRevision+1 {
		settings.invalid = true
		return hostInvalid("plugin configuration changed after the settings save")
	}
	settings.current, settings.pending = saved.Package, nil
	return nil
}

func (owner *Host) ObserveSettings(rows []protocol.PluginSummary, connected bool) {
	owner.mu.Lock()
	owner.settings.observe(rows, connected)
	invalid := owner.settings.invalid
	owner.mu.Unlock()
	if invalid {
		owner.cancel()
	}
}

func (owner *Host) RetainSettings() bool {
	owner.mu.Lock()
	defer owner.mu.Unlock()
	return owner.settings.surface && !owner.settings.invalid && owner.settings.connected &&
		(owner.settings.pending != nil || owner.settings.current != owner.binding.Scope.Package)
}

func (owner *Host) settingsModule() HostModule {
	return HostModule{Name: "sailry/settings", Exports: []HostExport{
		hostSync("prepareSettings", func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			values, err := hostValue[map[string]json.RawMessage](arguments, 0)
			if err != nil {
				return nil, err
			}
			edits, err := hostValue[map[string]json.RawMessage](arguments, 1)
			if err != nil {
				return nil, err
			}
			revision, err := hostValue[string](arguments, 2)
			if err != nil {
				return nil, err
			}
			scope := owner.scope()
			if scope.Surface.Kind != "settings" || revision != exactRevisionText(scope.Package.SettingsRevision) {
				return nil, hostInvalid("plugin configuration changed or settings surface required")
			}
			secrets := map[string]protocol.PluginSettingsSecretUpdate{}
			for field, edit := range edits {
				update, err := owner.secretEdit(edit)
				if err != nil {
					return nil, err
				}
				secrets[field] = update
			}
			return owner.prepareData(ctx, "save_plugin_settings", map[string]any{"package": scope.Package, "values": values, "secrets": secrets})
		}),
		hostAsync("loadSettingSecret", owner.loadSettingSecret),
	}}
}

func (owner *Host) secretEdit(body json.RawMessage) (protocol.PluginSettingsSecretUpdate, error) {
	var edit struct {
		Action string `json:"action"`
		Secret string `json:"secret"`
	}
	if err := protocol.DecodeObject(body, &edit, []string{"action"}, true); err != nil {
		return protocol.PluginSettingsSecretUpdate{}, hostInvalid("invalid credential edit")
	}
	switch edit.Action {
	case "keep", "clear":
		return protocol.PluginSettingsSecretUpdate{Kind: edit.Action}, nil
	case "set":
		secret, err := owner.native.secret(edit.Secret)
		return protocol.PluginSettingsSecretUpdate{Kind: "replace", Data: secret}, err
	}
	return protocol.PluginSettingsSecretUpdate{}, hostInvalid("invalid credential edit")
}

func (owner *Host) loadSettingSecret(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	identity, err := hostValue[string](arguments, 0)
	if err != nil {
		return nil, err
	}
	field, err := hostValue[string](arguments, 1)
	if err != nil {
		return nil, err
	}
	revision, err := hostValue[string](arguments, 2)
	if err != nil {
		return nil, err
	}
	scope := owner.scope()
	if scope.Surface.Kind != "settings" || revision != exactRevisionText(scope.Package.SettingsRevision) {
		return nil, hostInvalid("plugin configuration changed or settings surface required")
	}
	var version, loading uint64
	err = owner.native.update(ctx, func() error {
		draft := owner.native.state.secrets[identity]
		if draft == nil {
			return hostInvalid("credential draft is unavailable")
		}
		draft.loading++
		version, loading = draft.version, draft.loading
		return nil
	})
	if err != nil {
		return nil, err
	}
	read := func() error {
		command := protocol.Command{Kind: "read_plugin_settings", Data: protocol.CommandReadPluginSettingsPayload{Package: scope.Package}}
		_, err := owner.read(ctx, command)
		return err
	}
	if err := read(); err != nil {
		return nil, err
	}
	command, err := commandFrom("read_plugin_secret", map[string]any{"package": scope.Package, "field": field})
	if err != nil {
		return nil, err
	}
	request := owner.binding.Client.Prepare(command)
	output, err := owner.binding.Client.Execute(ctx, request)
	if err != nil {
		return nil, err
	}
	if err := read(); err != nil {
		return nil, err
	}
	if output.Kind != "plugin_secret" {
		return nil, hostInvalid("credential response expected")
	}
	body, _ := json.Marshal(output.Data)
	var secret *protocol.Secret
	if err := json.Unmarshal(body, &secret); err != nil {
		return nil, hostInvalid("credential response expected")
	}
	applied := false
	err = owner.native.update(ctx, func() error {
		draft := owner.native.state.secrets[identity]
		if draft == nil {
			return hostInvalid("credential draft is unavailable")
		}
		if draft.version != version || draft.loading != loading {
			return nil
		}
		draft.value, draft.imported = "", ""
		if secret != nil {
			draft.value = secret.Expose()
		}
		applied = true
		return nil
	})
	if err != nil {
		return nil, err
	}
	return hostJSON(applied)
}

func (owner *Host) searchModule() HostModule {
	return HostModule{Name: "sailry/search", Exports: []HostExport{
		hostSync("prepareSettings", func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			input, err := hostValue[map[string]json.RawMessage](arguments, 0)
			if err != nil {
				return nil, err
			}
			key, err := owner.secretEdit(input["key"])
			if err != nil {
				return nil, err
			}
			var basic struct {
				Action   string `json:"action"`
				Username string `json:"username"`
				Password string `json:"password"`
			}
			if protocol.DecodeObject(input["basic"], &basic, []string{"action"}, true) != nil {
				return nil, hostInvalid("invalid Basic authentication edit")
			}
			update := protocol.PluginSettingsSecretUpdate{Kind: basic.Action}
			switch basic.Action {
			case "keep", "clear":
			case "set":
				username, err := owner.native.secret(basic.Username)
				if err != nil {
					return nil, err
				}
				password, err := owner.native.secret(basic.Password)
				if err != nil {
					return nil, err
				}
				if username.Expose() == "" || strings.Contains(username.Expose(), ":") || password.Expose() == "" {
					return nil, hostInvalid("invalid Basic authentication")
				}
				update.Kind, update.Data = "replace", protocol.NewSecret(username.Expose()+":"+password.Expose())
			default:
				return nil, hostInvalid("invalid Basic authentication edit")
			}
			var driver string
			if json.Unmarshal(input["driver"], &driver) != nil {
				return nil, hostInvalid("invalid search driver")
			}
			return owner.prepareData(ctx, "save_search_settings", map[string]any{
				"settings": input["settings"], "credentials": map[string]any{driver: map[string]any{"key": key, "basic": update}},
			})
		}),
	}}
}
