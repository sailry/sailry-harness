package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"slices"
	"strings"
	"sync"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/externalbrowser"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type nodeModuleHost struct {
	node *Node
}

var _ moduleapi.Host = nodeModuleHost{}

func (h nodeModuleHost) NodeID() protocol.NodeID { return h.node.ID() }

func (h nodeModuleHost) HasCodeIndex() bool { return h.node.codeIndex != nil }

func (h nodeModuleHost) BrowserAvailable(inv *store.AgentInvocation) bool {
	return h.node.browsers.available(inv.Caller)
}

func (h nodeModuleHost) ExternalBrowserAvailable() bool {
	return h.node.externalBrowser.Available()
}

func (h nodeModuleHost) ExternalBrowserCloseOnCancel(ctx context.Context, session protocol.SessionID) func() error {
	var closing sync.Once
	return func() error {
		if ctx.Err() != nil {
			closing.Do(func() { h.node.externalBrowser.Close(session) })
		}
		return nil
	}
}

func (h nodeModuleHost) PluginCaptured(inv *store.AgentInvocation, name string) (protocol.PluginReference, bool) {
	for _, ref := range inv.Turn.Plugins {
		if ref.Name == name {
			return ref, true
		}
	}
	return protocol.PluginReference{}, false
}

func (h nodeModuleHost) PluginInstalled(ctx context.Context, name string) (bool, bool, error) {
	var installed, enabled bool
	err := h.node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		current, err := store.ReadPlugin(ctx, db, name)
		if err != nil {
			return err
		}
		if current != nil {
			installed = true
			enabled = current.Summary.Enabled
		}
		return nil
	})
	return installed, enabled, err
}

func (h nodeModuleHost) pluginGateEnabled(ctx context.Context, name string) error {
	if name == "" {
		return nil
	}
	installed, enabled, err := h.PluginInstalled(ctx, name)
	if err != nil {
		return err
	}
	if installed && !enabled {
		return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "plugin is disabled"}
	}
	return nil
}

func (h nodeModuleHost) ListLocalModels(_ context.Context, kind protocol.ModelsKind) ([]protocol.ModelsLocalModel, error) {
	if h.node.localModels == nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "local models are unavailable"}
	}
	return h.node.localModels.registry.List(kind)
}

func (h nodeModuleHost) ConnectionCatalog(ctx context.Context, inv *store.AgentInvocation) (store.ConnectionCatalog, error) {
	return h.node.connectionCatalog(ctx, inv)
}

func (h nodeModuleHost) CaptureConnectionTargets(inv *store.AgentInvocation, targets []protocol.ConnectionResource) {
	inv.ConnectionTargets = targets
}

func (h nodeModuleHost) RunWorkspaceOperation(ctx context.Context, inv *store.AgentInvocation, op protocol.ToolOperation, args json.RawMessage) (protocol.Output, error) {
	readOnly, _ := operations.Policy(op)
	if !readOnly {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "workflow helper only runs read-only workspace operations"}
	}
	if op == protocol.ToolOperationSearchCode || op == protocol.ToolOperationOutlineCode || op == protocol.ToolOperationGraphCode {
		if h.node.codeIndex == nil {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: codeIndexUnavailable}
		}
	}
	command, err := operations.Command(op, args, inv.Run.Worktree, inv.Session.ID, inv.Run.Turn, inv.Worktree.Project, true)
	if err != nil {
		return protocol.Output{}, err
	}
	request := protocol.NewRequest(h.node.ID(), command)
	effect := &workspaceCompletion{}
	defer effect.close()
	return h.node.workspaceIO(ctx, inv.Caller, request, nil, effect)
}

func (h nodeModuleHost) WorkspaceTool(inv *store.AgentInvocation, cfg moduleapi.WorkspaceToolConfig) (engine.Tool, error) {
	spec := cfg.Spec
	var schema map[string]any
	if err := json.Unmarshal(spec.Parameters, &schema); err != nil {
		return engine.Tool{}, err
	}
	readOnly, concurrent := spec.ReadOnly, spec.Concurrent
	if spec.Operation != "" {
		readOnly, concurrent = operations.Policy(spec.Operation)
	}
	op := spec.Operation
	var mu sync.Mutex
	calls := map[string]*workspaceToolCall{}
	preparedArgs := map[string]json.RawMessage{}
	tool := engine.Tool{
		Definition:      fantasy.FunctionTool{Name: spec.Name, Description: spec.Description, InputSchema: schema},
		SchemaJSON:      spec.Parameters,
		ReadOnly:        readOnly,
		Concurrent:      concurrent,
		StreamRead:      spec.StreamRead && inv.Worktree.Project != nil && readOnly && concurrent,
		Deferred:        !spec.Initial,
		Approval:        !readOnly,
		Binding:         &store.ApprovalBinding{Tool: spec.Name, Operation: &op},
		LegacyPackage:   spec.LegacyPackage,
		LegacyName:      spec.LegacyName,
		Instruction:     strings.TrimSpace(cfg.Instruction),
		TurnInstruction: cfg.TurnInstruction,
		ContextParts:    cfg.ContextParts,
		SearchLabels:    spec.SearchLabels(),
	}
	tool.ReleasePrepared = func(call canonical.ToolRequest) {
		mu.Lock()
		delete(calls, call.ID)
		delete(preparedArgs, call.ID)
		mu.Unlock()
	}
	var observed workspaceRevisions
	if op == protocol.ToolOperationApplyChanges {
		tool.ObserveHistory = observed.update
	}
	tool.Prepare = func(ctx context.Context, call canonical.ToolRequest) error {
		args := call.Arguments
		if cfg.PrepareArgs != nil {
			var err error
			args, err = cfg.PrepareArgs(ctx, args)
			if err != nil {
				return err
			}
		}
		sandbox := protocol.DefaultSandboxSettings()
		if op == protocol.ToolOperationRunCommand {
			if err := h.node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				var err error
				sandbox, err = store.ReadSandboxSettings(ctx, db)
				return err
			}); err != nil {
				return err
			}
		}
		command, err := operations.Command(op, args, inv.Run.Worktree, inv.Session.ID, inv.Run.Turn, inv.Worktree.Project, sandbox.Enabled && inv.Turn.Config.Permission != protocol.PermissionFull)
		if op == protocol.ToolOperationApplyChanges {
			var patch struct {
				Input string `json:"input"`
			}
			if err = json.Unmarshal(args, &patch); err != nil {
				return err
			}
			changes, e := files.ResolvePatchWithRevisions(ctx, inv.Worktree.Path, patch.Input, observed.paths, core.RestrictedFiles)
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
				err = h.node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
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
		request := protocol.NewRequest(h.node.ID(), command)
		current := &workspaceToolCall{request: request}
		if fileTextMutation(command.Kind) {
			current.prepared, err = files.Prepare(ctx, inv.Worktree.Path, command, core.RestrictedFiles)
			if err != nil {
				return err
			}
		}
		if files.SupportsMutation(command.Kind) {
			if err = h.node.protectWorkspaceMutation(inv.Worktree.Path, inv.Worktree.Path, command, current.prepared); err != nil {
				return err
			}
		}
		mu.Lock()
		calls[call.ID] = current
		preparedArgs[call.ID] = args
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
		return store.ConfigureWorkspaceApproval(ctx, db, inv.Run.Turn, value, op, current.request.Command, changes)
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
		args := preparedArgs[call.ID]
		delete(calls, call.ID)
		delete(preparedArgs, call.ID)
		mu.Unlock()
		if current == nil {
			return workspaceToolFault(errors.New("native tool was not prepared"))
		}
		request := current.request
		if !readOnly {
			body, err := json.Marshal(request)
			if err != nil {
				return workspaceToolFault(err)
			}
			unlock := h.node.agent.LockPublication()
			err = h.node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
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
		effect := &workspaceCompletion{commandControl: current.commandControl}
		defer effect.close()
		output, effectErr := h.node.workspaceIO(ctx, inv.Caller, request, current.prepared, effect)
		if !readOnly {
			output, effectErr = h.node.recordWorkspaceEffect(context.WithoutCancel(ctx), inv.Caller, request, output, effectErr, effect)
		}
		if effectErr != nil {
			return workspaceToolFault(effectErr)
		}
		value := h.node.workspaceToolValue(inv.Session, request, output)
		if written, ok := output.Data.(protocol.OutputArtifactWrittenPayload); ok && output.Kind == "artifact_written" {
			value = map[string]any{"kind": output.Kind, "data": output.Data, "sailry_content": protocol.ToolContent{Version: 1,
				Blocks: []protocol.ToolContentBlock{{Kind: "artifact", Data: protocol.ToolContentBlockArtifactPayload{Artifact: written.Artifact}}}}}
		}
		result := engine.ToolResult{Value: value, ReturnedError: workspaceOutputFailed(output), ReturnedFailure: workspaceOutputFailure(output)}
		if cfg.FormatResult != nil && result.Failure == nil {
			result = cfg.FormatResult(ctx, args, result)
		}
		return result
	}
	return tool, nil
}

func (h nodeModuleHost) ScopedTool(inv *store.AgentInvocation, cfg moduleapi.ScopedToolConfig) (engine.Tool, error) {
	spec := cfg.Spec
	var schema map[string]any
	if err := json.Unmarshal(spec.Parameters, &schema); err != nil {
		return engine.Tool{}, err
	}
	op := spec.Operation
	readOnly, concurrent := operations.Policy(op)
	scope := protocol.PluginContext{Turn: &inv.Run.Turn, Worktree: &inv.Run.Worktree, Session: &inv.Session.ID, Surface: protocol.PluginDesktopSurface{Kind: "workspace"}}
	var mu sync.Mutex
	calls := map[string]*protocol.Request{}
	tool := engine.Tool{
		Definition:    fantasy.FunctionTool{Name: spec.Name, Description: spec.Description, InputSchema: schema},
		SchemaJSON:    spec.Parameters,
		ReadOnly:      readOnly,
		Concurrent:    concurrent,
		Deferred:      !spec.Initial,
		Approval:      !readOnly,
		Binding:       &store.ApprovalBinding{Tool: spec.Name, Operation: &op},
		LegacyPackage: spec.LegacyPackage,
		LegacyName:    spec.LegacyName,
		Instruction:   strings.TrimSpace(cfg.Instruction),
		SearchLabels:  spec.SearchLabels(),
	}
	tool.ReleasePrepared = releasePrepared(&mu, calls)
	tool.Prepare = func(ctx context.Context, call canonical.ToolRequest) error {
		args := call.Arguments
		if cfg.PrepareArgs != nil {
			var err error
			args, err = cfg.PrepareArgs(ctx, args)
			if err != nil {
				return err
			}
		}
		command, err := operations.ScopedCommand(op, args, inv.Session.ID)
		if err != nil {
			return err
		}
		if save, saving := command.Data.(*protocol.CommandPutRolePayload); saving {
			if err := store.ValidateRole(save.Role); err != nil {
				return err
			}
		}
		request := protocol.NewRequest(h.node.ID(), command)
		request.Plugin = &scope
		mu.Lock()
		calls[call.ID] = &request
		mu.Unlock()
		return nil
	}
	tool.ConfigureApproval = func(ctx context.Context, db *sql.Conn, call canonical.ToolRequest, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
		mu.Lock()
		command := calls[call.ID].Command
		mu.Unlock()
		return store.ConfigureWorkspaceApproval(ctx, db, inv.Run.Turn, value, op, command, nil)
	}
	tool.Approved = func(call canonical.ToolRequest, _ protocol.ApprovalID, request protocol.RequestID) {
		mu.Lock()
		defer mu.Unlock()
		calls[call.ID].ID = request
	}
	tool.Execute = func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
		mu.Lock()
		request := calls[call.ID]
		delete(calls, call.ID)
		mu.Unlock()
		if request == nil {
			return workspaceToolFault(errors.New("native tool was not prepared"))
		}
		output, err := h.node.connectionDispatch(ctx, *request, connectionFault(protocol.ErrorCodeCancelled, "tool was cancelled"),
			connectionFault(protocol.ErrorCodeOutcomeUnknown, "tool response is unavailable"))
		if err != nil {
			return workspaceToolFault(err)
		}
		value, err := operations.ScopedOutput(op, output)
		if err != nil {
			return workspaceToolFault(err)
		}
		return engine.ToolResult{Value: value}
	}
	return tool, nil
}

func (h nodeModuleHost) ConnectionTool(inv *store.AgentInvocation, cfg moduleapi.ConnectionToolConfig) (engine.Tool, error) {
	spec := cfg.Spec
	var schema map[string]any
	if err := json.Unmarshal(spec.Parameters, &schema); err != nil {
		return engine.Tool{}, err
	}
	op := spec.Operation
	readOnly, concurrent := operations.Policy(op)
	owner := protocol.PluginReference{}
	scope := protocol.PluginContext{Turn: &inv.Run.Turn, Package: owner, Worktree: &inv.Run.Worktree, Session: &inv.Session.ID, Surface: protocol.PluginDesktopSurface{Kind: "workspace"}}
	var mu sync.Mutex
	calls := map[string]*connectionToolCall{}
	preparedArgs := map[string]json.RawMessage{}
	tool := engine.Tool{
		Definition:      fantasy.FunctionTool{Name: spec.Name, Description: spec.Description, InputSchema: schema},
		SchemaJSON:      spec.Parameters,
		ReadOnly:        readOnly,
		Concurrent:      concurrent,
		Deferred:        !spec.Initial,
		Approval:        !readOnly,
		Binding:         &store.ApprovalBinding{Owner: owner, Tool: spec.Name, Operation: &op},
		LegacyPackage:   spec.LegacyPackage,
		LegacyName:      spec.LegacyName,
		Instruction:     strings.TrimSpace(cfg.Instruction),
		TurnInstruction: cfg.TurnInstruction,
		SearchLabels:    spec.SearchLabels(),
	}
	tool.ReleasePrepared = func(call canonical.ToolRequest) {
		mu.Lock()
		delete(calls, call.ID)
		delete(preparedArgs, call.ID)
		mu.Unlock()
	}
	decl := operations.Declaration{Spec: operations.Spec{Operation: op}, Owner: owner, Name: spec.Name}
	tool.Prepare = func(ctx context.Context, call canonical.ToolRequest) error {
		args := call.Arguments
		if cfg.PrepareArgs != nil {
			var err error
			args, err = cfg.PrepareArgs(ctx, args)
			if err != nil {
				return err
			}
		}
		command, err := h.node.connectionCommand(ctx, inv, decl, args, cfg.Captured)
		if err != nil {
			return err
		}
		current := &connectionToolCall{request: protocol.NewRequest(h.node.ID(), command)}
		current.request.Plugin = &scope
		if current.trust, err = h.node.connectionPreflight(ctx, inv, owner, scope, command, cfg.Captured); err != nil {
			return err
		}
		mu.Lock()
		calls[call.ID] = current
		preparedArgs[call.ID] = args
		mu.Unlock()
		return nil
	}
	tool.ConfigureApproval = func(ctx context.Context, db *sql.Conn, call canonical.ToolRequest, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
		mu.Lock()
		current := calls[call.ID]
		mu.Unlock()
		if current.trust != nil {
			if err := store.ValidateConnectionTrust(ctx, db, inv.Run.Turn, tool.Binding, *current.trust); err != nil {
				return value, err
			}
			value.Ssh = current.trust
			value.State, value.Source = protocol.ConversationApprovalStatePending, protocol.ConversationApprovalSourceUser
			value.Choice, value.Prefix, value.CanRemember = nil, nil, false
		}
		return store.ConfigureWorkspaceApproval(ctx, db, inv.Run.Turn, value, op, current.request.Command, nil)
	}
	tool.Approved = func(call canonical.ToolRequest, _ protocol.ApprovalID, request protocol.RequestID) {
		mu.Lock()
		defer mu.Unlock()
		calls[call.ID].request.ID = request
	}
	tool.Execute = func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
		mu.Lock()
		current := calls[call.ID]
		args := preparedArgs[call.ID]
		delete(calls, call.ID)
		delete(preparedArgs, call.ID)
		mu.Unlock()
		if current == nil {
			return workspaceToolFault(errors.New("native tool was not prepared"))
		}
		if current.trust != nil {
			revision, err := h.node.connectionTrust(ctx, scope, *current.trust)
			if err != nil {
				return workspaceToolFault(err)
			}
			switch input := current.request.Command.Data.(type) {
			case *protocol.CommandRunSshPayload:
				input.ExpectedRevision = revision
			case *protocol.CommandTransferSshPayload:
				input.ExpectedRevision = revision
			}
		}
		stopped := connectionFault(protocol.ErrorCodeCancelled, "connection tool was cancelled")
		if connectionCancel(current.request.Command.Kind) != "" {
			stopped = connectionFault(protocol.ErrorCodeOutcomeUnknown, "connection operation was cancelled; inspect its outcome before retrying")
		}
		output, err := h.node.connectionDispatch(ctx, current.request, stopped, connectionFault(protocol.ErrorCodeOutcomeUnknown, "connection operation outcome is unavailable"))
		if err != nil {
			return workspaceToolFault(err)
		}
		value, err := h.node.connectionResult(ctx, inv, owner, output, cfg.Captured)
		if err != nil {
			return workspaceToolFault(err)
		}
		result := engine.ToolResult{Value: value}
		if cfg.FormatResult != nil {
			result = cfg.FormatResult(ctx, args, result)
		}
		return result
	}
	return tool, nil
}

func (h nodeModuleHost) BrowserTool(inv *store.AgentInvocation, cfg moduleapi.BrowserToolConfig) (engine.Tool, error) {
	spec := cfg.Spec
	var schema map[string]any
	if err := json.Unmarshal(spec.Parameters, &schema); err != nil {
		return engine.Tool{}, err
	}
	op := spec.Operation
	readOnly := op == browserReadOperation
	var mu sync.Mutex
	calls := map[string]*protocol.Request{}
	tool := engine.Tool{
		Definition:    fantasy.FunctionTool{Name: spec.Name, Description: spec.Description, InputSchema: schema},
		SchemaJSON:    spec.Parameters,
		ReadOnly:      readOnly,
		Deferred:      !spec.Initial,
		Approval:      !readOnly,
		Binding:       &store.ApprovalBinding{Tool: spec.Name, Operation: &op},
		LegacyPackage: spec.LegacyPackage,
		LegacyName:    spec.LegacyName,
		Instruction:   strings.TrimSpace(cfg.Instruction),
		SearchLabels:  spec.SearchLabels(),
	}
	tool.Confirm = func(call canonical.ToolRequest) bool {
		mu.Lock()
		defer mu.Unlock()
		current := calls[call.ID]
		return current != nil && browserRequiresApproval(current.Command.Data.(*protocol.CommandUseBrowserPayload).Action)
	}
	tool.ReleasePrepared = releasePrepared(&mu, calls)
	tool.Prepare = func(ctx context.Context, call canonical.ToolRequest) error {
		args := call.Arguments
		if cfg.PrepareArgs != nil {
			var err error
			args, err = cfg.PrepareArgs(ctx, args)
			if err != nil {
				return err
			}
		}
		var input struct {
			Action    string                     `json:"action"`
			Arguments map[string]json.RawMessage `json:"arguments"`
		}
		if json.Unmarshal(args, &input) != nil || input.Arguments == nil {
			return browserFault(protocol.ErrorCodeInvalidRequest, "invalid browser tool arguments")
		}
		input.Arguments["action"], _ = json.Marshal(input.Action)
		encoded, err := json.Marshal(input.Arguments)
		if err != nil {
			return browserFault(protocol.ErrorCodeInvalidRequest, "invalid browser tool arguments")
		}
		var action protocol.BrowserAction
		if json.Unmarshal(encoded, &action) != nil {
			return browserFault(protocol.ErrorCodeInvalidRequest, "invalid browser tool arguments")
		}
		if action.Kind != "screenshot" && browserRequiresApproval(action) == readOnly {
			return browserFault(protocol.ErrorCodePermissionDenied, "browser action does not match the declared operation")
		}
		if err := validateBrowserAction(action); err != nil {
			return err
		}
		if readOnly && browserRequiresApproval(action) && inv.Turn.Config.Mode == protocol.WorkModePlan {
			return browserFault(protocol.ErrorCodePermissionDenied, "planning turns cannot save browser captures")
		}
		request := protocol.NewRequest(h.node.ID(), protocol.Command{Kind: "use_browser", Data: &protocol.CommandUseBrowserPayload{Session: inv.Session.ID, Worktree: inv.Run.Worktree, Action: action}})
		mu.Lock()
		calls[call.ID] = &request
		mu.Unlock()
		return nil
	}
	tool.ConfigureApproval = func(ctx context.Context, db *sql.Conn, call canonical.ToolRequest, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
		mu.Lock()
		current := calls[call.ID]
		mu.Unlock()
		if current == nil {
			return value, errors.New("native tool was not prepared")
		}
		return store.ConfigureWorkspaceApproval(ctx, db, inv.Run.Turn, value, op, current.Command, nil)
	}
	tool.Approved = func(call canonical.ToolRequest, _ protocol.ApprovalID, request protocol.RequestID) {
		mu.Lock()
		defer mu.Unlock()
		if current := calls[call.ID]; current != nil {
			current.ID = request
		}
	}
	tool.Execute = func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
		mu.Lock()
		current := calls[call.ID]
		delete(calls, call.ID)
		mu.Unlock()
		if current == nil {
			return workspaceToolFault(errors.New("native tool was not prepared"))
		}
		if err := h.pluginGateEnabled(ctx, cfg.PluginGate); err != nil {
			return workspaceToolFault(err)
		}
		output, err := h.node.useBrowser(ctx, inv.Caller, current.ID, *current.Command.Data.(*protocol.CommandUseBrowserPayload))
		if err != nil {
			return workspaceToolFault(err)
		}
		res := engine.ToolResult{Value: map[string]any{"kind": output.Kind, "data": output.Data}}
		if cfg.FormatResult != nil {
			res = cfg.FormatResult(ctx, res)
		}
		return res
	}
	return tool, nil
}

func (h nodeModuleHost) ExternalBrowserTool(inv *store.AgentInvocation, cfg moduleapi.ExternalBrowserToolConfig) (engine.Tool, error) {
	spec := cfg.Spec
	var schema map[string]any
	if err := json.Unmarshal(spec.Parameters, &schema); err != nil {
		return engine.Tool{}, err
	}
	op := spec.Operation
	readOnly := op == protocol.ToolOperationReadExternalBrowser
	var mu sync.Mutex
	calls := map[string]*protocol.Request{}
	tool := engine.Tool{
		Definition:    fantasy.FunctionTool{Name: spec.Name, Description: spec.Description, InputSchema: schema},
		SchemaJSON:    spec.Parameters,
		ReadOnly:      readOnly,
		Deferred:      !spec.Initial,
		Approval:      !readOnly,
		Binding:       &store.ApprovalBinding{Tool: spec.Name, Operation: &op},
		LegacyPackage: spec.LegacyPackage,
		LegacyName:    spec.LegacyName,
		Instruction:   strings.TrimSpace(cfg.Instruction),
		SearchLabels:  spec.SearchLabels(),
		Close:         cfg.Close,
	}
	tool.ReleasePrepared = releasePrepared(&mu, calls)
	tool.Prepare = func(ctx context.Context, call canonical.ToolRequest) error {
		args := call.Arguments
		if cfg.PrepareArgs != nil {
			var err error
			args, err = cfg.PrepareArgs(ctx, args)
			if err != nil {
				return err
			}
		}
		var input struct {
			Action    protocol.ExternalBrowserAction `json:"action"`
			Arguments json.RawMessage                `json:"arguments"`
		}
		decoder := json.NewDecoder(bytes.NewReader(args))
		decoder.DisallowUnknownFields()
		if decoder.Decode(&input) != nil || input.Arguments == nil {
			return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid browser tool arguments"}
		}
		if input.Action.ReadOnly() != readOnly {
			return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "browser action does not match the declared operation"}
		}
		if _, err := externalbrowser.Validate(input.Action, input.Arguments); err != nil {
			return err
		}
		request := protocol.NewRequest(h.node.ID(), protocol.Command{Kind: "use_external_browser", Data: &protocol.CommandUseExternalBrowserPayload{
			Session: inv.Session.ID, Worktree: inv.Run.Worktree, Action: input.Action, Arguments: input.Arguments}})
		mu.Lock()
		calls[call.ID] = &request
		mu.Unlock()
		return nil
	}
	if !readOnly {
		tool.ConfigureApproval = func(ctx context.Context, db *sql.Conn, call canonical.ToolRequest, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
			mu.Lock()
			current := calls[call.ID]
			mu.Unlock()
			if current == nil {
				return value, errors.New("native tool was not prepared")
			}
			return store.ConfigureWorkspaceApproval(ctx, db, inv.Run.Turn, value, op, current.Command, nil)
		}
		tool.Approved = func(call canonical.ToolRequest, _ protocol.ApprovalID, request protocol.RequestID) {
			mu.Lock()
			defer mu.Unlock()
			if current := calls[call.ID]; current != nil {
				current.ID = request
			}
		}
	}
	tool.Execute = func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
		mu.Lock()
		current := calls[call.ID]
		delete(calls, call.ID)
		mu.Unlock()
		if current == nil {
			return workspaceToolFault(errors.New("native tool was not prepared"))
		}
		input := current.Command.Data.(*protocol.CommandUseExternalBrowserPayload)
		output, err := h.node.externalBrowser.Execute(ctx, externalbrowser.Call{
			Request: current.ID, Session: input.Session, Root: inv.Worktree.Path, Action: input.Action, Arguments: input.Arguments,
			Check: func(ctx context.Context) error { return h.pluginGateEnabled(ctx, cfg.PluginGate) },
		})
		if err != nil {
			return workspaceToolFault(err)
		}
		res := engine.ToolResult{Value: map[string]any{"kind": output.Kind, "data": output.Data}}
		if cfg.FormatResult != nil {
			res = cfg.FormatResult(ctx, res)
		}
		return res
	}
	return tool, nil
}

func (h nodeModuleHost) storageScope(ctx context.Context, inv *store.AgentInvocation, namespace string) (context.Context, protocol.PluginContext, error) {
	ctx = store.WithCapturedUser(store.WithUser(ctx, inv.User), inv.StorageUser)
	ref, captured := h.PluginCaptured(inv, namespace)
	if !captured {
		err := h.node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			current, err := store.ReadPlugin(ctx, db, namespace)
			if err != nil {
				return err
			}
			if current != nil {
				ref = plugins.PackageReference(*current)
			}
			return nil
		})
		if err != nil {
			return ctx, protocol.PluginContext{}, err
		}
	}
	scope := protocol.PluginContext{
		Package:  ref,
		Worktree: &inv.Run.Worktree,
		Session:  &inv.Session.ID,
		Surface:  protocol.PluginDesktopSurface{Kind: "workspace"},
	}
	if captured {
		scope.Turn = &inv.Run.Turn
	}
	return ctx, scope, nil
}

func (h nodeModuleHost) PluginStorageGet(ctx context.Context, inv *store.AgentInvocation, namespace, key string) (protocol.PluginStorageEntry, error) {
	ctx, scope, err := h.storageScope(ctx, inv, namespace)
	if err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	if scope.Package.Name == "" {
		return protocol.PluginStorageEntry{}, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: namespace + " storage package is not installed"}
	}
	request := protocol.NewRequest(h.node.ID(), protocol.Command{Kind: "read_plugin_value", Data: &protocol.CommandReadPluginValuePayload{Key: key, Scope: protocol.PluginStorageScopeNode}})
	request.Plugin = &scope
	output, err := h.node.connectionDispatch(ctx, request, connectionFault(protocol.ErrorCodeCancelled, "storage read cancelled"), connectionFault(protocol.ErrorCodeOutcomeUnknown, "storage read unavailable"))
	if err != nil {
		return protocol.PluginStorageEntry{}, err
	}
	entry, ok := dispatchPayload[protocol.PluginStorageEntry](output.Data)
	if !ok {
		return protocol.PluginStorageEntry{}, &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "unexpected storage read result"}
	}
	return entry, nil
}

func (h nodeModuleHost) PluginStorageSearch(ctx context.Context, inv *store.AgentInvocation, namespace string, query protocol.PluginStorageSearch) (protocol.PluginStorageSearchPage, error) {
	ctx, scope, err := h.storageScope(ctx, inv, namespace)
	if err != nil {
		return protocol.PluginStorageSearchPage{}, err
	}
	if scope.Package.Name == "" {
		return protocol.PluginStorageSearchPage{}, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: namespace + " storage package is not installed"}
	}
	if query.Scope == "" {
		query.Scope = protocol.PluginStorageScopeNode
	}
	request := protocol.NewRequest(h.node.ID(), protocol.Command{Kind: "search_plugin_values", Data: &query})
	request.Plugin = &scope
	output, err := h.node.connectionDispatch(ctx, request, connectionFault(protocol.ErrorCodeCancelled, "storage search cancelled"), connectionFault(protocol.ErrorCodeOutcomeUnknown, "storage search unavailable"))
	if err != nil {
		return protocol.PluginStorageSearchPage{}, err
	}
	page, ok := dispatchPayload[protocol.PluginStorageSearchPage](output.Data)
	if !ok {
		return protocol.PluginStorageSearchPage{}, &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "unexpected storage search result"}
	}
	return page, nil
}

func (h nodeModuleHost) PluginStorageTransaction(ctx context.Context, inv *store.AgentInvocation, namespace string, ops []protocol.PluginTransactionOperation) ([]protocol.Output, error) {
	if inv.Turn.Config.Mode == protocol.WorkModePlan {
		return nil, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "planning mode permits read-only tools"}
	}
	ctx, scope, err := h.storageScope(ctx, inv, namespace)
	if err != nil {
		return nil, err
	}
	if scope.Package.Name == "" {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: namespace + " storage package is not installed"}
	}
	request := protocol.NewRequest(h.node.ID(), protocol.Command{Kind: "plugin_transaction", Data: &protocol.CommandPluginTransactionPayload{Operations: ops}})
	request.Plugin = &scope
	output, err := h.node.connectionDispatch(ctx, request, connectionFault(protocol.ErrorCodeCancelled, "storage transaction cancelled"), connectionFault(protocol.ErrorCodeOutcomeUnknown, "storage transaction outcome is unavailable"))
	if err != nil {
		return nil, err
	}
	results, ok := dispatchPayload[[]protocol.Output](output.Data)
	if !ok {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "unexpected storage transaction result"}
	}
	return results, nil
}

func (h nodeModuleHost) ConfigureStorageApproval(ctx context.Context, db *sql.Conn, turn protocol.TurnID, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
	return store.ConfigureWorkspaceApproval(ctx, db, turn, value, protocol.ToolOperationStorageTransaction, protocol.Command{}, nil)
}

func (node *Node) moduleToolPresentation(name string) (protocol.ToolPresentation, protocol.ToolGrouping, *protocol.ToolDisplay, bool) {
	meta, ok := node.modules.LookupTool(name)
	if !ok {
		return "", "", nil, false
	}
	return meta.Presentation, meta.Grouping, meta.Display, true
}

func (node *Node) moduleTools(ctx context.Context, inv *store.AgentInvocation) ([]engine.Tool, error) {
	if len(node.modules.Modules) == 0 {
		return nil, nil
	}
	host := nodeModuleHost{node: node}
	var tools []engine.Tool
	for _, module := range node.modules.Modules {
		built, err := module.ToolsForTurn(ctx, host, inv)
		if err != nil {
			for _, tool := range tools {
				if tool.Close != nil {
					_ = tool.Close()
				}
			}
			return nil, err
		}
		tools = append(tools, built...)
	}
	return tools, nil
}

func (node *Node) hasReplacedPlugin(name string) bool {
	return node.modules.ReplacesPlugin(name)
}

// suppressReplacedInstructions removes legacy plugin manifest instructions for
// packages superseded by assembled native modules so instructions are not duplicated.
func (node *Node) suppressReplacedInstructions(inv *store.AgentInvocation) {
	if len(node.modules.Modules) == 0 || len(inv.Instructions) == 0 {
		return
	}
	inv.Instructions = slices.DeleteFunc(slices.Clone(inv.Instructions), func(item store.AgentInstruction) bool {
		if len(item.AnyTools) == 0 {
			return false
		}
		for _, alias := range item.AnyTools {
			if _, replaced := node.modules.LookupTool(alias); replaced {
				return true
			}
		}
		return false
	})
}
