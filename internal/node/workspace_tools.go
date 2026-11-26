package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"path/filepath"
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
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type workspaceToolCall struct {
	request        protocol.Request
	prepared       *files.Prepared
	approval       protocol.ApprovalID
	commandControl *process.ControlTarget
	fileScope      externalFileScope
	patchInput     string
	patchRoot      string
}

func extractPatchInput(op protocol.ToolOperation, args json.RawMessage) string {
	if op != protocol.ToolOperationApplyChanges {
		return ""
	}
	var patch struct {
		Input string `json:"input"`
	}
	if json.Unmarshal(args, &patch) != nil {
		return ""
	}
	return patch.Input
}

func (node *Node) refreshWorkspacePatch(ctx context.Context, call *workspaceToolCall, worktree string) error {
	if call.patchInput == "" || call.prepared == nil || call.prepared.Rejected != nil {
		return nil
	}
	if err := node.locks.waitFiles(ctx, call.patchRoot, preparedTargetPaths(call.prepared)); err != nil {
		return err
	}
	stale, err := files.PreparedStale(ctx, call.patchRoot, call.prepared)
	if err != nil || !stale {
		return err
	}
	fresh, err := files.RepreparePatch(ctx, call.patchRoot, call.patchInput, call.prepared)
	if err != nil {
		return err
	}
	target := call.patchRoot
	if call.fileScope.worktree != "" {
		target = worktree
	}
	if err := node.protectWorkspaceMutation(target, target, fresh.Command, fresh, call.fileScope); err != nil {
		return err
	}
	call.prepared, call.request.Command = fresh, fresh.Command
	return nil
}

func prepareWorkspaceToolCommand(ctx context.Context, inv *store.AgentInvocation, op protocol.ToolOperation, args json.RawMessage, root string, sandbox bool) (protocol.Command, *files.Prepared, error) {
	if op != protocol.ToolOperationApplyChanges {
		command, err := operations.Command(op, args, inv.Run.Worktree, inv.Session.ID, inv.Run.Turn, inv.Worktree.Project, sandbox && inv.Turn.Config.Permission != protocol.PermissionFull)
		return command, nil, err
	}
	var patch struct {
		Input string `json:"input"`
	}
	if err := json.Unmarshal(args, &patch); err != nil {
		return protocol.Command{}, nil, err
	}
	prepared, err := files.PreparePatch(ctx, root, patch.Input, inv.Run.Worktree, core.RestrictedFiles)
	if err != nil {
		return protocol.Command{}, nil, err
	}
	return prepared.Command, prepared, nil
}

const childRunCommandDescription = "Run a command on the Node. Provide a short description and use the admitted worktree by default. The foreground timeout defaults to 120000 ms. For longer work use background=true, then read_command with wait_ms to collect the result before returning to the parent. Background commands stop when the delegated task ends. Use pty=true with background=true for a command that needs a terminal. Output reports truncation. Invoke the tool directly so it can obtain consent when needed."

func releasePrepared[T any](mu *sync.Mutex, calls map[string]T) func(canonical.ToolRequest) {
	return func(call canonical.ToolRequest) {
		mu.Lock()
		delete(calls, call.ID)
		mu.Unlock()
	}
}

func (node *Node) workspaceTools(ctx context.Context, inv *store.AgentInvocation) ([]engine.Tool, error) {
	if _, err := node.workspaceRoot(ctx, inv.Run.Worktree, false); err != nil {
		return nil, err
	}
	node.suppressReplacedInstructions(inv)
	catalog, err := operations.CaptureCatalog(ctx, node.profile.Ownership.Path, inv.Turn.Plugins)
	if err != nil {
		return nil, err
	}
	commands, err := operations.CommandSpecs()
	if err != nil {
		return nil, err
	}
	coreCommands := inv.Turn.Config.Resource == nil || inv.Turn.Config.Assistant != nil
	legacyCommandAliases := !slices.ContainsFunc(inv.Turn.Plugins, func(reference protocol.PluginReference) bool {
		return reference.Name == "commands" && !legacyNativeToolPackage(reference.Name, reference.Digest)
	})
	declarations := slices.Clone(catalog.Tools)
	for _, command := range commands {
		declarations = append(declarations, operations.Declaration{Spec: command.Spec, Name: command.Name, Initial: command.Name != "write_command", Display: &command.Display})
	}
	var tools []engine.Tool
	for _, declaration := range declarations {
		packaged := declaration.Owner.Name != ""
		if packaged && node.hasReplacedPlugin(declaration.Owner.Name, declaration.Owner.Digest) {
			continue
		}
		if node.codeIndex == nil && codeIndexOperation(declaration.Operation) {
			continue
		}
		if kind := operations.WorkspaceModuleCommand(declaration.Operation); kind != "" && node.workspace.Commands[kind].Run == nil {
			continue
		}
		if coreCommands && declaration.Owner.Name == "commands" && operations.LegacyCommandTool(declaration.Name) != "" &&
			legacyNativeToolPackage(declaration.Owner.Name, declaration.Owner.Digest) {
			continue
		}
		if !availableOperationTool(inv, declaration) || packaged && !slices.Contains(inv.Turn.Plugins, declaration.Owner) || operations.ConnectionOperation(declaration.Operation) || knowledgeOperation(declaration.Operation) || storageOperation(declaration.Operation) || operations.ScopedOperation(declaration.Operation) {
			continue
		}
		// A connection conversation without an assistant offers only tools that name a context.
		if inv.Turn.Config.Resource != nil && inv.Turn.Config.Assistant == nil && len(declaration.Contexts) == 0 &&
			!strings.HasPrefix(string(declaration.Operation), "artifacts.") {
			continue
		}
		var schema map[string]any
		if err = json.Unmarshal(declaration.Parameters, &schema); err != nil {
			return nil, err
		}
		if inv.Session.Delegation != nil && declaration.Operation == protocol.ToolOperationRunCommand {
			if properties, ok := schema["properties"].(map[string]any); ok {
				// Child commands end with the delegated task.
				delete(properties, "follow_up")
				delete(properties, "service")
			}
			declaration.Parameters, err = json.Marshal(schema)
			if err != nil {
				return nil, err
			}
			declaration.Description = childRunCommandDescription
		}
		var mu sync.Mutex
		calls := map[string]*workspaceToolCall{}
		tool := engine.Tool{Definition: fantasy.FunctionTool{Name: declaration.Name, Description: declaration.Description, InputSchema: schema}, SchemaJSON: declaration.Parameters, ReadOnly: declaration.ReadOnly, Concurrent: declaration.Concurrent, Deferred: !declaration.Initial, Approval: !declaration.ReadOnly, Binding: &store.ApprovalBinding{Owner: declaration.Owner, Tool: declaration.Name, Operation: &declaration.Operation}}
		if !packaged && legacyCommandAliases && operations.IsCommandTool(declaration.Name) {
			tool.LegacyPackage = "commands"
		}
		switch declaration.Operation {
		case protocol.ToolOperationListDirectory, protocol.ToolOperationReadFile, protocol.ToolOperationReadFileLines,
			protocol.ToolOperationFindFiles, protocol.ToolOperationSearchFiles,
			protocol.ToolOperationSearchCode, protocol.ToolOperationOutlineCode, protocol.ToolOperationGraphCode,
			protocol.ToolOperationInspectGit, protocol.ToolOperationReadGitDiff, protocol.ToolOperationReadGitLog,
			protocol.ToolOperationReadGitShow, protocol.ToolOperationReadGitBlame, protocol.ToolOperationListGitChanges, protocol.ToolOperationReadGitChange:
			tool.StreamRead = inv.Worktree.Project != nil && tool.ReadOnly && tool.Concurrent
		}
		if !packaged && declaration.Display != nil {
			tool.SearchLabels = toolSearchLabels(*declaration.Display)
		}
		tool.ReleasePrepared = releasePrepared(&mu, calls)
		observed := workspaceRevisions{worktreeID: inv.Run.Worktree, worktree: inv.Worktree.Path}
		if observesRevisions(declaration.Operation) {
			tool.ObserveHistory = observed.update
		}
		tool.Prepare = func(ctx context.Context, call canonical.ToolRequest) error {
			sandbox := protocol.DefaultSandboxSettings()
			if declaration.Operation == protocol.ToolOperationRunCommand {
				// Each command follows the setting as it is now, not as it
				// was when the turn started.
				if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
					var err error
					sandbox, err = store.ReadSandboxSettings(ctx, db)
					return err
				}); err != nil {
					return err
				}
			}
			// Full access is the admitted turn's policy, including for children.
			// Normalize access before approval captures the command to execute.
			arguments, err := observed.resolve(declaration.Operation, call.Arguments)
			if err != nil {
				return err
			}
			if !declaration.ReadOnly {
				if err := node.locks.waitFiles(ctx, inv.Worktree.Path, collectFileArgPaths(declaration.Operation, arguments)); err != nil {
					return err
				}
			}
			command, patchChanges, err := prepareWorkspaceToolCommand(ctx, inv, declaration.Operation, arguments, inv.Worktree.Path, sandbox.Enabled)
			if err != nil {
				return err
			}
			if target, ok := commandWorktree(command); ok && target != inv.Run.Worktree {
				allowed := false
				if inv.Worktree.Project != nil {
					err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
						capture, e := store.ReadProjectWorktree(ctx, db, target)
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
			if packaged {
				request.Plugin = &protocol.PluginContext{Turn: &inv.Run.Turn, Package: declaration.Owner, Worktree: &inv.Run.Worktree, Session: &inv.Session.ID, Surface: protocol.PluginDesktopSurface{Kind: "workspace"}}
			}
			current := &workspaceToolCall{request: request, prepared: patchChanges, patchInput: extractPatchInput(declaration.Operation, arguments), patchRoot: inv.Worktree.Path}
			if current.prepared == nil && fileTextMutation(command.Kind) {
				current.prepared, err = files.Prepare(ctx, inv.Worktree.Path, command, core.RestrictedFiles)
				if err != nil {
					return err
				}
			}
			if files.SupportsMutation(command.Kind) {
				if err = node.protectWorkspaceMutation(inv.Worktree.Path, inv.Worktree.Path, command, current.prepared, externalFileScope{}); err != nil {
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
			switch input := current.request.Command.Data.(type) {
			case *protocol.CommandStopCommandPayload:
				if input.Session == inv.Session.ID {
					current.commandControl = node.processes.SandboxedControl(input.Session, input.ID)
				}
			case *protocol.CommandWriteCommandPayload:
				if input.Session == inv.Session.ID {
					current.commandControl = node.processes.SandboxedControl(input.Session, input.ID)
				}
			}
			if current.commandControl != nil {
				value.Access = new(protocol.ProcessAccessWorkspace)
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
			if declaration.Operation == protocol.ToolOperationApplyChanges && inv.Turn.Config.Permission == protocol.PermissionFull {
				if err := node.refreshWorkspacePatch(ctx, current, inv.Worktree.Path); err != nil {
					return workspaceToolFault(err)
				}
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
			effect := &workspaceCompletion{commandControl: current.commandControl}
			defer effect.close()
			var output protocol.Output
			var effectErr error
			if request.Command.Kind == "read_tool_output" {
				output, effectErr = node.agentRequest(ctx, inv, request, "command output read was interrupted")
			} else {
				output, effectErr = node.workspaceIO(ctx, inv.Caller, request, current.prepared, effect)
			}
			if snapshot, ok := output.Data.(protocol.ProcessSnapshot); effectErr == nil && request.Command.Kind == "stop_command" && ok {
				effect.external = true
				effect.commit = func(ctx context.Context, db *sql.Conn, out protocol.Output) (protocol.Output, []protocol.Event, error) {
					events, err := store.ObserveCommandStop(ctx, db, inv.Run.Turn, snapshot, time.Now())
					return out, events, err
				}
			}
			if !declaration.ReadOnly {
				output, effectErr = node.recordWorkspaceEffect(context.WithoutCancel(ctx), inv.Caller, request, output, effectErr, effect)
			}
			if effectErr != nil {
				return workspaceToolFault(effectErr)
			}
			if declaration.Operation == protocol.ToolOperationReadCommand {
				if snapshot, ok := output.Data.(protocol.ProcessSnapshot); ok {
					if err := node.consumeWorkspaceCommand(ctx, inv.Run.Turn, snapshot); err != nil {
						return workspaceToolFault(err)
					}
				}
			}
			value := node.workspaceToolValue(inv.Session, request, output, effect.timedOut)
			// A written artifact is delivered as its card, whichever package's
			// tool wrote it: the Node stamps the summary it committed.
			if written, ok := output.Data.(protocol.OutputArtifactWrittenPayload); ok && output.Kind == "artifact_written" {
				value = map[string]any{"kind": output.Kind, "data": output.Data, "sailry_content": protocol.ToolContent{Version: 1,
					Blocks: []protocol.ToolContentBlock{{Kind: "artifact", Data: protocol.ToolContentBlockArtifactPayload{Artifact: written.Artifact}}}}}
			} else if !packaged {
				if content := operations.CommandContent(output); content != nil {
					shaped, ok := value.(map[string]any)
					if !ok {
						shaped = map[string]any{"kind": output.Kind, "data": output.Data}
					}
					shaped["sailry_content"] = content
					value = shaped
				}
			}
			return engine.ToolResult{Value: value, ReturnedError: workspaceOutputFailed(output), ReturnedFailure: workspaceOutputFailure(output)}
		}
		tools = append(tools, tool)
	}
	connections, err := node.connectionTools(ctx, inv, catalog)
	if err != nil {
		return nil, err
	}
	indexed, err := node.knowledgeTools(inv, catalog)
	if err != nil {
		return nil, err
	}
	tools = append(tools, indexed...)
	stored, err := node.storageTools(inv, catalog)
	if err != nil {
		return nil, err
	}
	tools = append(tools, stored...)
	scoped, err := node.scopedTools(inv, catalog)
	if err != nil {
		return nil, err
	}
	tools = append(tools, scoped...)
	browsers, err := node.browserTools(ctx, inv)
	if err != nil {
		return nil, err
	}
	managedBrowsers, err := node.externalBrowserTools(ctx, inv)
	if err != nil {
		return nil, err
	}
	media, err := node.mediaTools(inv)
	if err != nil {
		return nil, err
	}
	managed, err := node.agentTools(inv)
	if err != nil {
		return nil, err
	}
	presented, err := node.presentTools(inv)
	if err != nil {
		return nil, err
	}
	managed = append(managed, presented...)
	assembled, err := node.moduleTools(ctx, inv)
	if err != nil {
		return nil, err
	}
	bound, err := node.pluginTools(ctx, inv, append(append(append(tools, connections...), browsers...), managedBrowsers...))
	return append(append(append(assembled, bound...), media...), managed...), err
}

func (node *Node) consumeWorkspaceCommand(ctx context.Context, turn protocol.TurnID, snapshot protocol.ProcessSnapshot) error {
	switch snapshot.Info.Status.Kind {
	case "running", "stopping":
		return nil
	}
	var pending bool
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		pending, err = store.CommandBackgroundPending(ctx, db, snapshot)
		return err
	})
	if err != nil || !pending {
		return err
	}
	unlock := node.agent.LockPublication()
	changed := false
	err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		events, err := store.ConsumeCommandBackground(ctx, db, turn, snapshot, time.Now())
		if err != nil || len(events) == 0 {
			return err
		}
		changed = true
		_, err = store.RecordEvents(ctx, db, node.ID(), events)
		return err
	})
	if err == nil && changed {
		node.publish()
		node.agent.RefreshCommitted(snapshot.Info.Session)
	}
	unlock()
	if err == nil && changed {
		node.agent.Committed()
	}
	return err
}

func toolSearchLabels(display protocol.ToolDisplay) []string {
	labels := []string{display.Label}
	for _, label := range display.Locales {
		labels = append(labels, label)
	}
	slices.Sort(labels)
	return slices.Compact(labels)
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
func (node *Node) workspaceToolValue(session protocol.Session, request protocol.Request, output protocol.Output, timedOut bool) any {
	if value, shaped := codeToolValue(output); shaped {
		return value
	}
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
		sessionID := session.ID
		if snapshot, ok := output.Data.(protocol.ProcessSnapshot); ok {
			sessionID = snapshot.Info.Session
			id = snapshot.Info.ID
		}
		if preview, err := node.processes.Preview(sessionID, id, output.Kind == "command_output"); err == nil {
			value["sailry_command"] = preview
		}
	}
	if input, ok := request.Command.Data.(*protocol.CommandRunCommandPayload); ok && !input.Background && timedOut && output.Kind == "command_result" {
		if data, ok := value["data"].(map[string]any); ok {
			data["notice"] = "Background command capacity was full, so the command was stopped at its timeout."
		}
	}
	if input, ok := request.Command.Data.(*protocol.CommandRunCommandPayload); ok && output.Kind == "command_started" && !input.Background {
		if data, ok := value["data"].(map[string]any); ok {
			notice := "New guidance arrived while this command was running. It moved to the background. Use read_command or stop_command with this id."
			if timedOut {
				notice = "The command reached timeout_ms and is still running in the background. Use read_command with this id (wait_ms, until) to follow it, or stop_command to end it."
			}
			if session.Delegation != nil {
				notice += " Read the result before returning to the parent. The command stops when this delegated task ends."
			} else {
				notice += " Its completion is reported automatically."
			}
			data["notice"] = notice
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

// workspaceRevisions follows the canonical history visible to this turn. Receipts
// from a successful write are observations of its new revision, like reads.
type workspaceRevisions struct {
	paths       map[string]string
	invalidated map[string]struct{} // Unordered child evidence cannot refill these observations.
	count       int
	last        string
	worktree    string
	worktreeID  protocol.WorktreeID
}

func (r *workspaceRevisions) update(records []canonical.Record) {
	if r.paths == nil || r.count > len(records) || r.count > 0 && records[r.count-1].ID != r.last {
		r.paths = map[string]string{}
		r.invalidated = nil
		r.count = 0
	}
	// Replaying receipts can revisit the same path many times. Cache its
	// filesystem spelling for this replay, not across changes on disk.
	keys := make(map[string]string)
	key := func(path string) string {
		if k, ok := keys[path]; ok {
			return k
		}
		k := r.pathKey(path)
		keys[path] = k
		return k
	}
	for _, record := range records[r.count:] {
		if record.Kind == "background" {
			if rt := record.Metadata.Runtime; rt != nil && rt.FileChanges != nil {
				if changes := rt.FileChanges[r.worktreeID.String()]; len(changes) != 0 {
					r.observeChildChanges(changes, key, combinesChildChanges(record.Background()))
				}
			}
			continue
		}
		message := record.Message()
		if message == nil || record.Provenance.Author != "tool" {
			continue
		}
		for _, block := range message.Content {
			response, ok := block.Data.(*canonical.ToolResponse)
			if !ok {
				if v, ok := block.Data.(canonical.ToolResponse); ok {
					response = &v
				} else {
					continue
				}
			}
			r.observe(response, key)
		}
	}
	r.count = len(records)
	if r.count > 0 {
		r.last = records[r.count-1].ID
	}
}

// Batched background delivery flattens child receipts into one path map. A
// value can already have overwritten a different revision for the same path.
func combinesChildChanges(background *canonical.Background) bool {
	if background == nil {
		return false
	}
	seenChild := false
	for _, result := range background.Results {
		if result.Notice.Kind.Kind == "agent" {
			if seenChild {
				return true
			}
			seenChild = true
		}
	}
	return false
}

func (r *workspaceRevisions) observeChildChanges(files map[string]string, key func(string) string, combined bool) {
	for path, rev := range files {
		k := key(path)
		if k == "" {
			continue
		}
		if rev == "" {
			r.invalidateTree(k)
			continue
		}
		if combined {
			r.invalidate(k)
			continue
		}
		// Delivery order is not write order. Conflicting evidence invalidates
		// the observation; a duplicate cannot revive it without a direct read
		// or write. This also handles differently spelled aliases in old maps.
		if known, ok := r.paths[k]; ok {
			if known != rev {
				r.invalidate(k)
			}
			continue
		}
		if !r.isInvalidated(k) {
			r.paths[k] = rev
		}
	}
}

func (r *workspaceRevisions) isInvalidated(path string) bool {
	if len(r.invalidated) == 0 {
		return false
	}
	for {
		if _, ok := r.invalidated[path]; ok {
			return true
		}
		i := strings.LastIndexByte(path, '/')
		if i <= 0 {
			return false
		}
		path = path[:i]
	}
}

func (r *workspaceRevisions) observe(response *canonical.ToolResponse, key func(string) string) {
	if response.Status == canonical.ToolSuccess && response.Meta != nil {
		if raw := (*response.Meta)[canonical.FileReadsKey(r.worktreeID)]; raw != nil {
			var files map[string]string
			if json.Unmarshal(raw, &files) == nil {
				for path, revision := range files {
					r.observeRevision(key(path), revision)
				}
			}
		}
		if raw := (*response.Meta)[canonical.FileChangesKey(r.worktreeID)]; raw != nil {
			var files map[string]string
			if json.Unmarshal(raw, &files) == nil {
				r.observeChildChanges(files, key, false)
			}
		}
	}
	var value struct {
		Kind string
		Data struct {
			Path, Revision, From, To string
			Files                    []struct {
				Path, Destination, Revision string
				Kind                        protocol.ChangeKind
				Status                      protocol.ChangeStatus
			}
		}
	}
	if json.Unmarshal(response.Result.Value, &value) != nil {
		return
	}
	data := value.Data
	if value.Kind == "file_changes" {
		for _, file := range data.Files {
			file.Path, file.Destination = key(file.Path), key(file.Destination)
			switch file.Status {
			case protocol.ChangeStatusUnknown:
				r.invalidate(file.Path)
				r.invalidate(file.Destination)
			case protocol.ChangeStatusApplied:
				switch file.Kind {
				case protocol.ChangeKindDeleted:
					r.invalidate(file.Path)
				case protocol.ChangeKindRenamed:
					r.move(file.Path, file.Destination)
				default:
					r.observeRevision(file.Path, file.Revision)
				}
			}
		}
		return
	}
	if response.Status != canonical.ToolSuccess {
		return
	}
	switch value.Kind {
	case "file_content", "file_lines", "file_written", "notebook_page":
		if data.Revision != "" {
			r.observeRevision(key(data.Path), data.Revision)
		}
	case "entry_trashed":
		r.invalidateTree(key(data.Path))
	case "entry_moved":
		// Generic entry operations may move directories across roots.
		r.invalidateTree(key(data.From))
		r.invalidateTree(key(data.To))
	}
}

func observesRevisions(operation protocol.ToolOperation) bool {
	switch operation {
	case protocol.ToolOperationWriteFile, protocol.ToolOperationEditFile,
		protocol.ToolOperationDeleteFile, protocol.ToolOperationMoveFile, protocol.ToolOperationEditNotebook:
		return true
	}
	return false
}

// resolve fills an omitted expected_revision of a single-file mutation with
// the revision this conversation last observed, so the admitted command
// carries a concrete revision and a later external change still conflicts.
// write_file never defaults to replacing: an omitted revision creates, so its
// approval prompt, which reads the model's arguments, names the real effect.
// Other mutations require a prior read.
func (r *workspaceRevisions) resolve(operation protocol.ToolOperation, arguments json.RawMessage) (json.RawMessage, error) {
	if !observesRevisions(operation) {
		return arguments, nil
	}
	var values map[string]json.RawMessage
	if json.Unmarshal(arguments, &values) != nil || values == nil {
		// operations.Command reports malformed arguments.
		return arguments, nil
	}
	if _, ok := values["expected_revision"]; ok {
		return arguments, nil
	}
	var path string
	if json.Unmarshal(values["path"], &path) != nil {
		return arguments, nil
	}
	if operation == protocol.ToolOperationWriteFile {
		values["expected_revision"] = json.RawMessage("null")
	} else {
		revision := r.paths[r.pathKey(path)]
		if revision == "" {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: path + " has no current observed revision; read it or pass expected_revision"}
		}
		values["expected_revision"], _ = json.Marshal(revision)
	}
	return json.Marshal(values)
}

func (r *workspaceRevisions) pathKey(path string) string {
	if path == "" {
		return ""
	}
	if r.worktree == "" && !filepath.IsAbs(path) {
		return filepath.ToSlash(filepath.Clean(path))
	}
	return workspaceFileKey(r.worktree, path)
}

func (r *workspaceRevisions) observeRevision(path, revision string) {
	if path == "" {
		return
	}
	if revision == "" {
		r.invalidate(path)
		return
	}
	r.paths[path] = revision
	delete(r.invalidated, path)
}

func (r *workspaceRevisions) invalidate(path string) {
	if path == "" {
		return
	}
	delete(r.paths, path)
	if r.invalidated == nil {
		r.invalidated = map[string]struct{}{}
	}
	r.invalidated[path] = struct{}{}
}

func (r *workspaceRevisions) invalidateTree(path string) {
	if path == "" {
		return
	}
	r.invalidate(path)
	prefix := path + "/"
	for known := range r.paths {
		if strings.HasPrefix(known, prefix) {
			delete(r.paths, known)
		}
	}
}

func (r *workspaceRevisions) move(from, to string) {
	if from != to {
		r.observeRevision(to, r.paths[from])
		r.invalidate(from)
	}
}
