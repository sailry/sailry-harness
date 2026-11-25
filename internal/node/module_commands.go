package node

import (
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func moduleUnavailable(family string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: family + " module is unavailable"}
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

func (node *Node) hasGitService() bool {
	return node.workspaceServices.Git && node.git != nil
}

func (node *Node) hasWorktreeService() bool {
	return node.workspaceServices.Worktrees && node.git != nil
}

func (node *Node) runningModuleServices() moduleapi.ServiceCapabilities {
	var running moduleapi.ServiceCapabilities
	if node.executeDatabase != nil {
		running |= moduleapi.ServiceDatabase
	}
	if node.ssh != nil {
		running |= moduleapi.ServiceSSH
	}
	if node.terminals != nil {
		running |= moduleapi.ServiceTerminal
	}
	if node.hasGitService() {
		running |= moduleapi.ServiceGit
	}
	if node.hasWorktreeService() {
		running |= moduleapi.ServiceWorktrees
	}
	if node.codeIndex != nil {
		running |= moduleapi.ServiceCodeIndex
	}
	return running
}

func (node *Node) checkCommandModule(command protocol.Command) error {
	kind := command.Kind
	if (databaseCommand(kind) || databaseProfileCommand(kind)) && node.executeDatabase == nil {
		return moduleUnavailable("databases")
	}
	if sshCommand(kind) && node.ssh == nil {
		return moduleUnavailable("ssh")
	}
	if terminalCommand(kind) && node.terminals == nil {
		return moduleUnavailable("terminal")
	}
	if worktreeCommand(kind) {
		if !node.hasWorktreeService() {
			return moduleUnavailable("worktrees")
		}
		return nil
	}
	if !node.hasGitService() {
		if workspaceGitRead(kind) || workspaceGitWrite(kind) {
			return moduleUnavailable("git")
		}
		if draft, ok := command.Data.(*protocol.ProjectsDraft); ok && kind == "create_project" && draft.Source.Kind == "clone" {
			return moduleUnavailable("git")
		}
		if input, ok := command.Data.(*protocol.ConversationCheckpointHunkSelection); ok && input.Operation != protocol.ConversationCheckpointHunkOperationReject {
			return moduleUnavailable("git")
		}
	}
	return nil
}
