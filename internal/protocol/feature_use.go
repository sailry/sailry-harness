package protocol

import (
	"encoding/json"
	"slices"
	"strings"
)

// FeatureUse names one desktop feature whose uses are counted while usage
// metrics are on. The list is closed: an identifier carries no content, no
// user-chosen name and no session, project or host identity. The one open
// position is FeatureUsePluginPageOfficial, which the Node completes only
// with the name of a package it ships.
type FeatureUse string

const (
	FeatureUsePageConversation  FeatureUse = "page.conversation"
	FeatureUsePageActivity      FeatureUse = "page.activity"
	FeatureUsePageFiles         FeatureUse = "page.files"
	FeatureUsePageGit           FeatureUse = "page.git"
	FeatureUsePageMarket        FeatureUse = "page.market"
	FeatureUsePageHost          FeatureUse = "page.host"
	FeatureUsePageSettings      FeatureUse = "page.settings"
	FeatureUsePageNotifications FeatureUse = "page.notifications"
	FeatureUsePageProject       FeatureUse = "page.project"
	// A package's own page: a package the application ships is named, any
	// other is only "third_party".
	FeatureUsePagePluginThirdParty FeatureUse = "page.plugin_page.third_party"
	FeatureUsePagePluginOfficial   FeatureUse = "page.plugin_page.official:"

	FeatureUseSettingsGeneral      FeatureUse = "settings.section.general"
	FeatureUseSettingsAppearance   FeatureUse = "settings.section.appearance"
	FeatureUseSettingsConversation FeatureUse = "settings.section.conversation"
	FeatureUseSettingsShortcuts    FeatureUse = "settings.section.shortcuts"
	FeatureUseSettingsDictation    FeatureUse = "settings.section.dictation"
	FeatureUseSettingsProviders    FeatureUse = "settings.section.providers"
	FeatureUseSettingsConnectors   FeatureUse = "settings.section.connectors"
	FeatureUseSettingsTeams        FeatureUse = "settings.section.teams"
	FeatureUseSettingsPlugins      FeatureUse = "settings.section.plugins"
	FeatureUseSettingsSkills       FeatureUse = "settings.section.skills"
	FeatureUseSettingsKnowledge    FeatureUse = "settings.section.knowledge"
	FeatureUseSettingsDecision     FeatureUse = "settings.section.decision"
	FeatureUseSettingsTerminal     FeatureUse = "settings.section.terminal"
	FeatureUseSettingsConnections  FeatureUse = "settings.section.connections"
	FeatureUseSettingsHooks        FeatureUse = "settings.section.hooks"
	FeatureUseSettingsRuntime      FeatureUse = "settings.section.runtime"
	FeatureUseSettingsOCR          FeatureUse = "settings.section.ocr"
	FeatureUseSettingsPermissions  FeatureUse = "settings.section.permissions"
	FeatureUseSettingsBackup       FeatureUse = "settings.section.backup"
	FeatureUseSettingsAbout        FeatureUse = "settings.section.about"
	// A settings page a package contributes, never named.
	FeatureUseSettingsPluginPage FeatureUse = "settings.section.plugin_page"

	FeatureUsePanelBrowser   FeatureUse = "panel.tab.browser"
	FeatureUsePanelTerminal  FeatureUse = "panel.tab.terminal"
	FeatureUsePanelFiles     FeatureUse = "panel.tab.files"
	FeatureUsePanelGit       FeatureUse = "panel.tab.git"
	FeatureUsePanelHistory   FeatureUse = "panel.tab.history"
	FeatureUsePanelDocuments FeatureUse = "panel.tab.documents"
	FeatureUsePanelArtifact  FeatureUse = "panel.tab.artifact"
	FeatureUsePanelSubagent  FeatureUse = "panel.tab.subagent"

	FeatureUsePaneSplit FeatureUse = "panes.split"

	// One submission counts composer.submit, each flag that applies, its
	// mode and its permission level: marginal counts, never a combination.
	FeatureUseComposerSubmit            FeatureUse = "composer.submit"
	FeatureUseComposerHasAttachment     FeatureUse = "composer.submit.has_attachment"
	FeatureUseComposerHasReference      FeatureUse = "composer.submit.has_reference"
	FeatureUseComposerDictated          FeatureUse = "composer.submit.dictated"
	FeatureUseComposerQueued            FeatureUse = "composer.submit.queued"
	FeatureUseComposerGuided            FeatureUse = "composer.submit.guided"
	FeatureUseComposerEdited            FeatureUse = "composer.submit.edited"
	FeatureUseComposerModeCode          FeatureUse = "composer.mode.code"
	FeatureUseComposerModePlan          FeatureUse = "composer.mode.plan"
	FeatureUseComposerModeFlagship      FeatureUse = "composer.mode.flagship"
	FeatureUseComposerPermissionAsk     FeatureUse = "composer.permission.ask"
	FeatureUseComposerPermissionProject FeatureUse = "composer.permission.project"
	FeatureUseComposerPermissionFull    FeatureUse = "composer.permission.full"
	FeatureUseComposerModelPicker       FeatureUse = "composer.model_picker"

	FeatureUseSearchPalette       FeatureUse = "search.palette"
	FeatureUseReadAloudOSVoice    FeatureUse = "read_aloud.os_voice"
	FeatureUseReadAloudModelVoice FeatureUse = "read_aloud.model_voice"
	FeatureUseDictationStart      FeatureUse = "dictation.start"
	FeatureUseSessionExport       FeatureUse = "session.export"
	FeatureUseThemeBuiltin        FeatureUse = "theme.change.builtin"
	FeatureUseThemePackage        FeatureUse = "theme.change.package"

	// A settings family whose save was sent, never its values.
	FeatureUseSaveProvider    FeatureUse = "settings.save.provider"
	FeatureUseSaveTeam        FeatureUse = "settings.save.team"
	FeatureUseSaveRole        FeatureUse = "settings.save.role"
	FeatureUseSaveConnector   FeatureUse = "settings.save.connector"
	FeatureUseSaveMCP         FeatureUse = "settings.save.mcp"
	FeatureUseSavePlugin      FeatureUse = "settings.save.plugin"
	FeatureUseSaveHooks       FeatureUse = "settings.save.hooks"
	FeatureUseSaveBackup      FeatureUse = "settings.save.backup"
	FeatureUseSaveTerminal    FeatureUse = "settings.save.terminal"
	FeatureUseSaveMedia       FeatureUse = "settings.save.media"
	FeatureUseSavePower       FeatureUse = "settings.save.power"
	FeatureUseSaveTelemetry   FeatureUse = "settings.save.telemetry"
	FeatureUseSaveNetwork     FeatureUse = "settings.save.network"
	FeatureUseSaveSandbox     FeatureUse = "settings.save.sandbox"
	FeatureUseSaveSearch      FeatureUse = "settings.save.search"
	FeatureUseSaveCompaction  FeatureUse = "settings.save.compaction"
	FeatureUseSavePython      FeatureUse = "settings.save.python"
	FeatureUseSaveLocalModels FeatureUse = "settings.save.local_models"
	FeatureUseSaveCodeIndex   FeatureUse = "settings.save.code_index"
	FeatureUseSaveKnowledge   FeatureUse = "settings.save.knowledge"
	FeatureUseSaveCredential  FeatureUse = "settings.save.credential"
	FeatureUseSaveSSH         FeatureUse = "settings.save.ssh"
	FeatureUseSaveDatabase    FeatureUse = "settings.save.database"
	FeatureUseSaveDefaults    FeatureUse = "settings.save.defaults"
)

var featureUses = []FeatureUse{
	FeatureUsePageConversation, FeatureUsePageActivity, FeatureUsePageFiles, FeatureUsePageGit, FeatureUsePageMarket,
	FeatureUsePageHost, FeatureUsePageSettings, FeatureUsePageNotifications, FeatureUsePageProject, FeatureUsePagePluginThirdParty,
	FeatureUseSettingsGeneral, FeatureUseSettingsAppearance, FeatureUseSettingsConversation, FeatureUseSettingsShortcuts,
	FeatureUseSettingsDictation, FeatureUseSettingsProviders, FeatureUseSettingsConnectors, FeatureUseSettingsTeams,
	FeatureUseSettingsPlugins, FeatureUseSettingsSkills, FeatureUseSettingsKnowledge, FeatureUseSettingsDecision,
	FeatureUseSettingsTerminal, FeatureUseSettingsConnections, FeatureUseSettingsHooks, FeatureUseSettingsRuntime,
	FeatureUseSettingsOCR, FeatureUseSettingsPermissions, FeatureUseSettingsBackup, FeatureUseSettingsAbout, FeatureUseSettingsPluginPage,
	FeatureUsePanelBrowser, FeatureUsePanelTerminal, FeatureUsePanelFiles, FeatureUsePanelGit, FeatureUsePanelHistory,
	FeatureUsePanelDocuments, FeatureUsePanelArtifact, FeatureUsePanelSubagent, FeatureUsePaneSplit,
	FeatureUseComposerSubmit, FeatureUseComposerHasAttachment, FeatureUseComposerHasReference, FeatureUseComposerDictated,
	FeatureUseComposerQueued, FeatureUseComposerGuided, FeatureUseComposerEdited, FeatureUseComposerModeCode,
	FeatureUseComposerModePlan, FeatureUseComposerModeFlagship, FeatureUseComposerPermissionAsk, FeatureUseComposerPermissionProject,
	FeatureUseComposerPermissionFull, FeatureUseComposerModelPicker,
	FeatureUseSearchPalette, FeatureUseReadAloudOSVoice, FeatureUseReadAloudModelVoice, FeatureUseDictationStart,
	FeatureUseSessionExport, FeatureUseThemeBuiltin, FeatureUseThemePackage,
	FeatureUseSaveProvider, FeatureUseSaveTeam, FeatureUseSaveRole, FeatureUseSaveConnector, FeatureUseSaveMCP, FeatureUseSavePlugin,
	FeatureUseSaveHooks, FeatureUseSaveBackup, FeatureUseSaveTerminal, FeatureUseSaveMedia, FeatureUseSavePower, FeatureUseSaveTelemetry,
	FeatureUseSaveNetwork, FeatureUseSaveSandbox, FeatureUseSaveSearch, FeatureUseSaveCompaction, FeatureUseSavePython,
	FeatureUseSaveLocalModels, FeatureUseSaveCodeIndex, FeatureUseSaveKnowledge, FeatureUseSaveCredential, FeatureUseSaveSSH,
	FeatureUseSaveDatabase, FeatureUseSaveDefaults,
}

// FeatureUses lists the fixed identifiers. FeatureUsePagePluginOfficial is
// not among them: it is a prefix completed by a shipped package's name.
func FeatureUses() []FeatureUse { return slices.Clone(featureUses) }

// Listed reports whether the identifier is one of the fixed ones.
func (feature FeatureUse) Listed() bool { return slices.Contains(featureUses, feature) }

// OfficialPluginPage returns the package named by an official plugin page
// identifier. Whether a package of that name ships is the Node's to decide.
func (feature FeatureUse) OfficialPluginPage() (string, bool) {
	name, found := strings.CutPrefix(string(feature), string(FeatureUsePagePluginOfficial))
	return name, found && name != ""
}

// CommandCountFeaturePayload counts one use of a desktop feature on the
// controller's own Node. It carries the identifier and nothing else.
type CommandCountFeaturePayload struct {
	Feature FeatureUse `json:"feature"`
}

func DefaultCommandCountFeaturePayload() CommandCountFeaturePayload {
	return CommandCountFeaturePayload{
		Feature: "",
	}
}
func (value *CommandCountFeaturePayload) UnmarshalJSON(data []byte) error {
	type plain CommandCountFeaturePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"feature"}, true); err != nil {
		return err
	}
	*value = CommandCountFeaturePayload(decoded)
	return nil
}
func (value CommandCountFeaturePayload) MarshalJSON() ([]byte, error) {
	type plain CommandCountFeaturePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

func init() {
	commandVariants["count_feature"] = func() any { return new(CommandCountFeaturePayload) }
	outputVariants["feature_counted"] = nil
}
