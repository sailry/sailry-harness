package node

import (
	"github.com/sailry/sailry-harness/internal/protocol"
)

func moduleUnavailable(id string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: id + " module is unavailable"}
}

func databaseProfileCommand(kind string) bool {
	return kind == "list_databases" || kind == "save_database" || kind == "remove_database"
}

func sshResourceCommand(kind string) bool {
	switch kind {
	case "browse_ssh_directory", "download_ssh_file", "stage_ssh_upload", "finish_ssh_upload", "modify_ssh_file", "run_ssh", "transfer_ssh", "cancel_ssh":
		return true
	}
	return false
}

func sshCommand(kind string) bool {
	switch kind {
	case "list_ssh", "save_ssh", "remove_ssh", "trust_ssh", "check_ssh", "install_host", "update_ssh_host", "read_host_install", "cancel_ssh",
		"open_ssh_terminal", "close_ssh_terminal", "list_ssh_terminals":
		return true
	}
	return sshResourceCommand(kind)
}

func worktreeCommand(kind string) bool {
	switch kind {
	case "create_worktree", "create_managed_worktree", "register_worktree", "remove_worktree", "list_worktrees":
		return true
	}
	return false
}

func (node *Node) moduleServiceAvailable(id string) bool {
	switch id {
	case "code-index":
		return node.codeIndex != nil
	case "databases":
		return node.modules.Databases != nil
	case "ssh":
		return node.ssh != nil
	case "terminal":
		return node.terminals != nil
	case "git", "worktrees":
		return node.git != nil
	}
	return true
}

func (node *Node) checkCommandModule(command protocol.Command) error {
	kind := command.Kind
	if (databaseCommand(kind) || databaseProfileCommand(kind)) && (!node.modules.HasModule("databases") || node.modules.Databases == nil) {
		return moduleUnavailable("databases")
	}
	if sshCommand(kind) && (!node.modules.HasModule("ssh") || node.ssh == nil) {
		return moduleUnavailable("ssh")
	}
	if terminalCommand(kind) && (!node.modules.HasModule("terminal") || node.terminals == nil) {
		return moduleUnavailable("terminal")
	}
	if worktreeCommand(kind) && (!node.modules.HasModule("worktrees") || node.git == nil) {
		return moduleUnavailable("worktrees")
	}
	if (workspaceGitRead(kind) || workspaceGitWrite(kind)) && (!node.modules.HasModule("git") || node.git == nil) {
		return moduleUnavailable("git")
	}
	if draft, ok := command.Data.(*protocol.ProjectsDraft); ok && kind == "create_project" && draft.Source.Kind == "clone" && (!node.modules.HasModule("git") || node.git == nil) {
		return moduleUnavailable("git")
	}
	if input, ok := command.Data.(*protocol.ConversationCheckpointHunkSelection); ok && input.Operation != protocol.ConversationCheckpointHunkOperationReject && (!node.modules.HasModule("git") || node.git == nil) {
		return moduleUnavailable("git")
	}
	return nil
}
