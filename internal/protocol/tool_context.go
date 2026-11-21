package protocol

import "slices"

// ToolContextAvailable reports whether a declaration matches the conversation's
// tool contexts. An ordinary conversation has a worktree even without a project.
func ToolContextAvailable(contexts []PluginConversationResource, project *ProjectID, config SessionConfig) bool {
	if len(contexts) == 0 {
		return true
	}
	if config.Resource != nil {
		if project != nil {
			return false
		}
		var resource PluginConversationResource
		switch config.Resource.Kind {
		case "database":
			resource = PluginConversationResourceDatabase
		case "ssh":
			resource = PluginConversationResourceSsh
		default:
			return false
		}
		return slices.Contains(contexts, resource) || config.Assistant != nil && slices.Contains(contexts, PluginConversationResourcePlugin)
	}
	if project != nil {
		return slices.Contains(contexts, PluginConversationResourceWorkspace)
	}
	return slices.Contains(contexts, PluginConversationResourcePlugin) || config.Assistant == nil && slices.Contains(contexts, PluginConversationResourceWorkspace)
}
