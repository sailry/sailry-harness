package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// ConfigurationItem is one entry of a configuration import's report: its
// settings Area, its Name there ("" for a single setting), its Action
// (added, updated or skipped) and the Node's Reason for a skipped one.
type ConfigurationItem struct {
	Area, Name, Action, Reason string
}

// ConfigurationActions copy a Node's configuration through a file. Their
// outcomes are toasts.
type ConfigurationActions struct {
	// Export saves the configuration to a file the user chooses.
	Export func()
	// Pick reads a configuration file the user chooses.
	Pick func(selected func(document []byte))
	// Import sends a picked document. A preview changes nothing and
	// reports what an import would do. done receives the report, or with
	// ok false whether the request is kept for Retry.
	Import func(document []byte, preview bool, done func(items []ConfigurationItem, ok, retry bool))
	// Forget drops an import kept for Retry when its dialog closes.
	Forget func()
}

// configurationImport is the import dialog: the picked document and its
// preview's report. retry is whether the import sent last is unconfirmed.
type configurationImport struct {
	open, retry bool
	document    []byte
	items       []ConfigurationItem
}

// configurationAreas are the report's areas by their settings copy.
var configurationAreas = map[string]string{
	"providers": "settings_providers", "search": "configuration_area_search", "media": "settings_media",
	"compaction": "configuration_area_compaction", "team": "teams_concurrency", "roles": "teams_roles",
	"teams": "settings_teams", "sandbox": "settings_command_sandbox", "power": "power_prevent_sleep",
	"python": "runtime_python", "instructions": "settings_custom_prompt", "plugins": "settings_plugins",
}

// configurationActions are the report's actions by their copy.
var configurationActions = map[string]string{
	"added": "configuration_added", "updated": "configuration_updated", "skipped": "configuration_skipped",
}

// configurationArea is an area's label; one the Desktop does not know is
// shown as the Node named it.
func configurationArea(c *ui.Context, area string) string {
	if key, ok := configurationAreas[area]; ok {
		return i18n.T(c, key)
	}
	return area
}

// ConfigurationGroups are a report's items by area, in the order each area
// first appears.
func ConfigurationGroups(items []ConfigurationItem) (areas []string, grouped map[string][]ConfigurationItem) {
	grouped = map[string][]ConfigurationItem{}
	for _, item := range items {
		if _, seen := grouped[item.Area]; !seen {
			areas = append(areas, item.Area)
		}
		grouped[item.Area] = append(grouped[item.Area], item)
	}
	return areas, grouped
}

// configurationRows are the export and import entries of the Data card.
// The file holds plaintext API keys, which the export's row says once.
func configurationRows(c *ui.Context, m *Model) {
	a := m.ConfigurationActions
	t := func(k string) string { return i18n.T(c, k) }
	pending := m.Backup.Pending
	kit.Entry(c, "configuration-export", func() {
		kit.RowText(c, t("configuration_export"), t("configuration_export_description"))
	}, nil, func() {
		kit.TextActionBusy(c.Key("action"), t("backup_export"), pending, m.Busy[BusyBackup+"configuration-export"]).OnClick(func() {
			if a == nil {
				notice(c)
				return
			}
			a.Export()
		})
	})
	kit.Entry(c, "configuration-import", func() { ui.Text(c, t("configuration_import")).FontSize(14) }, nil, func() {
		kit.TextActionBusy(c.Key("action"), t("configuration_import_action"), pending, m.Busy[BusyBackup+"configuration-import"]).OnClick(func() {
			if a == nil {
				notice(c)
				return
			}
			a.Pick(m.previewConfiguration)
		})
	})
}

// previewConfiguration previews document's import and opens its dialog.
// Where the document came from, such as a picked file, is not the dialog's.
func (m *Model) previewConfiguration(document []byte) {
	m.ConfigurationActions.Import(document, true, func(items []ConfigurationItem, ok, _ bool) {
		if ok {
			m.configuration = configurationImport{open: true, document: document, items: items}
		}
	})
}

// configurationDialog is an import's preview: each entry by area, with
// what the import does to it and why a skipped one is left out. Import
// sends the same document; the dialog stays open until it is confirmed.
func configurationDialog(c *ui.Context, m *Model) {
	d, a := &m.configuration, m.ConfigurationActions
	if !d.open {
		// The dialog closed on an import that was not confirmed: its kept
		// request goes with it.
		if d.retry && a != nil && a.Forget != nil {
			a.Forget()
		}
		if d.document != nil {
			*d = configurationImport{}
		}
		return
	}
	if a == nil {
		return
	}
	p := theme.Of(c)
	t := func(k string) string { return i18n.T(c, k) }
	title := t("configuration_preview")
	action := t("configuration_import_action")
	if d.retry {
		action = t("plugins_retry")
	}
	busy := m.Busy[BusyBackup+"configuration-import"]
	kit.Dialog(c.Key("configuration"), &d.open, func() {
		kit.DialogHeader(c, title)
		if len(d.items) == 0 {
			kit.Result(c, kit.ResultProps{Icon: icons.Inbox, Title: t("configuration_empty"), Card: true})
		} else {
			areas, grouped := ConfigurationGroups(d.items)
			ui.Scroll(c.Key("report")).Grow(1).Shrink(1).Gap(16).Children(func() {
				for _, area := range areas {
					label := configurationArea(c, area)
					ui.Column(c.Key(area)).Gap(4).Children(func() {
						ui.Text(c, label).FontSize(13).FontWeight(600).TextColor(p.MutedForeground)
						kit.Card(c, func() {
							for i, item := range grouped[area] {
								name := item.Name
								if name == "" {
									name = label
								}
								kit.Entry(c, i, func() { kit.RowText(c, name, item.Reason) }, nil, func() {
									outcome := item.Action
									if key, ok := configurationActions[item.Action]; ok {
										outcome = t(key)
									}
									color := p.SurfaceForeground
									if item.Action == "skipped" {
										color = p.MutedForeground
									}
									ui.Text(c, outcome).FontSize(kit.RowFont).TextColor(color)
								})
							}
						})
					})
				}
			})
		}
		DialogFooter(c, action, func() { d.open = false }, func() {
			if busy {
				return
			}
			document := d.document
			a.Import(document, false, func(_ []ConfigurationItem, ok, retry bool) {
				if d.document == nil || string(d.document) != string(document) {
					return
				}
				if d.retry = retry; ok {
					*d = configurationImport{}
				}
			})
		}, busy)
	}).Width(min(600, width(c)-48)).Label(title)
}
