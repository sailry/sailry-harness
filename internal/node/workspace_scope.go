package node

import "github.com/sailry/sailry-harness/internal/protocol"

// commandWorktree reads typed v1 resource identity. Authorization and path
// resolution belong to the Node's captured workspace scope.
func commandWorktree(command protocol.Command) (protocol.WorktreeID, bool) {
	switch input := command.Data.(type) {
	case *protocol.CommandApplyGitHunkPayload:
		return input.Worktree, true
	case *protocol.CommandUseBrowserPayload:
		return input.Worktree, true
	case *protocol.CommandUseExternalBrowserPayload:
		return input.Worktree, true
	case *protocol.CommandDiscoverLocalSkillsPayload:
		return input.Worktree, true
	case *protocol.CommandInstallPluginPayload:
		return input.Worktree, true
	case *protocol.CommandRestoreFileCheckpointPayload:
		return input.Worktree, true
	case *protocol.CommandListTerminalToolsPayload:
		return input.Worktree, true
	case *protocol.CommandCloseTerminalPayload:
		return input.Worktree, true
	case *protocol.CommandListTerminalsPayload:
		return input.Worktree, true
	case *protocol.CommandUseMediaPayload:
		return input.Worktree, true
	case *protocol.CommandInspectGitPayload:
		return input.Worktree, true
	case *protocol.CommandReadGitHeadPayload:
		return input.Worktree, true
	case *protocol.CommandListGitBranchesPayload:
		return input.Worktree, true
	case *protocol.CommandResolveGitRevisionPayload:
		return input.Worktree, true
	case *protocol.CommandListWorktreesPayload:
		return input.Worktree, true
	case *protocol.CommandRemoveWorktreePayload:
		return input.Worktree, true
	case *protocol.CommandCreateGitBranchPayload:
		return input.Worktree, true
	case *protocol.CommandRenameGitBranchPayload:
		return input.Worktree, true
	case *protocol.CommandDeleteGitBranchPayload:
		return input.Worktree, true
	case *protocol.CommandSwitchGitBranchPayload:
		return input.Worktree, true
	case *protocol.CommandMergeGitBranchPayload:
		return input.Worktree, true
	case *protocol.CommandReadGitStashPayload:
		return input.Worktree, true
	case *protocol.CommandReadGitOutputPayload:
		return input.Worktree, true
	case *protocol.CommandListGitRemoteTagsPayload:
		return input.Worktree, true
	case *protocol.CommandReadGitLogPayload:
		return input.Worktree, true
	case *protocol.CommandReadGitCommitPayload:
		return input.Worktree, true
	case *protocol.CommandListGitChangesPayload:
		return input.Worktree, true
	case *protocol.CommandReadGitChangePayload:
		return input.Worktree, true
	case *protocol.CommandReadGitContextPayload:
		return input.Worktree, true
	case *protocol.CommandReadGitShowPayload:
		return input.Worktree, true
	case *protocol.CommandReadGitBlamePayload:
		return input.Worktree, true
	case *protocol.CommandSearchFilesPayload:
		return input.Worktree, true
	case *protocol.CommandRunGitActionPayload:
		return input.Worktree, true
	case *protocol.CommandCreateGitCommitPayload:
		return input.Worktree, true
	case *protocol.CommandUpdateGitIndexPayload:
		return input.Worktree, true
	case *protocol.CommandReadGitDiffPayload:
		return input.Worktree, true
	case *protocol.CommandListDirectoryPayload:
		return input.Worktree, true
	case *protocol.CommandReadFilePayload:
		return input.Worktree, true
	case *protocol.CommandSearchCodePayload:
		return input.Worktree, true
	case *protocol.CommandOutlineCodePayload:
		return input.Worktree, true
	case *protocol.CommandGraphCodePayload:
		return input.Worktree, true
	case *protocol.CommandReadHooksPayload:
		return input.Worktree, true
	case *protocol.CommandSaveHooksPayload:
		return input.Worktree, true
	case *protocol.CommandReadFileLinesPayload:
		return input.Worktree, true
	case *protocol.CommandReadImagePayload:
		return input.Worktree, true
	case *protocol.CommandFindFilesPayload:
		return input.Worktree, true
	case *protocol.CommandReadOfficePayload:
		return input.Worktree, true
	case *protocol.CommandReadNotebookPayload:
		return input.Worktree, true
	case *protocol.CommandEditNotebookPayload:
		return input.Worktree, true
	case *protocol.CommandDownloadFilePayload:
		return input.Worktree, true
	case *protocol.CommandFinishAttachmentUploadPayload:
		return input.Worktree, true
	case *protocol.CommandReadAttachmentPayload:
		return input.Worktree, true
	case *protocol.CommandDownloadAttachmentPayload:
		return input.Worktree, true
	case *protocol.CommandDiscardAttachmentPayload:
		return input.Worktree, true
	case *protocol.CommandFinishFileUploadPayload:
		return input.Worktree, true
	case *protocol.CommandWriteFilePayload:
		return input.Worktree, true
	case *protocol.CommandEditFilePayload:
		return input.Worktree, true
	case *protocol.CommandDeleteFilePayload:
		return input.Worktree, true
	case *protocol.CommandMoveFilePayload:
		return input.Worktree, true
	case *protocol.CommandApplyChangesPayload:
		return input.Worktree, true
	case *protocol.CommandCreateDirectoryPayload:
		return input.Worktree, true
	case *protocol.CommandRenameEntryPayload:
		return input.Worktree, true
	case *protocol.CommandCopyEntryPayload:
		return input.Worktree, true
	case *protocol.CommandCopyEntryToPayload:
		return input.Worktree, true
	case *protocol.CommandMoveEntryToPayload:
		return input.Worktree, true
	case *protocol.CommandTrashEntryPayload:
		return input.Worktree, true
	case *protocol.CommandTrashFilePayload:
		return input.Worktree, true
	case *protocol.CommandReadWorktreeCatalogPayload:
		return input.Worktree, true
	case *protocol.CommandMoveConversationPayload:
		return input.Worktree, true
	case *protocol.CommandForkConversationAtPayload:
		return input.Worktree, true
	case *protocol.AttachmentSpec:
		return input.Worktree, true
	case *protocol.FileUploadSpec:
		return input.Worktree, true
	case *protocol.CommandCreateManagedWorktreePayload:
		return input.Source, true
	default:
		return protocol.WorktreeID{}, false
	}
}
