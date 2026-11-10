package live

import (
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func (state *pluginNativeState) composerOperationsAvailable(config protocol.SessionConfig) []protocol.ToolOperation {
	catalog := state.runtime.localRuntime().pluginViewCatalog(state.runtime)
	var project *protocol.ProjectID
	if state.scope.Session != nil {
		if session, exists := state.runtime.session(state.scope.Session.String()); exists {
			project = session.Project
		}
	} else if state.scope.Worktree != nil {
		for _, worktree := range state.runtime.controller.Snapshot().Worktrees {
			if worktree.ID == *state.scope.Worktree {
				project = worktree.Project
				break
			}
		}
	}
	var selected *protocol.PluginConversationDeclaration
	if binding := config.Assistant; binding != nil {
		for _, info := range catalog {
			if !pluginNativePackageAvailable(info) || packageReference(info.Summary) != binding.Package ||
				info.Extension == nil || info.Extension.Desktop == nil {
				continue
			}
			for _, declaration := range info.Extension.Desktop.Conversations {
				if declaration.ID == binding.ID {
					entry := declaration
					selected = &entry
					break
				}
			}
		}
		if selected == nil {
			return []protocol.ToolOperation{}
		}
	}
	order := []protocol.ToolOperation{}
	available := map[protocol.ToolOperation]bool{}
	for _, info := range catalog {
		if info.Extension == nil {
			continue
		}
		extension := info.Extension
		for _, tool := range extension.Tools {
			operation := tool.Operation
			if operation == nil && tool.Handler != nil {
				operation = tool.Handler.Operation
			}
			if operation == nil {
				continue
			}
			if _, seen := available[*operation]; !seen {
				order = append(order, *operation)
				available[*operation] = false
			}
			action := pluginNativeOperationAction(*operation)
			if available[*operation] || !pluginNativePackageAvailable(info) ||
				action != "" && !slices.Contains(extension.Actions, action) ||
				len(tool.Platforms) != 0 || !pluginNativeToolContext(tool.Contexts, project, config.Resource) ||
				config.Resource != nil && config.Assistant == nil && len(tool.Contexts) == 0 {
				continue
			}
			if selected != nil && !slices.ContainsFunc(selected.Tools, func(entry protocol.PluginConversationTool) bool {
				switch entry.Kind {
				case "plugin":
					value, valid := detail[protocol.PluginConversationToolPluginPayload](entry.Data)
					return valid && value.Server == nil && config.Assistant.Package == packageReference(info.Summary) && value.Name == tool.Name
				case "package":
					value, valid := detail[protocol.PluginConversationToolPackagePayload](entry.Data)
					return valid && value.Package == info.Summary.Name && value.Name == tool.Name
				default:
					return false
				}
			}) {
				continue
			}
			available[*operation] = true
		}
	}
	return slices.DeleteFunc(order, func(operation protocol.ToolOperation) bool { return !available[operation] })
}

func pluginNativePackageAvailable(info protocol.PluginInfo) bool {
	return info.Summary.Enabled && info.Summary.Compatibility.Kind == "compatible"
}

func pluginNativeToolContext(contexts []protocol.PluginConversationResource, project *protocol.ProjectID, resource *protocol.ConnectionResource) bool {
	if len(contexts) == 0 {
		return true
	}
	for _, kind := range contexts {
		switch kind {
		case protocol.PluginConversationResourcePlugin:
			if project == nil && resource == nil {
				return true
			}
		case protocol.PluginConversationResourceWorkspace:
			if project != nil && resource == nil {
				return true
			}
		case protocol.PluginConversationResourceDatabase:
			if project == nil && resource != nil && resource.Kind == "database" {
				return true
			}
		case protocol.PluginConversationResourceSsh:
			if project == nil && resource != nil && resource.Kind == "ssh" {
				return true
			}
		}
	}
	return false
}

func pluginNativeOperationAction(operation protocol.ToolOperation) protocol.PluginAction {
	switch operation {
	case protocol.ToolOperationInstallModel, protocol.ToolOperationRemoveModel:
		return protocol.PluginActionInstallModels
	case protocol.ToolOperationListRoles:
		return protocol.PluginActionReadRoles
	case protocol.ToolOperationSaveRole, protocol.ToolOperationRemoveRole, protocol.ToolOperationApplyRoles:
		return protocol.PluginActionWriteRoles
	case protocol.ToolOperationReadArtifact, protocol.ToolOperationListArtifacts, protocol.ToolOperationListArtifactRevisions:
		return protocol.PluginActionReadArtifacts
	case protocol.ToolOperationWriteArtifact:
		return protocol.PluginActionWriteArtifacts
	case protocol.ToolOperationSearchCode, protocol.ToolOperationOutlineCode, protocol.ToolOperationGraphCode:
		return protocol.PluginActionReadCodeIndex
	case protocol.ToolOperationInstallCodeServer, protocol.ToolOperationUninstallCodeServer, protocol.ToolOperationStartCodeServer, protocol.ToolOperationStopCodeServer:
		return protocol.PluginActionManageCode
	case protocol.ToolOperationCodePackages, protocol.ToolOperationCodeCatalog, protocol.ToolOperationCodeServers, protocol.ToolOperationCodeDiagnostics,
		protocol.ToolOperationCodeDefinition, protocol.ToolOperationCodeReferences, protocol.ToolOperationCodeHover, protocol.ToolOperationCodeSymbols:
		return protocol.PluginActionReadCode
	case protocol.ToolOperationInspectGit, protocol.ToolOperationReadGitDiff, protocol.ToolOperationReadGitLog, protocol.ToolOperationReadGitShow,
		protocol.ToolOperationListGitChanges, protocol.ToolOperationReadGitChange, protocol.ToolOperationReadGitBlame:
		return protocol.PluginActionReadGit
	case protocol.ToolOperationListWorktrees:
		return protocol.PluginActionReadWorktrees
	case protocol.ToolOperationCreateManagedWorktree, protocol.ToolOperationRegisterWorktree, protocol.ToolOperationRemoveWorktree:
		return protocol.PluginActionWriteWorktrees
	case protocol.ToolOperationListDatabaseConnections, protocol.ToolOperationBrowseDatabase, protocol.ToolOperationQueryDatabase:
		return protocol.PluginActionReadDatabases
	case protocol.ToolOperationSaveDatabaseConnection:
		return protocol.PluginActionManageDatabases
	case protocol.ToolOperationListSshConnections:
		return protocol.PluginActionReadSsh
	case protocol.ToolOperationSaveSshConnection:
		return protocol.PluginActionManageSsh
	case protocol.ToolOperationExecuteDatabase:
		return protocol.PluginActionControlDatabases
	case protocol.ToolOperationRunSsh, protocol.ToolOperationTransferSsh:
		return protocol.PluginActionControlSsh
	case protocol.ToolOperationDelegateAgent, protocol.ToolOperationGuideAgent, protocol.ToolOperationWaitAgent:
		return protocol.PluginActionDelegateAgents
	case protocol.ToolOperationListDirectory, protocol.ToolOperationReadFile, protocol.ToolOperationReadFileLines, protocol.ToolOperationFindFiles,
		protocol.ToolOperationSearchFiles, protocol.ToolOperationReadOffice, protocol.ToolOperationReadNotebook:
		return protocol.PluginActionReadFiles
	case protocol.ToolOperationWriteFile, protocol.ToolOperationEditFile, protocol.ToolOperationEditNotebook, protocol.ToolOperationDeleteFile,
		protocol.ToolOperationMoveFile, protocol.ToolOperationApplyChanges, protocol.ToolOperationCreateDirectory:
		return protocol.PluginActionWriteFiles
	case protocol.ToolOperationInspectMedia:
		return protocol.PluginActionInspectMedia
	case protocol.ToolOperationGenerateImage:
		return protocol.PluginActionGenerateImage
	case protocol.ToolOperationGenerateVideo:
		return protocol.PluginActionGenerateVideo
	case protocol.ToolOperationReadComputer:
		return protocol.PluginActionReadComputer
	case protocol.ToolOperationControlComputer:
		return protocol.PluginActionControlComputer
	case protocol.ToolOperationHttp:
		return protocol.PluginActionHttp
	case protocol.ToolOperationFetchWeb, protocol.ToolOperationSearchWeb:
		return protocol.PluginActionReadWeb
	case protocol.ToolOperationReadBrowser:
		return protocol.PluginActionReadBrowser
	case protocol.ToolOperationControlBrowser:
		return protocol.PluginActionControlBrowser
	case protocol.ToolOperationReadExternalBrowser:
		return protocol.PluginActionReadExternalBrowser
	case protocol.ToolOperationControlExternalBrowser:
		return protocol.PluginActionControlExternalBrowser
	case protocol.ToolOperationGetValue, protocol.ToolOperationListKeys:
		return protocol.PluginActionReadStorage
	case protocol.ToolOperationSetValue, protocol.ToolOperationDeleteValue, protocol.ToolOperationStorageTransaction:
		return protocol.PluginActionWriteStorage
	case protocol.ToolOperationKnowledgeQuery, protocol.ToolOperationKnowledgeRead, protocol.ToolOperationKnowledgeCollections:
		return protocol.PluginActionReadKnowledge
	case protocol.ToolOperationKnowledgeCreate, protocol.ToolOperationKnowledgeRebuild, protocol.ToolOperationKnowledgeRemove:
		return protocol.PluginActionWriteKnowledge
	case protocol.ToolOperationReadCommand:
		return protocol.PluginActionReadCommands
	case protocol.ToolOperationRunCommand, protocol.ToolOperationStopCommand, protocol.ToolOperationWriteCommand:
		return protocol.PluginActionControlCommands
	default:
		return ""
	}
}
