package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"slices"
	"strings"
	"sync"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type workspaceToolCall struct {
	request  protocol.Request
	prepared *files.Prepared
	approval protocol.ApprovalID
}

func (node *Node) workspaceTools(ctx context.Context, inv *store.AgentInvocation) ([]engine.Tool, error) {
	if _, err := node.workspaceRoot(ctx, inv.Run.Worktree, false); err != nil {
		return nil, err
	}
	catalog, err := operations.CaptureCatalog(ctx, node.profile.Ownership.Path, inv.Turn.Plugins)
	if err != nil {
		return nil, err
	}
	sandbox := protocol.DefaultSandboxSettings()
	if err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error { var err error; sandbox, err = store.ReadSandboxSettings(ctx, db); return err }); err != nil {
		return nil, err
	}
	var tools []engine.Tool
	for _, declaration := range catalog.Tools {
		if !declaration.Available(inv.Worktree.Project != nil, connectionBound(inv)) || !slices.Contains(inv.Turn.Plugins, declaration.Owner) || operations.ConnectionOperation(declaration.Operation) {
			continue
		}
		// A connection conversation without an assistant offers only tools that name a context.
		if inv.Turn.Config.Resource != nil && inv.Turn.Config.Assistant == nil && len(declaration.Contexts) == 0 &&
			!strings.HasPrefix(string(declaration.Operation), "artifacts.") {
			continue
		}
		if declaration.Operation == protocol.ToolOperationWriteArtifact && inv.Session.Delegation != nil {
			continue
		}
		var schema map[string]any
		if err = json.Unmarshal(declaration.Parameters, &schema); err != nil {
			return nil, err
		}
		var mu sync.Mutex
		calls := map[string]*workspaceToolCall{}
		tool := engine.Tool{Definition: fantasy.FunctionTool{Name: declaration.Name, Description: declaration.Description, InputSchema: schema}, SchemaJSON: declaration.Parameters, ReadOnly: declaration.ReadOnly, Concurrent: declaration.Concurrent, Deferred: !declaration.Initial, Approval: !declaration.ReadOnly, Binding: &store.ApprovalBinding{Owner: declaration.Owner, Tool: declaration.Name, Operation: &declaration.Operation}}
		tool.Prepare = func(ctx context.Context, call canonical.ToolRequest) error {
			command, err := operations.Command(declaration.Operation, call.Arguments, inv.Run.Worktree, inv.Session.ID, inv.Run.Turn, inv.Worktree.Project, sandbox.Enabled)
			if declaration.Operation == protocol.ToolOperationApplyChanges {
				var patch struct {
					Input string `json:"input"`
				}
				if err = json.Unmarshal(call.Arguments, &patch); err != nil {
					return err
				}
				revisions, e := node.observedWorkspaceRevisions(ctx, inv.Session.ID)
				if e != nil {
					return e
				}
				changes, e := files.ResolvePatchWithRevisions(ctx, inv.Worktree.Path, patch.Input, revisions, core.RestrictedFiles)
				if e != nil {
					return e
				}
				command = protocol.Command{Kind: "apply_changes", Data: &protocol.CommandApplyChangesPayload{Worktree: inv.Run.Worktree, Changes: changes}}
				err = nil
			}
			if err != nil {
				return err
			}
			if target, ok := commandWorktree(command); ok && target != inv.Run.Worktree {
				allowed := false
				if inv.Worktree.Project != nil {
					err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
						capture, e := store.ReadWorkspaceGitCapture(ctx, db, target)
						if e != nil {
							return e
						}
						allowed = capture.Project.ID == *inv.Worktree.Project
						return nil
					})
					if err != nil {
						return err
					}
				}
				if !allowed {
					return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "tool worktree is outside the captured project"}
				}
			}
			request := protocol.NewRequest(node.ID(), command)
			request.Plugin = &protocol.PluginContext{Turn: &inv.Run.Turn, Package: declaration.Owner, Worktree: &inv.Run.Worktree, Session: &inv.Session.ID, Surface: protocol.PluginDesktopSurface{Kind: "workspace"}}
			current := &workspaceToolCall{request: request}
			if fileTextMutation(command.Kind) {
				current.prepared, err = files.Prepare(ctx, inv.Worktree.Path, command, core.RestrictedFiles)
				if err != nil {
					return err
				}
			}
			if files.SupportsMutation(command.Kind) {
				if err = node.protectWorkspaceMutation(inv.Worktree.Path, inv.Worktree.Path, command, current.prepared); err != nil {
					return err
				}
			}
			mu.Lock()
			calls[call.ID] = current
			mu.Unlock()
			return nil
		}
		tool.ConfigureApproval = func(ctx context.Context, db *sql.Conn, call canonical.ToolRequest, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
			mu.Lock()
			current := calls[call.ID]
			mu.Unlock()
			var changes *protocol.ChangePreview
			if current.prepared != nil {
				changes = &current.prepared.Preview
			}
			return store.ConfigureWorkspaceApproval(ctx, db, inv.Run.Turn, value, declaration.Operation, current.request.Command, changes)
		}
		tool.Approved = func(call canonical.ToolRequest, approval protocol.ApprovalID, request protocol.RequestID) {
			mu.Lock()
			defer mu.Unlock()
			current := calls[call.ID]
			current.approval = approval
			current.request.ID = request
		}
		tool.Execute = func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
			mu.Lock()
			current := calls[call.ID]
			delete(calls, call.ID)
			mu.Unlock()
			if current == nil {
				return workspaceToolFault(errors.New("native tool was not prepared"))
			}
			request := current.request
			if !declaration.ReadOnly {
				body, err := json.Marshal(request)
				if err != nil {
					return workspaceToolFault(err)
				}
				unlock := node.agent.LockPublication()
				err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
					if err := store.ApprovedWorkspaceRequest(ctx, db, inv.Run.Turn, current.approval, request); err != nil {
						return err
					}
					previous, err := admission.Admit(ctx, db, inv.Caller, request, body)
					if err != nil {
						return err
					}
					if previous != nil {
						return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "tool effect was already admitted and will not replay"}
					}
					if current.prepared != nil {
						return store.CaptureFileCheckpointsRequest(ctx, db, inv.Run.Turn, current.approval, request, current.prepared)
					}
					return nil
				})
				unlock()
				if err != nil {
					return workspaceToolFault(err)
				}
			}
			effect := &workspaceCompletion{}
			defer effect.close()
			output, effectErr := node.workspaceIO(ctx, inv.Caller, request, current.prepared, effect)
			if !declaration.ReadOnly {
				unlock := node.agent.LockPublication()
				defer unlock()
				var err error
				output, effectErr, err = node.recordWorkspaceEffect(context.WithoutCancel(ctx), inv.Caller, request, output, effectErr, effect)
				if err != nil {
					return workspaceToolFault(&protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "tool effect result could not be recorded; inspect the target before retrying"})
				}
			}
			if effectErr != nil {
				return workspaceToolFault(effectErr)
			}
			if declaration.Operation == protocol.ToolOperationReadCommand {
				if snapshot, ok := output.Data.(protocol.ProcessSnapshot); ok {
					unlock := node.agent.LockPublication()
					err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
						events, err := store.ConsumeCommandBackground(ctx, db, inv.Run.Turn, snapshot, time.Now())
						if err != nil {
							return err
						}
						_, err = store.RecordEvents(ctx, db, node.ID(), events)
						return err
					})
					if err == nil {
						node.publish()
						node.agent.RefreshCommitted(inv.Session.ID)
					}
					unlock()
					if err != nil {
						return workspaceToolFault(err)
					}
					node.agent.Committed()
				}
			}
			value := node.workspaceToolValue(ctx, request, output)
			return engine.ToolResult{Value: value, ReturnedError: workspaceOutputFailed(output), ReturnedFailure: workspaceOutputFailure(output)}
		}
		tools = append(tools, tool)
	}
	connections, err := node.connectionTools(ctx, inv, catalog)
	if err != nil {
		return nil, err
	}
	return node.pluginTools(ctx, inv, append(tools, connections...))
}
func workspaceToolFault(err error) engine.ToolResult {
	return engine.ToolResult{Failure: nodeFault(err).(*protocol.Fault)}
}
func workspaceOutputFailed(output protocol.Output) bool { return workspaceOutputFailure(output) != nil }
func workspaceOutputFailure(output protocol.Output) *protocol.ToolFailure {
	var outcome *protocol.ProcessOutcome
	switch value := output.Data.(type) {
	case protocol.ProcessCompletion:
		outcome = &value.Outcome
	case protocol.ProcessSnapshot:
		switch value.Info.Status.Kind {
		case "finished":
			raw, _ := json.Marshal(value.Info.Status.Data)
			var decoded protocol.ProcessOutcome
			if json.Unmarshal(raw, &decoded) == nil {
				outcome = &decoded
			}
		case "failed":
			return &protocol.ToolFailure{Origin: protocol.ToolFailureOriginRuntime}
		}
	case protocol.FileChanges:
		if value.Status != protocol.ChangesStatusApplied {
			return &protocol.ToolFailure{Origin: protocol.ToolFailureOriginTool}
		}
	}
	if outcome == nil {
		return nil
	}
	failure := &protocol.ToolFailure{Origin: protocol.ToolFailureOriginRuntime}
	switch outcome.Kind {
	case "exited":
		raw, _ := json.Marshal(outcome.Data)
		var code int32
		if json.Unmarshal(raw, &code) == nil && code == 0 {
			return nil
		}
		failure.Origin = protocol.ToolFailureOriginTool
	case "cancelled":
		code := protocol.ErrorCodeCancelled
		failure.Code = &code
	case "unknown":
		code := protocol.ErrorCodeOutcomeUnknown
		failure.Code = &code
	}
	return failure
}
func (node *Node) workspaceToolValue(ctx context.Context, request protocol.Request, output protocol.Output) any {
	if output.Kind != "command_result" && output.Kind != "command_output" && output.Kind != "command_started" {
		return output
	}
	raw, err := json.Marshal(output)
	if err != nil {
		return output
	}
	var value map[string]any
	if json.Unmarshal(raw, &value) != nil {
		return output
	}
	id := request.ID
	switch input := request.Command.Data.(type) {
	case *protocol.CommandReadCommandPayload:
		id = input.ID
	case *protocol.CommandStopCommandPayload:
		id = input.ID
	}
	if output.Kind == "command_result" || output.Kind == "command_output" {
		value["sailry_command"] = map[string]any{"id": id}
		var session protocol.SessionID
		if request.Plugin != nil && request.Plugin.Session != nil {
			session = *request.Plugin.Session
		}
		if snapshot, ok := output.Data.(protocol.ProcessSnapshot); ok {
			session = snapshot.Info.Session
			id = snapshot.Info.ID
		}
		if preview, err := node.processes.Preview(session, id, output.Kind == "command_output"); err == nil {
			value["sailry_command"] = preview
		}
	}
	if input, ok := request.Command.Data.(*protocol.CommandRunCommandPayload); ok && output.Kind == "command_started" && !input.Background {
		if data, ok := value["data"].(map[string]any); ok {
			data["notice"] = "The user sent a new message while this command was running. The command was moved to the background and is still running; use the command read and stop tools with this id to check it. Its completion is reported automatically."
		}
	}
	if workspaceOutputFailed(output) {
		workspace := false
		text := ""
		if input, ok := request.Command.Data.(*protocol.CommandRunCommandPayload); ok {
			workspace = input.Access == protocol.ProcessAccessWorkspace
		}
		switch result := output.Data.(type) {
		case protocol.ProcessCompletion:
			text = result.Stdout.Text + "\n" + result.Stderr.Text
		case protocol.ProcessSnapshot:
			workspace = result.Info.Access == protocol.ProcessAccessWorkspace
			text = result.Stdout.Text + "\n" + result.Stderr.Text
		}
		if workspace {
			lower := strings.ToLower(text)
			for _, diagnostic := range []string{"operation not permitted", "permission denied", "sandbox", "dyld:", "dyld[", "could not resolve host", "network is unreachable"} {
				if strings.Contains(lower, diagnostic) {
					if data, ok := value["data"].(map[string]any); ok {
						data["sandbox_notice"] = "This command ran in the workspace sandbox: network access is disabled, writes are limited to the worktree and temporary directories, and existing Git metadata and managed child worktrees are protected. macOS also blocks new .git paths. Git writes, including git init, git add and git commit, require access=\"unrestricted\" under the current permission policy. If sandbox restrictions caused the failure, inspect any partial changes before requesting unrestricted access to retry. Unrestricted access does not override operating-system permissions."
					}
					break
				}
			}
		}
	}
	return value
}
func (node *Node) observedWorkspaceRevisions(ctx context.Context, session protocol.SessionID) (map[string]string, error) {
	result := map[string]string{}
	var state store.AgentState
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		state, err = store.LoadAgentState(ctx, db, session)
		return err
	})
	if err != nil {
		return nil, err
	}
	for _, record := range state.Records {
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				response, ok := block.Data.(*canonical.ToolResponse)
				if !ok || response.Status != canonical.ToolSuccess {
					continue
				}
				var value struct {
					Kind string `json:"kind"`
					Data struct {
						Path     string  `json:"path"`
						Revision *string `json:"revision"`
					} `json:"data"`
				}
				if json.Unmarshal(response.Result.Value, &value) == nil && value.Data.Revision != nil {
					switch value.Kind {
					case "file_content", "file_lines", "file_written", "notebook_page":
						result[value.Data.Path] = *value.Data.Revision
					}
				}
			}
		}
	}
	return result, nil
}
