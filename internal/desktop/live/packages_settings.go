package live

import (
	"encoding/json"
	"errors"
	"slices"
	"sort"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func packageSettingsFields(schema *protocol.PluginSettingsSchema, state protocol.PluginSettingsState, locale string, catalogs ...protocol.PluginModelsCatalog) []viewmodel.Setting {
	if schema == nil {
		return nil
	}
	names := make([]string, 0, len(schema.Properties))
	for name := range schema.Properties {
		names = append(names, name)
	}
	sort.Strings(names)
	fields := make([]viewmodel.Setting, 0, len(names))
	for _, name := range names {
		property := schema.Properties[name]
		field := viewmodel.Setting{Name: name, Title: name, Kind: "string", Required: slices.Contains(schema.Required, name)}
		if property.Title != nil {
			field.Title = *property.Title
		}
		// A package names a setting by its field; a title is the older key.
		if title := schema.Locales[locale][name]; title != "" {
			field.Title = title
		} else if title := schema.Locales[locale][field.Title]; title != "" {
			field.Title = title
		}
		value := state.Values[name]
		if len(value) == 0 && property.Default != nil {
			value = *property.Default
		}
		switch property.Kind {
		case protocol.PluginSettingsKindBoolean:
			field.Kind = "bool"
			if len(value) == 0 {
				value = json.RawMessage("false")
			}
		case protocol.PluginSettingsKindNumber, protocol.PluginSettingsKindInteger:
			field.Kind = "number"
		}
		if property.Secret != nil {
			field.Kind, field.Set = "secret", slices.Contains(state.Keepable, name)
		} else {
			field.Value = packageSettingText(value)
		}
		if property.Choices != nil {
			field.Kind = "choice"
			for _, choice := range *property.Choices {
				field.Options = append(field.Options, packageSettingText(choice))
			}
		}
		if property.Effort != nil && len(catalogs) != 0 {
			// The levels follow the model field the package names.
			field.Kind, field.Model, field.Options = "effort", *property.Effort, nil
			field.Efforts = map[string][]string{}
			for _, model := range catalogs[0].Models {
				for _, effort := range model.Efforts {
					field.Efforts[model.ID] = append(field.Efforts[model.ID], effortName(effort))
				}
			}
		}
		if property.Model && len(catalogs) != 0 {
			// A model field's options are model IDs: the forms draw them
			// with kit.ModelSelect, by their names.
			field.Kind = "model"
			field.Options = nil
			for _, model := range catalogs[0].Models {
				field.Options = append(field.Options, model.ID)
			}
			if field.Value != "" && !slices.Contains(field.Options, field.Value) {
				field.Options = append(field.Options, field.Value)
			}
		}
		fields = append(fields, field)
	}
	return fields
}

func packageSettingText(value json.RawMessage) string {
	if len(value) == 0 {
		return ""
	}
	var text string
	if json.Unmarshal(value, &text) == nil {
		return text
	}
	return string(value)
}

func (r *Runtime) configurePackage(id string) {
	name := r.packageID(id)
	info, exists := r.packages.infos[name]
	if !exists || info.Settings == nil {
		return
	}
	reference := packageReference(info.Summary)
	root := r.localRuntime()
	root.packages.configuration++
	generation := root.packages.configuration
	// The dialog opens where it was asked for: the settings section that
	// asked, or the page, as a package's own page from its header.
	page, section := root.App.Page, root.App.Settings.Section
	modelsNeeded := false
	for _, property := range info.Settings.Properties {
		modelsNeeded = modelsNeeded || property.Model || property.Effort != nil
	}
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_plugin_settings", Data: protocol.CommandReadPluginSettingsPayload{Package: reference}})
		state, valid := detail[protocol.PluginSettingsState](out.Data)
		if err == nil && (!valid || out.Kind != "plugin_settings" || state.Package != reference) {
			err = errors.New("unexpected plugin settings response")
		}
		var models protocol.PluginModelsCatalog
		if err == nil && modelsNeeded {
			out, err = r.controller.Do(r.ctx, protocol.Command{Kind: "list_plugin_models"})
			models, valid = detail[protocol.PluginModelsCatalog](out.Data)
			if err == nil && (!valid || out.Kind != "plugin_models") {
				err = errors.New("unexpected plugin model catalog response")
			}
		}
		r.enqueue(func(c *ui.Context) {
			if err != nil {
				r.packageReport(c, err)
				return
			}
			r.packages.settings[name] = state
			if root.packages.configuration == generation && root.App.Page == page &&
				(page != shell.SettingsPage || root.packageOwner() == r && root.App.Settings.Section == section) {
				root.packages.configurer = r
				r.App.Settings.OpenPackageConfiguration(name, packageView(info, r.packageLocale()).Name, packageSettingsFields(info.Settings, state, r.packageLocale(), models), packageSettingsTabs(info.Settings, r.packageLocale())...)
			}
		})
	})
}

func (r *Runtime) savePackageSettings(id string, fields []viewmodel.Setting, secretActions map[string]int, done func(bool)) {
	name := r.packageID(id)
	info, exists := r.packages.infos[name]
	base, captured := r.packages.settings[name]
	if !exists || !captured || info.Settings == nil {
		done(false)
		return
	}
	input := protocol.CommandSavePluginSettingsPayload{Package: base.Package, Values: map[string]json.RawMessage{}, Secrets: map[string]protocol.PluginSettingsSecretUpdate{}}
	for _, field := range fields {
		property, known := info.Settings.Properties[field.Name]
		if !known {
			continue
		}
		if field.Value == "" && !field.Required && property.Kind != protocol.PluginSettingsKindString {
			continue
		}
		if property.Secret != nil {
			action := "keep"
			switch secretActions[field.Name] {
			case 1:
				action = "replace"
			case 2:
				action = "clear"
			}
			if !field.Set && action == "keep" {
				action = "clear"
				if field.Value != "" {
					action = "replace"
				}
			}
			update := protocol.PluginSettingsSecretUpdate{Kind: action}
			if action == "replace" {
				update.Data = protocol.NewSecret(field.Value)
			}
			input.Secrets[field.Name] = update
			continue
		}
		var encoded []byte
		var err error
		switch property.Kind {
		case protocol.PluginSettingsKindBoolean:
			var value bool
			value, err = strconv.ParseBool(field.Value)
			if err == nil {
				encoded, err = json.Marshal(value)
			}
		case protocol.PluginSettingsKindInteger:
			var value int64
			value, err = strconv.ParseInt(field.Value, 10, 64)
			if err == nil {
				encoded, err = json.Marshal(value)
			}
		case protocol.PluginSettingsKindNumber:
			var value float64
			value, err = strconv.ParseFloat(field.Value, 64)
			if err == nil {
				encoded, err = json.Marshal(value)
			}
		default:
			encoded, err = json.Marshal(field.Value)
		}
		if err != nil {
			r.enqueue(func(c *ui.Context) { r.packageReport(c, err); done(false) })
			return
		}
		input.Values[field.Name] = encoded
	}
	r.packageCommand(protocol.Command{Kind: "save_plugin_settings", Data: input}, func(out protocol.Output, ok bool) {
		if ok {
			if state, valid := detail[protocol.PluginSettingsState](out.Data); valid {
				r.packages.settings[name] = state
				info.Summary.SettingsRevision = state.Package.SettingsRevision
				r.packages.infos[name] = info
				if r.authorizations != nil && r.authorizations.mcpConfiguring == name {
					r.authorizations.mcpConfiguring = ""
					r.authorizations.packages[name] = info
					r.reconnectMCP("mcp:" + name)
					r.readConnectorPackages()
				}
			}
		}
		done(ok)
	})
}

var pythonIndexURLs = []string{"", "https://mirrors.aliyun.com/pypi/simple/", "https://mirrors.tencent.com/pypi/simple/", "https://pypi.tuna.tsinghua.edu.cn/simple/"}

func pythonView(index *string) settings.Python {
	if index == nil || strings.TrimRight(*index, "/") == "https://pypi.org/simple" {
		return settings.Python{}
	}
	for number, endpoint := range pythonIndexURLs {
		if strings.TrimRight(*index, "/") == strings.TrimRight(endpoint, "/") {
			return settings.Python{Index: number}
		}
	}
	return settings.Python{Index: len(pythonIndexURLs), Custom: *index}
}

func (r *Runtime) readPython() {
	r.packages.pythonRead = true
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_python_settings"})
		r.enqueue(func(c *ui.Context) {
			info, valid := detail[protocol.PythonInfo](out.Data)
			if err == nil && (!valid || out.Kind != "python_settings") {
				err = errors.New("unexpected Python settings response")
			}
			if err != nil {
				r.packages.pythonReadFailed = true
				r.packageReport(c, err)
				return
			}
			r.packages.pythonRevision = info.Settings.Revision
			r.packages.pythonVersion = info.Version
			target := &r.packages.pythonDraft
			if !target.Pending && target.Draft == target.Saved {
				target.Draft = pythonView(info.Settings.Index)
			}
			target.Saved, target.Revision = pythonView(info.Settings.Index), info.Settings.Revision
			if r.localRuntime().packageOwner() != r {
				return
			}
			m := &r.App.Settings.Python
			if !m.Pending && m.Draft == m.Saved {
				m.Draft = pythonView(info.Settings.Index)
			}
			m.Saved, m.Revision = pythonView(info.Settings.Index), info.Settings.Revision
			r.App.Settings.PythonVersion = info.Version
			r.packages.pythonDraft = *m
		})
	})
}

func (r *Runtime) savePython() {
	field := &r.App.Settings.Python
	if field.Pending {
		return
	}
	input := protocol.PythonSettings{Revision: field.Revision}
	if field.Draft.Index == len(pythonIndexURLs) {
		input.Index = optionalPackageText(field.Draft.Custom)
	} else if field.Draft.Index > 0 && field.Draft.Index < len(pythonIndexURLs) {
		value := pythonIndexURLs[field.Draft.Index]
		input.Index = &value
	}
	field.Pending = true
	r.packages.pythonDraft = *field
	r.packageCommand(protocol.Command{Kind: "save_python_settings", Data: input}, func(out protocol.Output, ok bool) {
		if r.localRuntime().packageOwner() != r {
			field = &r.packages.pythonDraft
		}
		field.Pending = false
		if ok {
			if info, valid := detail[protocol.PythonInfo](out.Data); valid {
				field.Saved, field.Draft, field.Revision = pythonView(info.Settings.Index), pythonView(info.Settings.Index), info.Settings.Revision
				r.packages.pythonVersion = info.Version
				if r.localRuntime().packageOwner() == r {
					r.App.Settings.PythonVersion = info.Version
				}
				r.packages.pythonRevision = info.Settings.Revision
			}
		}
		r.packages.pythonDraft = *field
	})
}

// packageSettingsTabs are the package's settings pages (x-sailry-tabs),
// titled in the locale as its settings' own labels are.
func packageSettingsTabs(schema *protocol.PluginSettingsSchema, locale string) []viewmodel.SettingsTab {
	if schema == nil {
		return nil
	}
	tabs := make([]viewmodel.SettingsTab, 0, len(schema.Tabs))
	for _, tab := range schema.Tabs {
		view := viewmodel.SettingsTab{ID: tab.ID, Title: tab.Title, Fields: tab.Fields}
		if title := schema.Locales[locale][tab.ID]; title != "" {
			view.Title = title
		}
		if tab.Enabled != nil {
			view.Enabled = *tab.Enabled
		}
		tabs = append(tabs, view)
	}
	return tabs
}
