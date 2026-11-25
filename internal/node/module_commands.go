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

func (node *Node) runningModuleServices() moduleapi.ServiceCapabilities {
	running := node.workspaceCapabilities
	if node.executeDatabase != nil {
		running |= moduleapi.ServiceDatabase
	}
	if node.ssh != nil {
		running |= moduleapi.ServiceSSH
	}
	if node.terminals != nil {
		running |= moduleapi.ServiceTerminal
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
	if draft, ok := command.Data.(*protocol.ProjectsDraft); ok && kind == "create_project" && draft.Source.Kind != "local" && node.workspace.ProjectSources[draft.Source.Kind] == nil {
		return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "project source is unavailable"}
	}
	if input, ok := command.Data.(*protocol.ConversationCheckpointHunkSelection); ok && input.Operation != protocol.ConversationCheckpointHunkOperationReject && node.workspace.StageTurnHunk == nil {
		return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "hunk staging is unavailable"}
	}
	return nil
}
