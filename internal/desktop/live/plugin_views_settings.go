package live

import (
	"context"
	"errors"
	"slices"
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginSchemaSettings struct {
	owner     *Runtime
	source    protocol.PluginReference
	reference protocol.PluginReference
	locale    string
	ctx       context.Context
	cancel    context.CancelFunc
	state     protocol.PluginSettingsState
	fields    []viewmodel.Setting
	secrets   map[string]int
	loading   bool
	pending   bool
	failure   error
}

func (r *Runtime) renderPluginSettings(view *ui.Context, entry pluginSettingsEntry) {
	page := entry.info.Extension.SettingsPage
	render := func() {
		if page.EnabledControl {
			enabled := entry.info.Summary.Enabled
			kit.Card(view, func() {
				kit.Row(view, "plugin-enabled", i18n.T(view, "plugins_enabled"), "", func() {
					ui.Switch(view, &enabled).Label(i18n.T(view, "plugins_enabled")).Disabled(!entry.owner.controller.Connected()).
						OnChange(func() { entry.owner.togglePackage(entry.info.Summary.Name, enabled) })
				})
			})
		}
		if page.Entry == nil {
			if entry.info.Settings != nil {
				r.renderPluginSchema(view, entry)
			} else {
				kit.Card(view, func() {
					kit.Row(view, "plugin-node", i18n.T(view, "project_host"), "", func() {
						ui.Text(view, i18n.T(view, entry.owner.packageHostLabel())).FontSize(14).TextColor(theme.Of(view).MutedForeground)
					})
				})
			}
			return
		}
		mounted := r.ensurePluginView(view, entry.owner, entry.info, entry.scope, "settings", nil)
		if !r.renderMountedPlugin(view, mounted) {
			ui.Box(view).FillWidth()
		}
	}
	if page.FullWidth {
		ui.Column(view).Fill().MinWidth(0).MinHeight(0).Children(render)
		return
	}
	description := stringValue(entry.info.Summary.Description)
	if entry.info.Extension.Description != nil {
		description = pluginNavigation(*entry.info.Extension.Description, r.packageLocale())
	}
	kit.Page(view, entry.section, pluginNavigation(page.Navigation, r.packageLocale()), description, render)
}

func rootPluginSchemaCurrent(root *Runtime, section settings.Section, schema *pluginSchemaSettings) bool {
	if root.pluginViews == nil || root.pluginViews.closed || root.pluginViews.schemas[section] != schema ||
		schema.ctx.Err() != nil || schema.locale != root.packageLocale() {
		return false
	}
	entry, exists := root.pluginViews.settings[section]
	return exists && entry.owner == schema.owner &&
		(entry.scope.Package == schema.reference || entry.scope.Package == schema.source)
}

func (r *Runtime) closePluginSchema(section settings.Section, schema *pluginSchemaSettings) {
	schema.cancel()
	for index := range schema.fields {
		if schema.fields[index].Kind == "secret" {
			schema.fields[index].Value = ""
		}
	}
	delete(r.pluginViews.schemas, section)
}

func (r *Runtime) ensurePluginSchema(view *ui.Context, entry pluginSettingsEntry) *pluginSchemaSettings {
	root := r.localRuntime()
	schema := root.pluginViews.schemas[entry.section]
	if schema != nil && rootPluginSchemaCurrent(root, entry.section, schema) {
		return schema
	}
	if schema != nil {
		root.closePluginSchema(entry.section, schema)
	}
	if !entry.owner.controller.Connected() {
		return nil
	}
	ctx, cancel := context.WithCancel(entry.owner.ctx)
	schema = &pluginSchemaSettings{
		owner: entry.owner, source: entry.scope.Package, reference: entry.scope.Package, locale: root.packageLocale(),
		ctx: ctx, cancel: cancel, secrets: map[string]int{}, loading: true,
	}
	root.pluginViews.schemas[entry.section] = schema
	if !entry.owner.async(func() {
		output, err := entry.owner.controller.Do(ctx, protocol.Command{Kind: "read_plugin_settings", Data: protocol.CommandReadPluginSettingsPayload{Package: schema.reference}})
		state, valid := detail[protocol.PluginSettingsState](output.Data)
		if err == nil && (!valid || output.Kind != "plugin_settings" || state.Package != schema.reference) {
			err = errors.New("unexpected plugin settings response")
		}
		var models protocol.PluginModelsCatalog
		if err == nil {
			for _, property := range entry.info.Settings.Properties {
				if !property.Model {
					continue
				}
				output, err = entry.owner.controller.Do(ctx, protocol.Command{Kind: "list_plugin_models"})
				models, valid = detail[protocol.PluginModelsCatalog](output.Data)
				if err == nil && (!valid || output.Kind != "plugin_models") {
					err = errors.New("unexpected plugin model catalog response")
				}
				break
			}
		}
		root.enqueue(func(current *ui.Context) {
			if !rootPluginSchemaCurrent(root, entry.section, schema) {
				return
			}
			schema.loading, schema.failure = false, err
			if err != nil {
				entry.owner.packageReport(current, err)
				return
			}
			schema.state = state
			schema.fields = packageSettingsFields(entry.info.Settings, state, schema.locale, models)
		})
	}) {
		root.closePluginSchema(entry.section, schema)
		return nil
	}
	return schema
}

func (r *Runtime) renderPluginSchema(view *ui.Context, entry pluginSettingsEntry) {
	schema := r.ensurePluginSchema(view, entry)
	if schema == nil {
		return
	}
	if schema.loading {
		kit.Busy(view)
		return
	}
	if schema.failure != nil {
		kit.IconButton(view.Key("retry"), icons.RotateCw, i18n.T(view, "chat_retry"), 28, false).
			Disabled(!entry.owner.controller.Connected()).OnClick(func() {
			r.closePluginSchema(entry.section, schema)
		})
		return
	}
	disabled := schema.pending || !entry.owner.controller.Connected()
	for index := range schema.fields {
		field := &schema.fields[index]
		label := field.Title
		kit.Field(view, field.Name, label, "", func() {
			switch field.Kind {
			case "bool":
				value := field.Value == "true"
				if ui.Switch(view, &value).Label(label).Disabled(disabled).Changed() {
					field.Value = strconv.FormatBool(value)
				}
			case "choice":
				control, _ := kit.Select(view.Key(field.Name), "value", label, &field.Value, field.Options, disabled)
				control.FillWidth()
			case "model":
				// The models the field offers, by their names.
				models := kit.ModelsOffered(entry.owner.modelOptions(), field.Options)
				kit.ModelSelect(view.Key(field.Name), kit.ModelSelectProps{Label: label, Placeholder: i18n.T(view, "composer_select_model"),
					Models: models, Model: field.Value, Disabled: disabled, OnChange: func(id, _ string) { field.Value = id }}).FillWidth()
			case "secret":
				action := schema.secrets[field.Name]
				if field.Set {
					kit.Segmented(view, field.Name, &action, []string{
						i18n.T(view, "plugins_settings_keep"), i18n.T(view, "plugins_settings_replace"), i18n.T(view, "plugins_settings_clear"),
					}, kit.SegmentedProps{Block: true}).Label(label).Disabled(disabled)
					schema.secrets[field.Name] = action
				}
				if !field.Set || action == 1 {
					ui.TextInput(view.Key(field.Name), &field.Value).Password().Label(label).FillWidth().Disabled(disabled)
				} else {
					key := "plugins_settings_configured"
					if action == 2 {
						key = "plugins_settings_clear_effect"
					}
					ui.Text(view, i18n.T(view, key)).FontSize(14).TextColor(theme.Of(view).MutedForeground)
				}
			default:
				ui.TextInput(view.Key(field.Name), &field.Value).Label(label).FillWidth().Disabled(disabled)
			}
		})
	}
	// PrimaryBusy also draws the label, which the bare Primary left out.
	kit.PrimaryBusy(view.Key("save"), i18n.T(view, "settings_save"), !entry.owner.controller.Connected(), schema.pending).OnClick(func() {
		if schema.pending || !entry.owner.controller.Connected() || !rootPluginSchemaCurrent(r.localRuntime(), entry.section, schema) {
			return
		}
		schema.pending = true
		entry.owner.packages.settings[entry.info.Summary.Name] = schema.state
		entry.owner.savePackageSettings(entry.info.Summary.Name, slices.Clone(schema.fields), schema.secrets, func(saved bool) {
			if !rootPluginSchemaCurrent(r.localRuntime(), entry.section, schema) {
				return
			}
			schema.pending = false
			if !saved {
				return
			}
			schema.source = schema.reference
			schema.state = entry.owner.packages.settings[entry.info.Summary.Name]
			schema.reference = schema.state.Package
			for index := range schema.fields {
				field := &schema.fields[index]
				if field.Kind == "secret" {
					field.Value, field.Set = "", slices.Contains(schema.state.Keepable, field.Name)
				}
			}
			clear(schema.secrets)
		})
	})
}
