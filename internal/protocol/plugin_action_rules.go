package protocol

func (value PluginAction) ReadOnly() bool {
	switch value {
	case PluginActionReadCode,
		PluginActionReadCodeIndex,
		PluginActionReadCodeIndexSettings,
		PluginActionReadArtifacts,
		PluginActionReadCodeSettings,
		PluginActionReadSearchSettings,
		PluginActionReadActivity,
		PluginActionReadUsage,
		PluginActionReadDatabases,
		PluginActionReadSsh,
		PluginActionReadRoles,
		PluginActionReadTeams,
		PluginActionReadModels,
		PluginActionRunDecisions,
		PluginActionInspectMedia,
		PluginActionReadMediaSettings,
		PluginActionReadComputer,
		PluginActionReadBrowser,
		PluginActionReadExternalBrowser,
		PluginActionReadFiles,
		PluginActionReadGit,
		PluginActionReadWorktrees,
		PluginActionReadCommands,
		PluginActionReadTerminals,
		PluginActionReadConversation,
		PluginActionReadMessage,
		PluginActionReadProjects,
		PluginActionReadWeb,
		PluginActionReadStorage,
		PluginActionReadKnowledge:
		return true
	default:
		return false
	}
}
