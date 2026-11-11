package operations

import (
	"encoding/json"
	"math"
	"path/filepath"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Command binds model arguments to immutable turn resources. Resource identity
// never comes from an untrusted tool argument except an explicit worktree tool.
func Command(operation protocol.ToolOperation, arguments json.RawMessage, worktree protocol.WorktreeID, session protocol.SessionID, turn protocol.TurnID, project *protocol.ProjectID, sandbox bool) (protocol.Command, error) {
	var values map[string]any
	if err := json.Unmarshal(arguments, &values); err != nil {
		return protocol.Command{}, err
	}
	if values == nil {
		// A plugin script can return null where the model schema cannot.
		return protocol.Command{}, invalidCommand("native operation arguments must be an object")
	}
	set := func(key string, value any) { values[key] = value }
	defaultValue := func(key string, value any) {
		if _, ok := values[key]; !ok {
			values[key] = value
		}
	}
	kind := ""
	switch string(operation) {
	case "files.list":
		kind = "list_directory"
		set("worktree", worktree)
		defaultValue("after", nil)
	case "files.read", "files.read_lines":
		kind = "read_file_lines"
		set("worktree", worktree)
		defaultValue("offset", 0)
		defaultValue("limit", 2000)
		defaultValue("expected_revision", nil)
		if string(operation) == "files.read" {
			path, _ := values["path"].(string)
			switch strings.ToLower(filepath.Ext(path)) {
			case ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp":
				if values["expected_revision"] != nil {
					return protocol.Command{}, invalidCommand("expected_revision applies only to text reads")
				}
				kind = "read_image"
			}
		}
	case "files.write":
		kind = "write_file"
		set("worktree", worktree)
	case "files.edit":
		kind = "edit_file"
		set("worktree", worktree)
		defaultValue("replace_all", false)
	case "files.delete":
		kind = "delete_file"
		set("worktree", worktree)
	case "files.move":
		kind = "move_file"
		set("worktree", worktree)
		set("from", values["path"])
		set("to", values["destination"])
		delete(values, "path")
		delete(values, "destination")
	case "files.mkdir":
		kind = "create_directory"
		set("worktree", worktree)
	case "files.find":
		kind = "find_files"
		set("worktree", worktree)
		defaultValue("path", "")
		defaultValue("limit", 500)
		defaultValue("offset", 0)
	case "files.search":
		kind = "search_files"
		defaultValue("path", "")
		defaultValue("limit", 100)
		defaultValue("offset", 0)
		defaultValue("mode", "files")
		defaultValue("context", 0)
		defaultValue("regex", false)
		defaultValue("case_sensitive", false)
		defaultValue("multiline", false)
		defaultValue("globs", []string{})
		values = map[string]any{"worktree": worktree, "options": values}
	case "office.read":
		kind = "read_office"
		defaultValue("offset", 0)
		values = map[string]any{"worktree": worktree, "options": values}
	case "notebook.read":
		kind = "read_notebook"
		defaultValue("offset", 0)
		defaultValue("limit", 20)
		values = map[string]any{"worktree": worktree, "options": values}
	case "notebook.edit":
		kind = "edit_notebook"
		values = map[string]any{"worktree": worktree, "edit": values}
	case "git.status":
		kind = "inspect_git"
	case "git.diff":
		kind = "read_git_diff"
	case "git.log":
		kind = "read_git_log"
		defaultValue("cursor", nil)
		defaultValue("revision", nil)
		defaultValue("path", nil)
	case "git.show":
		kind = "read_git_show"
	case "git.blame":
		kind = "read_git_blame"
	case "git.changes":
		kind = "list_git_changes"
	case "git.change":
		kind = "read_git_change"
	case "worktrees.list":
		kind = "list_worktrees"
		set("worktree", worktree)
	case "worktrees.create":
		if project == nil {
			return protocol.Command{}, invalidCommand("worktree creation requires a project")
		}
		kind = "create_managed_worktree"
		set("source", worktree)
		set("project", *project)
	case "worktrees.register":
		if project == nil {
			return protocol.Command{}, invalidCommand("worktree registration requires a project")
		}
		kind = "register_worktree"
		set("project", *project)
	case "worktrees.remove":
		kind = "remove_worktree"
	case "process.run":
		kind = "run_command"
		set("turn", turn)
		if values["access"] == nil {
			access := "unrestricted"
			if sandbox {
				access = "workspace"
			}
			set("access", access)
		}
		defaultValue("follow_up", false)
		defaultValue("pty", false)
	case "process.read":
		set("session", session)
		if values["id"] == nil {
			kind = "list_commands"
			values = map[string]any{"session": session}
		} else if values["stream"] != nil || values["offset"] != nil || values["limit"] != nil {
			kind = "read_command_page"
			defaultValue("stream", "stdout")
			defaultValue("offset", 0)
			defaultValue("limit", 32768)
		} else {
			kind = "read_command"
		}
	case "process.stop":
		kind = "stop_command"
		set("session", session)
	case "process.write":
		kind = "write_command"
		set("session", session)
	case "web.fetch":
		kind = "fetch_web"
		defaultValue("offset", 0)
	case "web.search":
		kind = "search_web"
		defaultValue("limit", 5)
	case "code.search":
		kind = "search_code"
		values = map[string]any{"worktree": worktree, "query": values}
	case "code.outline":
		kind = "outline_code"
		set("worktree", worktree)
	case "code.graph":
		kind = "graph_code"
		values = map[string]any{"worktree": worktree, "query": values}
	case "artifacts.read":
		kind = "read_artifact"
		set("session", session)
	case "artifacts.list":
		kind = "list_artifacts"
		set("session", session)
		defaultValue("limit", 32)
		defaultValue("after", nil)
	case "artifacts.revisions":
		kind = "list_artifact_revisions"
		set("session", session)
		defaultValue("limit", 32)
		defaultValue("before", nil)
	case "artifacts.write":
		kind = "write_artifact"
		defaultValue("kind", "document")
		defaultValue("summary", "")
		defaultValue("id", nil)
		defaultValue("expected_revision", nil)
	case "storage.get":
		kind = "read_plugin_value"
	case "storage.list":
		kind = "list_plugin_keys"
		limit, ok := values["limit"].(float64)
		if !ok || limit < 1 || limit > 9007199254740991 || limit != math.Trunc(limit) {
			return protocol.Command{}, invalidCommand("limit must be a positive safe integer")
		}
		set("limit", min(limit, 100))
	case "storage.set":
		kind = "write_plugin_value"
	case "storage.delete":
		kind = "remove_plugin_value"
	default:
		return protocol.Command{}, invalidCommand("native operation is unavailable")
	}
	if strings.HasPrefix(string(operation), "git.") {
		if values["worktree"] == nil {
			set("worktree", worktree)
		}
	}
	raw, err := json.Marshal(map[string]any{"kind": kind, "data": values})
	if err != nil {
		return protocol.Command{}, err
	}
	var command protocol.Command
	if err = json.Unmarshal(raw, &command); err != nil {
		return command, invalidCommand("invalid native operation arguments: " + err.Error())
	}
	return command, nil
}
func invalidCommand(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}

// ConnectionOperation reports whether an operation reaches a saved database or
// SSH connection. These operations are bound by the Node, which owns the
// sharing decision, instead of Command.
func ConnectionOperation(operation protocol.ToolOperation) bool {
	switch operation {
	case protocol.ToolOperationListDatabaseConnections, protocol.ToolOperationSaveDatabaseConnection,
		protocol.ToolOperationListSshConnections, protocol.ToolOperationSaveSshConnection,
		protocol.ToolOperationBrowseDatabase, protocol.ToolOperationQueryDatabase, protocol.ToolOperationExecuteDatabase,
		protocol.ToolOperationRunSsh, protocol.ToolOperationTransferSsh:
		return true
	}
	return false
}

// ConnectionArguments is one decoded connection tool call. Only the fields of
// its operation are set.
type ConnectionArguments struct {
	// Target is the connection a catalog, query, run or transfer names.
	Target *protocol.ConnectionResource
	// Save holds the checked arguments of a save operation.
	Save json.RawMessage
	// Catalog, query and execute.
	Database *string
	ReadOnly bool
	Sql      string
	RowLimit uint32
	// Run and transfer.
	Command    string
	Path       string
	RemotePath string
	Direction  protocol.SshDirection
	TimeoutMs  uint64
}

// ConnectionCall decodes the model arguments of a connection operation.
func ConnectionCall(operation protocol.ToolOperation, arguments json.RawMessage) (ConnectionArguments, error) {
	invalid := func(err error) (ConnectionArguments, error) {
		return ConnectionArguments{}, invalidCommand("invalid connection operation: " + err.Error())
	}
	switch operation {
	case protocol.ToolOperationListDatabaseConnections, protocol.ToolOperationListSshConnections:
		var input struct{}
		if err := protocol.DecodeObject(arguments, &input, nil, true); err != nil {
			return invalid(err)
		}
		return ConnectionArguments{}, nil
	case protocol.ToolOperationSaveDatabaseConnection, protocol.ToolOperationSaveSshConnection:
		var fields map[string]json.RawMessage
		if err := json.Unmarshal(arguments, &fields); err != nil || fields == nil {
			return ConnectionArguments{}, invalidCommand("invalid connection operation: expected an object")
		}
		for name, value := range fields {
			if name != "connection" && name != "expected_revision" && string(value) == "null" {
				return ConnectionArguments{}, invalidCommand("connection edit fields cannot be null")
			}
		}
		if credential, present := fields["credential"]; present && operation == protocol.ToolOperationSaveSshConnection {
			var tagged struct {
				Kind string `json:"kind"`
			}
			if json.Unmarshal(credential, &tagged) != nil {
				return ConnectionArguments{}, invalidCommand("invalid SSH credential")
			}
			switch tagged.Kind {
			case "password", "private_key", "key_path", "agent":
			default:
				return ConnectionArguments{}, invalidCommand("invalid SSH credential")
			}
		}
		return ConnectionArguments{Save: arguments}, nil
	case protocol.ToolOperationBrowseDatabase:
		var input struct {
			Connection protocol.DatabaseID `json:"connection"`
			Database   *string             `json:"database"`
		}
		if err := protocol.DecodeObject(arguments, &input, []string{"connection"}, true); err != nil {
			return invalid(err)
		}
		return ConnectionArguments{Target: &protocol.ConnectionResource{Kind: "database", Data: input.Connection}, Database: input.Database}, nil
	case protocol.ToolOperationQueryDatabase, protocol.ToolOperationExecuteDatabase:
		var input struct {
			Connection protocol.DatabaseID `json:"connection"`
			Database   *string             `json:"database"`
			Sql        string              `json:"sql"`
			RowLimit   uint32              `json:"row_limit"`
			TimeoutMs  uint64              `json:"timeout_ms"`
		}
		if err := protocol.DecodeObject(arguments, &input, []string{"connection", "sql", "row_limit", "timeout_ms"}, true); err != nil {
			return invalid(err)
		}
		return ConnectionArguments{Target: &protocol.ConnectionResource{Kind: "database", Data: input.Connection}, Database: input.Database,
			ReadOnly: operation == protocol.ToolOperationQueryDatabase, Sql: input.Sql, RowLimit: input.RowLimit, TimeoutMs: input.TimeoutMs}, nil
	case protocol.ToolOperationRunSsh:
		var input struct {
			Connection protocol.SshID `json:"connection"`
			Command    string         `json:"command"`
			TimeoutMs  uint64         `json:"timeout_ms"`
		}
		if err := protocol.DecodeObject(arguments, &input, []string{"connection", "command", "timeout_ms"}, true); err != nil {
			return invalid(err)
		}
		return ConnectionArguments{Target: &protocol.ConnectionResource{Kind: "ssh", Data: input.Connection}, Command: input.Command, TimeoutMs: input.TimeoutMs}, nil
	case protocol.ToolOperationTransferSsh:
		var input struct {
			Connection protocol.SshID        `json:"connection"`
			Path       string                `json:"path"`
			RemotePath string                `json:"remote_path"`
			Direction  protocol.SshDirection `json:"direction"`
			TimeoutMs  uint64                `json:"timeout_ms"`
		}
		if err := protocol.DecodeObject(arguments, &input, []string{"connection", "path", "remote_path", "direction", "timeout_ms"}, true); err != nil {
			return invalid(err)
		}
		return ConnectionArguments{Target: &protocol.ConnectionResource{Kind: "ssh", Data: input.Connection}, Path: input.Path, RemotePath: input.RemotePath,
			Direction: input.Direction, TimeoutMs: input.TimeoutMs}, nil
	}
	return ConnectionArguments{}, invalidCommand("native operation is unavailable")
}
