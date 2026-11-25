package operations

import "github.com/sailry/sailry-harness/internal/protocol"

// WorkspaceModuleCommand maps optional workspace tool operations to their
// protocol commands. The selected module, not the operation, supplies execution.
func WorkspaceModuleCommand(operation protocol.ToolOperation) string {
	switch operation {
	case protocol.ToolOperationInspectGit:
		return "inspect_git"
	case protocol.ToolOperationReadGitDiff:
		return "read_git_diff"
	case protocol.ToolOperationReadGitLog:
		return "read_git_log"
	case protocol.ToolOperationReadGitShow:
		return "read_git_show"
	case protocol.ToolOperationReadGitBlame:
		return "read_git_blame"
	case protocol.ToolOperationListGitChanges:
		return "list_git_changes"
	case protocol.ToolOperationReadGitChange:
		return "read_git_change"
	case protocol.ToolOperationListWorktrees:
		return "list_worktrees"
	case protocol.ToolOperationCreateManagedWorktree:
		return "create_managed_worktree"
	case protocol.ToolOperationRegisterWorktree:
		return "register_worktree"
	case protocol.ToolOperationRemoveWorktree:
		return "remove_worktree"
	}
	return ""
}
