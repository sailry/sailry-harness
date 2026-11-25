package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"slices"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/agent/prompt"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginScriptTurn struct {
	node       *Node
	invocation *store.AgentInvocation
	info       protocol.PluginInfo
	bundle     plugins.ScriptBundle
	scope      protocol.PluginContext
	mu         sync.Mutex
	state      json.RawMessage
	// writes counts each tool call's durable SDK writes across its
	// evaluations: a flow's steps, a handler and its result formatter.
	writes map[protocol.RequestID]uint64
}

func (owner *pluginScriptTurn) evaluate(ctx context.Context, handler, tool, call string, input any, readOnly bool) (json.RawMessage, error) {
	owner.mu.Lock()
	defer owner.mu.Unlock()
	identity := owner.callIdentity(tool, call)
	execution := &plugins.ScriptExecution{CallID: &identity, Mode: owner.invocation.Turn.Config.Mode, ReadOnly: readOnly, Turn: owner.state, Writes: owner.writes[identity]}
	result, err := owner.runTool(ctx, handler, input, execution)
	if owner.writes == nil {
		owner.writes = map[protocol.RequestID]uint64{}
	}
	owner.writes[identity] = execution.Writes
	if err == nil && execution.StagedTurn != nil {
		owner.state = slices.Clone(execution.StagedTurn)
	}
	return result, err
}

// The bundled native read adapters only convert arguments. Each can use its
// own script execution without retaining state or locking the package turn.
func (owner *pluginScriptTurn) evaluateNativeRead(ctx context.Context, handler, tool, call string, input any, _ bool) (json.RawMessage, error) {
	identity := owner.callIdentity(tool, call)
	return owner.runTool(ctx, handler, input, &plugins.ScriptExecution{
		CallID: &identity, Mode: owner.invocation.Turn.Config.Mode, ReadOnly: true,
	})
}

func (owner *pluginScriptTurn) callIdentity(tool, call string) protocol.RequestID {
	identityTool := tool
	if !strings.HasPrefix(tool, "turn.") {
		identityTool = plugins.ToolAlias(owner.info.Summary.Name, tool)
	}
	return plugins.ToolCallIdentity(owner.invocation.Run.Turn, owner.scope.Package, identityTool, call)
}

func (owner *pluginScriptTurn) runTool(ctx context.Context, handler string, input any, execution *plugins.ScriptExecution) (json.RawMessage, error) {
	ctx = store.WithCapturedUser(store.WithUser(ctx, owner.invocation.User), owner.invocation.StorageUser)
	operation, cancel := context.WithCancel(ctx)
	defer cancel()
	scope := owner.scope
	operation = owner.node.plugins.memory.Context(operation, scope.Package, "tool", pluginMemoryUser(operation))
	result, err := plugins.RunTool(operation, owner.bundle, handler, input, plugins.ScriptEnvironment{
		Context: scope, Target: owner.node.ID(), Deadline: time.Now().Add(plugins.ScriptCallTimeout),
	}, owner.node.dispatchCallbackResult, execution)
	if err == nil {
		result = plugins.StampCards(result, owner.node.ID(), scope, owner.info, *execution.CallID)
	}
	return result, err
}

type pluginToolBinding struct {
	owner protocol.PluginReference
	alias string
}

func (node *Node) scriptTools(ctx context.Context, invocation *store.AgentInvocation, info protocol.PluginInfo, native map[pluginToolBinding]engine.Tool) ([]engine.Tool, error) {
	host := info.Extension.Host
	packageRef := plugins.PackageReference(info)
	legacy := legacyNativeToolPackage(packageRef.Name, packageRef.Digest)
	boundTool := func(name string, operation protocol.ToolOperation) (engine.Tool, bool) {
		alias := plugins.ToolAlias(packageRef.Name, name)
		candidate, ok := native[pluginToolBinding{owner: packageRef, alias: alias}]
		return candidate, ok && candidate.Binding != nil && candidate.Binding.Owner == packageRef && candidate.Binding.Tool == alias &&
			candidate.Binding.Operation != nil && *candidate.Binding.Operation == operation
	}
	declarations := []protocol.ToolDeclaration{}
	names := []string{}
	// A directly bound resource still uses the captured package's scoped host tools.
	coreCommands := invocation.Turn.Config.Resource == nil || invocation.Turn.Config.Assistant != nil
	projected, hiddenInstructionAlias := plugins.ProjectScriptTools(packageRef, info.Extension.Tools, func(operation protocol.ToolOperation) bool {
		return node.codeIndex == nil && codeIndexOperation(operation)
	})
	for _, declaration := range projected {
		alias := plugins.ToolAlias(packageRef.Name, declaration.Name)
		if alias == operations.LegacyUpdatePlanTool || legacy && coreCommands && operations.LegacyCommandTool(alias) != "" {
			continue
		}
		if availablePluginTool(invocation, declaration) {
			if operation := declaration.Handler.Operation; operation != nil && (*operation == protocol.ToolOperationDelegateAgent || *operation == protocol.ToolOperationGuideAgent) &&
				!store.DelegationAvailable(invocation.Session, invocation.Turn) {
				continue
			}
			// The media tools are the Node's own: a package's declaration of a
			// media operation binds nothing. A managed browser tool exists
			// only when this Node can run that browser, and a browser tool
			// only when the turn's controller has a browser. Command mutations
			// likewise need a package-owned approval and execution binding.
			if operation := declaration.Handler.Operation; operation != nil {
				if _, media := mediaOperationKind(*operation); media {
					continue
				}
				switch *operation {
				case protocol.ToolOperationReadExternalBrowser, protocol.ToolOperationControlExternalBrowser, browserReadOperation, browserControlOperation,
					protocol.ToolOperationRunCommand, protocol.ToolOperationStopCommand, protocol.ToolOperationWriteCommand:
					if _, bound := boundTool(declaration.Name, *operation); !bound {
						continue
					}
				}
			}
			declarations = append(declarations, declaration)
			names = append(names, declaration.Name)
		}
	}
	if len(declarations) == 0 {
		return nil, nil
	}
	files, err := node.plugins.manager.ReadBundle(ctx, packageRef, host.Entry, host.Resources)
	if err != nil {
		return nil, err
	}
	owner := &pluginScriptTurn{node: node, invocation: invocation, info: info, bundle: plugins.ScriptBundle{Entry: host.Entry, Files: files},
		scope: protocol.PluginContext{Turn: &invocation.Run.Turn, Package: packageRef, Worktree: &invocation.Run.Worktree,
			Session: &invocation.Session.ID, Surface: protocol.PluginDesktopSurface{Kind: "workspace"}}, state: json.RawMessage("null")}
	instruction := ""
	if info.Extension.Instructions != nil {
		instruction = *info.Extension.Instructions
		if hiddenInstructionAlias != "" {
			invocation.Instructions = slices.DeleteFunc(slices.Clone(invocation.Instructions), func(captured store.AgentInstruction) bool {
				return captured.Text == instruction && slices.Contains(captured.AnyTools, hiddenInstructionAlias)
			})
			instruction = ""
		}
	}
	overrides := map[string]json.RawMessage{}
	var turnInstruction prompt.Part
	var contextParts []string
	if host.Turn != nil {
		selected := pluginToolOperations(declarations)
		input, err := pluginTurnInput(invocation, info, names, selected)
		if err != nil {
			return nil, err
		}
		connections, err := node.connectionTurnInput(ctx, invocation, selected)
		if err != nil {
			return nil, err
		}
		input["connections"] = connections
		body, err := owner.evaluate(ctx, host.Turn.Initialize, "turn.initialize", host.Turn.Initialize, input, false)
		if err != nil {
			return nil, err
		}
		var selection struct {
			Instruction string                     `json:"instruction"`
			Parts       []string                   `json:"parts"`
			Tools       []string                   `json:"tools"`
			Parameters  map[string]json.RawMessage `json:"parameters"`
		}
		decoder := json.NewDecoder(bytes.NewReader(body))
		decoder.DisallowUnknownFields()
		if decoder.Decode(&selection) != nil || selection.Tools == nil {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid plugin turn selection"}
		}
		for _, name := range selection.Tools {
			if !slices.Contains(names, name) {
				return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin turn selected an unavailable tool"}
			}
		}
		for name, schema := range selection.Parameters {
			var document map[string]any
			if !slices.Contains(selection.Tools, name) || len(schema) > plugins.ScriptMaxDataBytes || json.Unmarshal(schema, &document) != nil || document["type"] != "object" {
				return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin turn selected an invalid tool schema"}
			}
		}
		combined := 0
		if len(selection.Parts) > 8 {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin turn returned too many context parts"}
		}
		turnInstruction = prompt.Part{Key: "plugin:" + info.Summary.Name, Text: strings.TrimSpace(selection.Instruction)}
		for _, part := range selection.Parts {
			combined += len(part)
			if strings.TrimSpace(part) == "" || combined > 32*1024 {
				return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin turn returned invalid context parts"}
			}
			if !slices.Contains(contextParts, part) {
				contextParts = append(contextParts, part)
			}
		}
		declarations = slices.DeleteFunc(declarations, func(declaration protocol.ToolDeclaration) bool {
			return !slices.Contains(selection.Tools, declaration.Name)
		})
		overrides = selection.Parameters
	}
	var tools []engine.Tool
	for _, declaration := range declarations {
		handler := *declaration.Handler
		if schema, exists := overrides[declaration.Name]; exists {
			handler.Parameters = schema
		}
		var schema map[string]any
		if json.Unmarshal(handler.Parameters, &schema) != nil || schema["type"] != "object" {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin tool parameters are invalid"}
		}
		description := ""
		if declaration.Description != nil {
			description = *declaration.Description
		}
		readOnly := pluginHandlerReadOnly(handler, info.Extension.Actions)
		tool := engine.Tool{Definition: fantasy.FunctionTool{Name: plugins.ToolAlias(info.Summary.Name, declaration.Name), Description: description, InputSchema: schema},
			SchemaJSON: handler.Parameters, ReadOnly: readOnly, Deferred: !declaration.Initial, Approval: !readOnly,
			Binding:     &store.ApprovalBinding{Owner: owner.scope.Package, Tool: plugins.ToolAlias(info.Summary.Name, declaration.Name), Operation: handler.Operation},
			Instruction: strings.TrimSpace(instruction)}
		if handler.Operation == nil && handler.Flow == nil {
			// A plain handler acts only through its package's own grants, each
			// checked when the script dispatches, so the call itself asks for
			// nothing unless the package declares that it needs the user's
			// consent. A planning turn still refuses one that is not read-only.
			tool.Approval = handler.Approval && !readOnly
		} else if pluginStorageHandler(handler) {
			// Writes confined to the package's own values are authorized by
			// the storage scope.
			tool.ConfigureApproval = func(ctx context.Context, db *sql.Conn, _ canonical.ToolRequest, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
				return store.ConfigureWorkspaceApproval(ctx, db, invocation.Run.Turn, value, protocol.ToolOperationStorageTransaction, protocol.Command{}, nil)
			}
		}
		if handler.Flow != nil {
			tool.Flow = handler.Flow.Operations
			// A flow that declares a step it can never run is refused before
			// the user is asked to approve it.
			if index := slices.IndexFunc(tool.Flow, func(operation protocol.ToolOperation) bool { return !plugins.FlowOperation(operation) }); index >= 0 {
				tool.Prepare = func(context.Context, canonical.ToolRequest) error { return pluginFlowLimit(tool.Flow[index]) }
			}
		}
		if handler.Operation != nil {
			_, tool.Concurrent = operations.Policy(*handler.Operation)
		} else if handler.Flow != nil && len(handler.Flow.Operations) != 0 {
			tool.Concurrent = true
			for _, operation := range handler.Flow.Operations {
				_, concurrent := operations.Policy(operation)
				tool.Concurrent = tool.Concurrent && concurrent
			}
		}
		tool.TurnInstruction, tool.ContextParts = turnInstruction, contextParts
		if host.Turn != nil && host.Turn.BeforeModel != nil {
			tool.BeforeModel = owner.beforeModel(*host.Turn.BeforeModel)
		}
		if execute := node.coreGoalExecutor(invocation, info, declaration.Name); execute != nil {
			tool.Execute = execute
			tool.Concurrent = tool.ReadOnly
			tools = append(tools, tool)
			continue
		}
		if handler.Operation != nil {
			// A package tool bound to agents.delegate or agents.guide runs
			// the core tool with the arguments its script prepares.
			if *handler.Operation == protocol.ToolOperationDelegateAgent || *handler.Operation == protocol.ToolOperationGuideAgent {
				tool.Guidance = *handler.Operation == protocol.ToolOperationGuideAgent
				tool.BindDelegation = func(bound engine.Tool) engine.Tool { return owner.bindNative(tool, bound, declaration, handler) }
				tools = append(tools, tool)
				continue
			}
			if bound, exists := boundTool(declaration.Name, *handler.Operation); exists {
				tools = append(tools, owner.bindNative(tool, bound, declaration, handler))
				continue
			}
		}
		tool.Execute = func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
			var input any
			if json.Unmarshal(call.Arguments, &input) != nil {
				return workspaceToolFault(&protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid plugin tool arguments"})
			}
			if handler.Flow != nil {
				return owner.flow(ctx, declaration.Name, handler, call, input, readOnly)
			}
			body, err := owner.evaluate(ctx, handler.Name, declaration.Name, call.ID, input, readOnly)
			if err != nil {
				return workspaceToolFault(err)
			}
			var prepared any
			if json.Unmarshal(body, &prepared) != nil {
				return workspaceToolFault(&protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin tool returned invalid JSON"})
			}
			if pluginResultFailed(prepared) || handler.Operation == nil {
				return engine.ToolResult{Value: prepared, ReturnedError: pluginResultFailed(prepared)}
			}
			output := owner.operation(ctx, declaration.Name, call, 0, *handler.Operation, body)
			if handler.Result == nil || output.Failure != nil {
				return output
			}
			formatted, err := owner.evaluate(ctx, *handler.Result, declaration.Name, call.ID, map[string]any{"arguments": input, "output": output.Value}, readOnly)
			if err != nil {
				return output
			}
			if !output.ReturnedError {
				_ = json.Unmarshal(formatted, &output.Value)
			}
			return output
		}
		tools = append(tools, tool)
	}
	return tools, nil
}

func pluginToolOperations(declarations []protocol.ToolDeclaration) []protocol.ToolOperation {
	var selected []protocol.ToolOperation
	for _, declaration := range declarations {
		if declaration.Operation != nil {
			selected = append(selected, *declaration.Operation)
		}
		if handler := declaration.Handler; handler != nil {
			if handler.Operation != nil {
				selected = append(selected, *handler.Operation)
			}
			if handler.Flow != nil {
				selected = append(selected, handler.Flow.Operations...)
			}
		}
	}
	return selected
}

const (
	pluginTurnRoleSummaryChars = 1024
	pluginTurnMessageBytes     = 4096
)

// pluginTurnInput is the turn.initialize input. Every field comes from the
// frozen invocation, never from the controller's live selection.
func pluginTurnInput(invocation *store.AgentInvocation, info protocol.PluginInfo, names []string, selected []protocol.ToolOperation) (map[string]any, error) {
	roster := invocation.Turn.Roles
	// A notice turn and a continuation are both turns no user started.
	automatic := invocation.Run.Notice || invocation.Run.Continuation != nil
	options, err := plugins.CapturedOptions(info, invocation.Turn.Config)
	if err != nil {
		return nil, err
	}
	roles := []any{}
	if slices.Contains(selected, protocol.ToolOperationDelegateAgent) {
		for _, profile := range roster.Profiles {
			purpose := strings.TrimSpace(profile.Description)
			if purpose == "" {
				purpose = strings.TrimSpace(profile.Instructions)
			}
			if characters := []rune(purpose); len(characters) > pluginTurnRoleSummaryChars {
				purpose = string(characters[:pluginTurnRoleSummaryChars])
			}
			roles = append(roles, map[string]any{"key": profile.Key, "name": profile.Name, "description": purpose,
				"read_only": profile.ReadOnly, "tools": pluginRoleTools(roster, profile)})
		}
	}
	input := map[string]any{"tools": names, "project": invocation.Worktree.Project,
		"resources": true,
		"roles":     roles, "options": options, "session_revision": nil, "automatic": automatic, "notice": invocation.Run.Notice,
		"mode": invocation.Turn.Config.Mode, "config": nil, "execution": nil, "team": nil, "models": nil, "message": nil,
		"connections": map[string]any{"bound": invocation.Turn.Config.Resource, "databases": []any{}, "ssh": []any{}}}
	aliases := []string{}
	for _, name := range names {
		aliases = append(aliases, plugins.ToolAlias(info.Summary.Name, name))
	}
	input["names"] = aliases
	// A package sees only its own continuation and the marker it chose.
	if continuation := invocation.Run.Continuation; continuation != nil && continuation.Package.Name == info.Summary.Name {
		input["continuation"] = map[string]any{"marker": continuation.Kind}
	}
	if slices.Contains(selected, protocol.ToolOperationApplyRoles) {
		input["session_revision"] = strconv.FormatUint(invocation.Turn.Revision, 10)
	}
	if slices.Contains(info.Extension.Actions, protocol.PluginActionReadConversation) {
		models := roster.Models
		if models == nil {
			models = map[protocol.RoleID]protocol.RoleModel{}
		}
		input["config"], input["execution"], input["models"] = invocation.Turn.Config, roster.Execution, models
		if roster.Team != nil {
			input["team"] = map[string]any{"id": roster.Team.ID, "revision": strconv.FormatUint(roster.Team.Revision, 10)}
		}
	}
	// Only a person's own text: package-authored, notice and delegated input stay private.
	if slices.Contains(info.Extension.Actions, protocol.PluginActionReadMessage) && !automatic && invocation.Session.Delegation == nil {
		message := invocation.Message.Text
		if strings.TrimSpace(message) != "" {
			end := min(len(message), pluginTurnMessageBytes)
			for end > 0 && end < len(message) && !utf8.RuneStart(message[end]) {
				end--
			}
			input["message"] = message[:end]
		}
	}
	return input, nil
}

// pluginRoleTools reports whether the role's captured model can call tools. A
// role without its own model follows the lead's.
func pluginRoleTools(roster protocol.RoleSnapshot, profile protocol.RoleProfile) bool {
	model := profile.Model
	if override, exists := roster.Models[profile.ID]; exists {
		model = &override
	}
	if model == nil {
		return true
	}
	for _, provider := range roster.Providers {
		if provider.ID != model.Provider {
			continue
		}
		for _, entry := range provider.Models {
			if entry.ID == model.Model {
				return entry.Tools
			}
		}
		return false
	}
	return false
}

func pluginResultFailed(value any) bool {
	object, ok := value.(map[string]any)
	if !ok {
		return false
	}
	failed, _ := object["isError"].(bool)
	return failed
}

func (owner *pluginScriptTurn) beforeModel(handler string) func(context.Context, []canonical.Record, engine.ToolResultReader) (bool, error) {
	names := make(map[string]string, len(owner.info.Extension.Tools))
	for _, declaration := range owner.info.Extension.Tools {
		names[plugins.ToolAlias(owner.info.Summary.Name, declaration.Name)] = declaration.Name
	}
	lastCall := ""
	return func(ctx context.Context, records []canonical.Record, read engine.ToolResultReader) (bool, error) {
		results, newest, err := beforeModelResults(records, owner.invocation.Run.Turn, names, lastCall, read)
		if err != nil {
			return false, err
		}
		if newest == lastCall && newest != "" {
			return false, nil
		}
		body, err := owner.evaluate(ctx, handler, "turn.before_model", newest, map[string]any{"results": results}, false)
		if err != nil {
			return false, err
		}
		lastCall = newest
		var response struct {
			Stop bool `json:"stop"`
		}
		_ = json.Unmarshal(body, &response)
		return response.Stop, nil
	}
}

func beforeModelResults(records []canonical.Record, turn protocol.TurnID, names map[string]string, lastCall string, read engine.ToolResultReader) ([]json.RawMessage, string, error) {
	// Reserve the array envelope. Keep whole recent results, since even a
	// valid partial result can give a package the wrong view of its state.
	remaining := plugins.ScriptMaxDataBytes - len(`{"results":[]}`)
	results := []json.RawMessage{}
	newest := ""
	for i := len(records) - 1; i >= 0; i-- {
		record := records[i]
		if record.Turn != turn {
			continue
		}
		if record.Metadata.Input != nil && record.Provenance.Author == "user" {
			break
		}
		message := record.Message()
		if message == nil {
			continue
		}
		for j := len(message.Content) - 1; j >= 0; j-- {
			value, ok := message.Content[j].Data.(*canonical.ToolResponse)
			if !ok {
				continue
			}
			if newest == "" {
				newest = value.ID
				if newest != "" && newest == lastCall {
					return nil, newest, nil
				}
			}
			name, ok := names[value.Name]
			if !ok || remaining == 0 {
				continue
			}
			result, err := read(value)
			if err != nil {
				return nil, "", err
			}
			if len(result) > remaining {
				continue
			}
			body, err := json.Marshal(struct {
				Name     string          `json:"name"`
				Response json.RawMessage `json:"response"`
			}{name, result})
			if err == nil && len(body)+1 <= remaining {
				results = append(results, body)
				remaining -= len(body) + 1
			}
		}
	}
	slices.Reverse(results)
	return results, newest, nil
}
