package live

import (
	"context"
	"strings"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Anonymous feature counters. The window reports that a feature was used,
// as an identifier of protocol.FeatureUse's closed list, to the Node on this
// device, which counts it only while usage metrics are on. Whatever host a
// conversation runs on, the count goes to the local Node alone. Nothing here
// reads a draft, a title, a path or an identifier.

var pageFeatures = map[shell.Page]protocol.FeatureUse{
	shell.ConversationPage:  protocol.FeatureUsePageConversation,
	shell.ActivityPage:      protocol.FeatureUsePageActivity,
	shell.FilesPage:         protocol.FeatureUsePageFiles,
	shell.GitPage:           protocol.FeatureUsePageGit,
	shell.MarketPage:        protocol.FeatureUsePageMarket,
	shell.HostPage:          protocol.FeatureUsePageHost,
	shell.SettingsPage:      protocol.FeatureUsePageSettings,
	shell.NotificationsPage: protocol.FeatureUsePageNotifications,
	shell.ProjectPage:       protocol.FeatureUsePageProject,
}

var sectionFeatures = map[settings.Section]protocol.FeatureUse{
	settings.General:      protocol.FeatureUseSettingsGeneral,
	settings.Appearance:   protocol.FeatureUseSettingsAppearance,
	settings.Conversation: protocol.FeatureUseSettingsConversation,
	settings.Shortcuts:    protocol.FeatureUseSettingsShortcuts,
	settings.Dictation:    protocol.FeatureUseSettingsDictation,
	settings.Providers:    protocol.FeatureUseSettingsProviders,
	settings.Connectors:   protocol.FeatureUseSettingsConnectors,
	settings.Teams:        protocol.FeatureUseSettingsTeams,
	settings.Plugins:      protocol.FeatureUseSettingsPlugins,
	settings.Skills:       protocol.FeatureUseSettingsSkills,
	settings.Knowledge:    protocol.FeatureUseSettingsKnowledge,
	settings.Decision:     protocol.FeatureUseSettingsDecision,
	settings.Terminal:     protocol.FeatureUseSettingsTerminal,
	settings.Connections:  protocol.FeatureUseSettingsConnections,
	settings.Hooks:        protocol.FeatureUseSettingsHooks,
	settings.Runtime:      protocol.FeatureUseSettingsRuntime,
	settings.OCR:          protocol.FeatureUseSettingsOCR,
	settings.Permissions:  protocol.FeatureUseSettingsPermissions,
	settings.Backup:       protocol.FeatureUseSettingsBackup,
	settings.About:        protocol.FeatureUseSettingsAbout,
}

var panelFeatures = map[panel.Kind]protocol.FeatureUse{
	panel.Browser:   protocol.FeatureUsePanelBrowser,
	panel.Terminal:  protocol.FeatureUsePanelTerminal,
	panel.Files:     protocol.FeatureUsePanelFiles,
	panel.Changes:   protocol.FeatureUsePanelGit,
	panel.History:   protocol.FeatureUsePanelHistory,
	panel.Documents: protocol.FeatureUsePanelDocuments,
	panel.Artifact:  protocol.FeatureUsePanelArtifact,
	panel.Subagent:  protocol.FeatureUsePanelSubagent,
}

var modeFeatures = map[composer.Mode]protocol.FeatureUse{
	composer.Code:     protocol.FeatureUseComposerModeCode,
	composer.Plan:     protocol.FeatureUseComposerModePlan,
	composer.Flagship: protocol.FeatureUseComposerModeFlagship,
}

var permissionFeatures = map[composer.Permission]protocol.FeatureUse{
	composer.AskFirst:      protocol.FeatureUseComposerPermissionAsk,
	composer.ProjectAccess: protocol.FeatureUseComposerPermissionProject,
	composer.FullAccess:    protocol.FeatureUseComposerPermissionFull,
}

// savedFeatures names the settings family a dispatched command saves.
var savedFeatures = map[string]protocol.FeatureUse{
	"put_provider": protocol.FeatureUseSaveProvider, "save_provider": protocol.FeatureUseSaveProvider, "remove_provider": protocol.FeatureUseSaveProvider,
	"put_team": protocol.FeatureUseSaveTeam, "remove_team": protocol.FeatureUseSaveTeam, "save_team_settings": protocol.FeatureUseSaveTeam,
	"put_role": protocol.FeatureUseSaveRole, "remove_role": protocol.FeatureUseSaveRole,
	"update_connection": protocol.FeatureUseSaveConnector, "set_connection_enabled": protocol.FeatureUseSaveConnector,
	"disconnect_connection": protocol.FeatureUseSaveConnector, "remove_connection": protocol.FeatureUseSaveConnector,
	"install_mcp": protocol.FeatureUseSaveMCP, "set_plugin_mcp_allowed": protocol.FeatureUseSaveMCP, "save_plugin_mcp": protocol.FeatureUseSaveMCP,
	"save_plugin_settings": protocol.FeatureUseSavePlugin, "set_plugin_enabled": protocol.FeatureUseSavePlugin,
	"save_hooks":                  protocol.FeatureUseSaveHooks,
	"save_backup_settings":        protocol.FeatureUseSaveBackup,
	"save_terminal_settings":      protocol.FeatureUseSaveTerminal,
	"save_media_settings":         protocol.FeatureUseSaveMedia,
	"save_power_settings":         protocol.FeatureUseSavePower,
	"save_telemetry_settings":     protocol.FeatureUseSaveTelemetry,
	"save_network_settings":       protocol.FeatureUseSaveNetwork,
	"save_sandbox_settings":       protocol.FeatureUseSaveSandbox,
	"save_search_settings":        protocol.FeatureUseSaveSearch,
	"save_compaction_settings":    protocol.FeatureUseSaveCompaction,
	"save_python_settings":        protocol.FeatureUseSavePython,
	"save_local_model_settings":   protocol.FeatureUseSaveLocalModels,
	"save_code_index_settings":    protocol.FeatureUseSaveCodeIndex,
	"create_knowledge_collection": protocol.FeatureUseSaveKnowledge, "update_knowledge_collection": protocol.FeatureUseSaveKnowledge,
	"delete_knowledge_collection": protocol.FeatureUseSaveKnowledge,
	"put_credential":              protocol.FeatureUseSaveCredential,
	"save_ssh":                    protocol.FeatureUseSaveSSH, "remove_ssh": protocol.FeatureUseSaveSSH,
	"save_database": protocol.FeatureUseSaveDatabase, "remove_database": protocol.FeatureUseSaveDatabase,
	"set_defaults": protocol.FeatureUseSaveDefaults,
}

// featureUsage is what the last frame showed.
type featureUsage struct {
	started bool
	shell   shell.Usage
	section settings.Section
	theme   string
	// dictated are the composers whose current draft dictation was started in.
	dictated map[*composer.State]bool
}

// count sends the uses to the local Node. The Node counts or drops them by
// consent; a failed count is not retried and not shown. The demo sends
// nothing to a Node, so what is used in it is not counted.
func (r *Runtime) count(features ...protocol.FeatureUse) {
	root := r.localRuntime()
	if len(features) == 0 || root.App.Preview || root.App.DemoOn() {
		return
	}
	root.async(func() {
		for _, feature := range features {
			_ = root.controller.CountFeature(root.ctx, feature)
		}
	})
}

// countSaved observes a client's dispatched commands and counts, on the
// local Node, the settings family a command saves. It sees the command's
// kind alone. The clients outlive any one window, so the count does not
// depend on a window's runtime.
func countSaved(local *client.Client) func(kind string) {
	return func(kind string) {
		if feature, saved := savedFeatures[kind]; saved {
			go func() { _ = local.CountFeature(context.Background(), feature) }()
		}
	}
}

// countFrame compares the frame with the one before it. The first frame is
// only the baseline: what a window opens on and restores was not chosen now.
func (r *Runtime) countFrame() {
	last := &r.usage
	current, section, selected := r.App.Usage(), r.App.Settings.Section, r.App.Theme
	opened := r.App.Panel.TakeOpened()
	var features []protocol.FeatureUse
	if last.started {
		paged := current.Page != last.shell.Page || current.PluginPage != last.shell.PluginPage
		if paged {
			if current.Page == shell.PluginPage {
				features = append(features, r.pluginPageFeature(current.PluginPage))
			} else if feature, known := pageFeatures[current.Page]; known {
				features = append(features, feature)
			}
		}
		if current.Page == shell.SettingsPage && (paged || section != last.section) {
			feature, known := sectionFeatures[section]
			if !known {
				feature = protocol.FeatureUseSettingsPluginPage
			}
			// Experimental is a core page the feature list does not name.
			if section != settings.Experimental {
				features = append(features, feature)
			}
		}
		if current.Search && !last.shell.Search {
			features = append(features, protocol.FeatureUseSearchPalette)
		}
		if current.ModelPicker && !last.shell.ModelPicker {
			features = append(features, protocol.FeatureUseComposerModelPicker)
		}
		for range current.Splits - last.shell.Splits {
			features = append(features, protocol.FeatureUsePaneSplit)
		}
		for _, kind := range opened {
			if feature, known := panelFeatures[kind]; known {
				features = append(features, feature)
			}
		}
		if selected != last.theme {
			if selected == "" {
				features = append(features, protocol.FeatureUseThemeBuiltin)
			} else {
				features = append(features, protocol.FeatureUseThemePackage)
			}
		}
	}
	last.started, last.shell, last.section, last.theme = true, current, section, selected
	r.count(features...)
}

// pluginPageFeature names a package page's package only when the Node that
// holds it says the application shipped it. The local Node then accepts the
// name only when it ships that package itself; any other package's page is
// "third_party".
func (r *Runtime) pluginPageFeature(id string) protocol.FeatureUse {
	if r.pluginViews != nil {
		if entry, found := r.pluginViews.navigation[id]; found && entry.info.Origin != nil && entry.info.Origin.Kind == "bundled" &&
			!strings.ContainsAny(entry.info.Summary.Name, ": ") {
			return protocol.FeatureUsePagePluginOfficial + protocol.FeatureUse(entry.info.Summary.Name)
		}
	}
	return protocol.FeatureUsePagePluginThirdParty
}

// countSubmit counts a composer submission: once, and once for each flag,
// its mode and its permission level. The counts are marginal; no row ties
// one flag to another.
func (x *conversationActions) countSubmit(m composer.Message, guide, queue bool) {
	root := x.r.localRuntime()
	features := []protocol.FeatureUse{protocol.FeatureUseComposerSubmit}
	if len(m.Attachments) > 0 {
		features = append(features, protocol.FeatureUseComposerHasAttachment)
	}
	if len(m.References) > 0 {
		features = append(features, protocol.FeatureUseComposerHasReference)
	}
	if root.usage.dictated[x.comp] {
		features = append(features, protocol.FeatureUseComposerDictated)
		delete(root.usage.dictated, x.comp)
	}
	if queue {
		features = append(features, protocol.FeatureUseComposerQueued)
	}
	if guide {
		features = append(features, protocol.FeatureUseComposerGuided)
	}
	if feature, known := modeFeatures[m.Mode]; known {
		features = append(features, feature)
	}
	if feature, known := permissionFeatures[m.Permission]; known {
		features = append(features, feature)
	}
	root.count(features...)
}

// countDictation counts a dictation that started and marks its draft.
func (x *conversationActions) countDictation() {
	root := x.r.localRuntime()
	if root.usage.dictated == nil {
		root.usage.dictated = map[*composer.State]bool{}
	}
	root.usage.dictated[x.comp] = true
	root.count(protocol.FeatureUseDictationStart)
}

// CountReadAloud counts a reading that started, by the voice that reads it.
func (r *Runtime) CountReadAloud(model bool) {
	if model {
		r.count(protocol.FeatureUseReadAloudModelVoice)
	} else {
		r.count(protocol.FeatureUseReadAloudOSVoice)
	}
}
