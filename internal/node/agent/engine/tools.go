package engine

import (
	"container/list"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"runtime"
	"slices"
	"strconv"
	"strings"
	"sync"
	"time"

	"charm.land/fantasy"
	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/memory"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/agent/prompt"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/santhosh-tekuri/jsonschema/v6"
	"github.com/santhosh-tekuri/jsonschema/v6/kind"
	"lukechampine.com/blake3"
)

// Tool is one captured declaration and executor. Concurrency and authority are
// properties of the captured registration, never arguments supplied by a model.
type Tool struct {
	Definition fantasy.FunctionTool
	SchemaJSON json.RawMessage
	ReadOnly   bool
	Concurrent bool
	// StreamRead permits a native read to start before its model response is committed.
	StreamRead bool
	// FilePaths identifies the exact files a native file tool reads or changes.
	FilePaths  func(canonical.ToolRequest) []string
	Deferred   bool
	Delegation bool
	Approval   bool
	// Confirm reports whether one prepared call of a tool without Approval
	// asks first: a read-only tool with an optional project write.
	Confirm func(canonical.ToolRequest) bool
	Binding *store.ApprovalBinding
	Execute func(context.Context, canonical.ToolRequest) ToolResult
	Prepare func(context.Context, canonical.ToolRequest) error
	// ReleasePrepared drops transient preparation even when the call never
	// reaches Execute. It is safe to call after Execute has consumed it.
	ReleasePrepared   func(canonical.ToolRequest)
	ObserveHistory    func([]canonical.Record)
	ConfigureApproval func(context.Context, *sql.Conn, canonical.ToolRequest, protocol.ConversationApproval) (protocol.ConversationApproval, error)
	Approved          func(canonical.ToolRequest, protocol.ApprovalID, protocol.RequestID)
	Close             func() error
	Instruction       string
	TurnInstruction   prompt.Part
	ContextParts      []string
	BeforeModel       func(context.Context, []canonical.Record, ToolResultReader) (bool, error)
	BindDelegation    func(Tool) Tool
	// Guidance makes BindDelegation receive guide_agent instead of spawn_agent.
	Guidance bool
	// Flow lists the operations a package flow may run.
	Flow []protocol.ToolOperation
	// Exposed is the name an MCP tool is offered under when no other tool
	// owns it. A package tool is offered under its declared name.
	Exposed      string
	MCPAuthority mcp.AuthorityKey
	// Display is how the turn's records show a call of a tool that no package
	// declaration describes, such as one an MCP server lists.
	Display *protocol.ToolDisplay
	// SearchLabels are the captured display labels and translations used only
	// for discovery. They do not enlarge the model's tool declarations.
	SearchLabels []string
	// LegacyPackage, LegacyServer and LegacyName identify a former bundled
	// plugin tool (or plugin MCP tool when LegacyServer is non-empty) that this
	// native module tool supersedes. History, tool_search, skills and assistant
	// selectors referencing the legacy alias resolve to this tool.
	LegacyPackage string
	LegacyServer  string
	LegacyName    string
	schema        *jsonschema.Schema
	parentSchema  *jsonschema.Schema
	// ExecutePrepared runs a call with arguments a package prepared. The call
	// keeps the identity the model sent; the arguments are what executes.
	ExecutePrepared func(context.Context, canonical.ToolRequest, json.RawMessage) ToolResult
}

// FunctionTool constructs a fantasy.FunctionTool for callers outside engine
// without importing the model SDK directly.
func FunctionTool(name, description string, inputSchema map[string]any) fantasy.FunctionTool {
	return fantasy.FunctionTool{Name: name, Description: description, InputSchema: inputSchema}
}

// LegacyAlias returns the former plugin-scoped (or MCP-scoped) canonical alias
// superseded by this tool, or an empty string if none applies.
func (t Tool) LegacyAlias() string {
	if !nativeTool(t) || t.LegacyPackage == "" {
		return ""
	}
	name := t.LegacyName
	if name == "" {
		name = t.Definition.Name
	}
	if t.LegacyServer != "" {
		return mcp.Alias(t.LegacyPackage, t.LegacyServer, name)
	}
	return operations.Alias(t.LegacyPackage, name)
}

// ToolResultReader materializes a selected result for a package hook. The
// canonical history passed to the hook retains its stored asset references.
type ToolResultReader func(*canonical.ToolResponse) (json.RawMessage, error)

type ToolResult struct {
	Collect         *collectedChild
	Meta            *map[string]json.RawMessage
	ReturnedFailure *protocol.ToolFailure
	ReturnedError   bool
	Value           any
	Format          canonical.ToolFormat
	Error           bool
	Failure         *protocol.Fault
	Context         *canonical.Context
	// Attachments are stored assets shown with the result and never sent to
	// the model.
	Attachments  []canonical.Attachment
	modelFailure model.FailureKind
}

// WithModelFailure retains the classification of a direct model request for
// tool diagnostics, without changing the tool response kept in history.
func (result ToolResult) WithModelFailure(failure *model.Failure) ToolResult {
	if failure != nil && !failure.RequestObserved() {
		result.modelFailure = failure.Kind
	}
	return result
}

func (r *Runtime) captureTools(x *execution) error {
	child := x.inv.Session.Delegation != nil && *x.inv.Session.Delegation != nil
	if child && x.parentTools == nil {
		return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "parent tool capture is unavailable"}
	}
	x.tools = append([]Tool(nil), r.tools...)
	if r.toolsForTurn != nil {
		scoped, err := r.toolsForTurn(x.ctx, x.inv)
		if err != nil {
			return err
		}
		x.tools = append(x.tools, scoped...)
	}
	builtins := r.builtins(x)
	x.tools = append(builtins, x.tools...)
	for index := range x.tools {
		if bind := x.tools[index].BindDelegation; bind != nil {
			name := "spawn_agent"
			if x.tools[index].Guidance {
				name = "guide_agent"
			}
			if delegate := findTool(builtins, name); delegate != nil {
				x.tools[index] = bind(*delegate)
			}
		}
	}
	x.tools = slices.DeleteFunc(x.tools, func(tool Tool) bool { return tool.BindDelegation != nil && !tool.Delegation })
	for _, tool := range x.tools {
		if tool.Close != nil {
			x.toolCleanup = append(x.toolCleanup, tool.Close)
		}
	}
	if child {
		x.tools = x.parentTools.restrict(x.tools)
		if x.childRole != nil && x.childRole.Tools != nil {
			legacy := legacyToolAliases(x.tools)
			if len(missingRoleTools(*x.childRole, x.inv.Turn.Config.Mode, modelSupportsTools(x.inv.Provider, x.inv.Turn.Config.Model), x.tools, legacy)) != 0 {
				return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "role references an unavailable tool"}
			}
			x.tools = slices.DeleteFunc(x.tools, func(tool Tool) bool {
				return tool.Definition.Name != "read_tool_output" && !slices.ContainsFunc(*x.childRole.Tools, func(selection protocol.PluginConversationTool) bool {
					return roleToolMatches(selection, tool, legacy)
				})
			})
		} else {
			// Default children are bounded workers; omit update_plan so they act
			// directly instead of spending turns maintaining a separate plan.
			// Roles that want plan tracking can still opt in via role.Tools.
			x.tools = slices.DeleteFunc(x.tools, func(tool Tool) bool {
				return tool.Definition.Name == operations.UpdatePlanTool
			})
		}
	}
	if err := r.deferTools(x); err != nil {
		return err
	}
	if err := r.applyAssistant(x); err != nil {
		return err
	}
	if search := r.builtinSearch(x); search != nil {
		x.tools = append(x.tools, *search, callTool())
	}
	for i := range x.tools {
		tool := &x.tools[i]
		schema, err := compileToolSchema(tool.Definition.Name, tool.SchemaJSON, tool.Definition.InputSchema)
		if err != nil {
			return err
		}
		tool.schema = schema
	}
	if delegationAvailable(x) {
		x.delegationTools = captureInheritedTools(x.tools)
		if spawn := findTool(x.tools, "spawn_agent"); spawn != nil {
			spawn.Definition.Description = spawnDescriptionFor(x, r.usableRoles(x))
		}
	}
	return nil
}

const maxCompiledToolSchemas = 512

type compiledToolSchemaEntry struct {
	key    [32]byte
	schema *jsonschema.Schema
}

var compiledToolSchemas = struct {
	mu      sync.Mutex
	order   list.List
	entries map[[32]byte]*list.Element
}{
	entries: map[[32]byte]*list.Element{},
}

func init() {
	memory.Default().Register("engine.tool_schemas", func(level memory.Level) {
		if level != memory.Pressure {
			return
		}
		compiledToolSchemas.mu.Lock()
		clear(compiledToolSchemas.entries)
		compiledToolSchemas.order.Init()
		compiledToolSchemas.mu.Unlock()
	})
}

func compileToolSchema(name string, raw json.RawMessage, inputSchema any) (*jsonschema.Schema, error) {
	if len(raw) == 0 && inputSchema != nil {
		if encoded, err := json.Marshal(inputSchema); err == nil {
			raw = encoded
		}
	}
	var key [32]byte
	cacheable := len(raw) > 0
	if cacheable {
		key = blake3.Sum256(raw)
		compiledToolSchemas.mu.Lock()
		if elem, ok := compiledToolSchemas.entries[key]; ok {
			compiledToolSchemas.order.MoveToFront(elem)
			schema := elem.Value.(compiledToolSchemaEntry).schema
			compiledToolSchemas.mu.Unlock()
			return schema, nil
		}
		compiledToolSchemas.mu.Unlock()
	}
	compiler := jsonschema.NewCompiler()
	compiler.UseLoader(jsonschema.SchemeURLLoader{})
	location := "urn:sailry:tool:" + name
	if err := compiler.AddResource(location, inputSchema); err != nil {
		return nil, err
	}
	schema, err := compiler.Compile(location)
	if err != nil {
		return nil, err
	}
	if cacheable {
		compiledToolSchemas.mu.Lock()
		if _, exists := compiledToolSchemas.entries[key]; !exists {
			if len(compiledToolSchemas.entries) >= maxCompiledToolSchemas {
				if back := compiledToolSchemas.order.Back(); back != nil {
					compiledToolSchemas.order.Remove(back)
					delete(compiledToolSchemas.entries, back.Value.(compiledToolSchemaEntry).key)
				}
			}
			elem := compiledToolSchemas.order.PushFront(compiledToolSchemaEntry{key: key, schema: schema})
			compiledToolSchemas.entries[key] = elem
		}
		compiledToolSchemas.mu.Unlock()
	}
	return schema, nil
}

func (r *Runtime) closeTools(x *execution) error {
	cleanup := x.toolCleanup
	x.toolCleanup = nil
	var failures []error
	panicked := false
	for _, close := range cleanup {
		failures = append(failures, func() (err error) {
			defer func() {
				if value := recover(); value != nil {
					panicked = true
					logPanic(x, "tool_cleanup", "", value)
					err = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "tool resource cleanup failed unexpectedly"}
				}
			}()
			return close()
		}())
	}
	if errors.Join(failures...) != nil {
		fault := &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "tool resource cleanup could not be confirmed"}
		if panicked {
			return &observedTurnPanic{fault}
		}
		return fault
	}
	return nil
}

// deferTools keeps a default child's initial and discoverable tools in the
// same places as the parent's. A role with an explicit list defers nothing.
func (r *Runtime) deferTools(x *execution) error {
	if x.parentTools != nil {
		explicit := x.childRole != nil && x.childRole.Tools != nil
		for index := range x.tools {
			tool := &x.tools[index]
			tool.Deferred = !explicit && x.parentTools[tool.Definition.Name].deferred
		}
		return nil
	}
	selected := ""
	if x.resources != nil {
		mode, err := x.resources.SelectedMode(x.inv.Turn.Config)
		if err != nil {
			return err
		}
		if mode != nil {
			selected = mode.Package.Name
		}
	}
	role := selectedRole(x.inv.Message) != nil
	for index := range x.tools {
		tool := &x.tools[index]
		switch {
		case tool.Delegation && role:
			tool.Deferred = false
		case tool.Binding == nil || tool.Binding.Owner.Name == "":
			if slices.Contains([]string{"list_sessions", "read_session", "search_session"}, tool.Definition.Name) {
				tool.Deferred = true
			} else if tool.Deferred && (visibleTool(*tool) || (selected != "" && tool.LegacyPackage == selected)) {
				tool.Deferred = false
			}
		default:
			tool.Deferred = !(tool.Binding.Owner.Name == selected || visibleTool(*tool) || !tool.Deferred)
		}
	}
	return nil
}

// declaredOperations lists the operations a package tool declares: its bound
// operation and the operations of its flow.
func declaredOperations(tool Tool) []protocol.ToolOperation {
	declared := tool.Flow
	if tool.Binding != nil && tool.Binding.Operation != nil {
		declared = append([]protocol.ToolOperation{*tool.Binding.Operation}, declared...)
	}
	return declared
}

// A native workflow chooses its initial visibility in its catalog. Its flow
// lists its capabilities; only direct operation bindings inherit a default.
func visibleTool(tool Tool) bool {
	if nativeTool(tool) {
		return tool.Binding != nil && tool.Binding.Operation != nil && visibleOperation(*tool.Binding.Operation)
	}
	declared := declaredOperations(tool)
	return len(declared) != 0 && !slices.ContainsFunc(declared, func(operation protocol.ToolOperation) bool { return !visibleOperation(operation) })
}

// visibleOperation names the operations offered on the first request.
// Common file operations, Git status/diff and commands are directly callable.
func visibleOperation(operation protocol.ToolOperation) bool {
	switch operation {
	case protocol.ToolOperationReadFile, protocol.ToolOperationReadFileLines, protocol.ToolOperationListDirectory, protocol.ToolOperationFindFiles, protocol.ToolOperationSearchFiles,
		protocol.ToolOperationWriteFile, protocol.ToolOperationEditFile, protocol.ToolOperationApplyChanges,
		protocol.ToolOperationInspectGit, protocol.ToolOperationReadGitDiff,
		protocol.ToolOperationRunCommand, protocol.ToolOperationReadCommand, protocol.ToolOperationStopCommand,
		protocol.ToolOperationProgress, protocol.ToolOperationListRoles, protocol.ToolOperationListArtifactRevisions:
		return true
	}
	return false
}

// deferredContext lists every captured deferred tool. Loading one does not
// change the list, so discovery appends no repeated catalog to the history.
func (r *Runtime) deferredContext(x *execution, state store.AgentState) string {
	names := r.exposure(x, state)
	var deferred []string
	for _, tool := range x.tools {
		if tool.Deferred {
			deferred = append(deferred, names.name(tool.Definition.Name))
		}
	}
	if len(deferred) == 0 {
		return ""
	}
	slices.Sort(deferred)
	return "Deferred tools\n" + strings.Join(deferred, ", ") + "\nUse tool_search to load a needed tool by exact name (select:name) or search by capability, then run it with call_tool."
}

// declarations are the same for every request of a turn and for later turns
// with the same captured configuration. Provider prompt caches key on the
// declared tools before the history, so loaded deferred tools are described
// in turn context and run through call_tool instead of being declared.
func (r *Runtime) declarations(x *execution, binding model.Binding, state store.AgentState) []fantasy.FunctionTool {
	var values []fantasy.FunctionTool
	if !capturedModel(binding).Tools {
		return values
	}
	// The request renames these canonical declarations at the model boundary.
	r.exposure(x, state)
	for _, tool := range x.tools {
		if !tool.Deferred {
			values = append(values, tool.Definition)
		}
	}
	return values
}

func toolLoaded(loaded []string, tool Tool, names *exposure) bool {
	return slices.ContainsFunc(loaded, func(name string) bool {
		return names.alias(name) == tool.Definition.Name
	})
}

func findTool(tools []Tool, name string) *Tool {
	for i := range tools {
		if tools[i].Definition.Name == name {
			return &tools[i]
		}
	}
	if name == operations.LegacyUpdatePlanTool {
		return findTool(tools, operations.UpdatePlanTool)
	}
	return nil
}

// File mutations prepare against the state left by earlier calls. Other
// approvals retain the command captured in the initial batch preflight.
func (tool *Tool) prepareAtAdmission() bool {
	return tool.Approval && (tool.FilePaths != nil || tool.ObserveHistory != nil ||
		tool.Binding != nil && tool.Binding.Operation != nil && *tool.Binding.Operation == protocol.ToolOperationApplyChanges)
}

func nativeTool(tool Tool) bool {
	return tool.Binding == nil || tool.Binding.Owner.Name == ""
}

// Historical aliases only migrate absent package identities. A captured plugin
// that owns the old name must keep its executor and approval binding.
func legacyToolAliases(tools []Tool) map[string]string {
	registered := make(map[string]bool, len(tools))
	for _, tool := range tools {
		registered[tool.Definition.Name] = true
	}
	aliases := map[string]string{}
	claim := func(alias, name string) {
		if alias == "" || registered[alias] {
			return
		}
		if previous, exists := aliases[alias]; exists && previous != name {
			aliases[alias] = ""
			return
		}
		aliases[alias] = name
	}
	for _, tool := range tools {
		if !nativeTool(tool) {
			continue
		}
		name := tool.Definition.Name
		claim(tool.LegacyAlias(), name)
		if tool.LegacyServer != "" {
			legacy := tool.LegacyName
			if legacy == "" {
				legacy = name
			}
			claim(MCPToolName(tool.LegacyServer, legacy), name)
		}
		if name == operations.UpdatePlanTool {
			claim(operations.LegacyUpdatePlanTool, name)
		}
	}
	for alias, name := range aliases {
		if name == "" {
			delete(aliases, alias)
		}
	}
	return aliases
}

func (r *Runtime) runTools(x *execution, state store.AgentState, calls []canonical.ToolRequest) error {
	var step context.Context
	if reads := x.streamReads; reads != nil {
		step = reads.ctx
		defer func() {
			reads.close()
			x.streamReads = nil
		}()
	} else {
		var end func()
		step, end = r.beginToolStep(x)
		defer end()
	}
	// Discovery answers with the names this turn offers its tools under.
	r.exposure(x, state)
	// Commit leading plan receipts before the work they introduce can block
	// on execution or approval. Do not move a later update ahead of that work.
	firstWork := slices.IndexFunc(calls, func(call canonical.ToolRequest) bool { return !operations.IsUpdatePlanTool(call.Name) })
	if firstWork > 0 {
		if err := r.runToolBatch(x, step, state, calls[:firstWork]); err != nil {
			return err
		}
		calls = calls[firstWork:]
	}
	return r.runToolBatch(x, step, state, calls)
}

func (r *Runtime) runToolBatch(x *execution, step context.Context, state store.AgentState, calls []canonical.ToolRequest) error {
	results := make([]ToolResult, len(calls))
	executable := make([]bool, len(calls))
	startedCalls := make([]bool, len(calls))
	asks := make([]bool, len(calls))
	var historyLimit int
	var marks []canonical.Record
	x.observedMu.Lock()
	if x.observed.Tools == nil {
		x.observed.Tools = map[string]int{}
	}
	for _, call := range calls {
		x.observed.Tools[call.Name]++
	}
	x.observedMu.Unlock()
	// Mark every allowed call before requesting the first approval.
	planning := x.inv.Turn.Config.Mode == protocol.WorkModePlan
	child := x.inv.Session.Delegation != nil
	readOnlyRole := child && x.childRole != nil && x.childRole.ReadOnly
	readOnlyChild := child && (planning || readOnlyRole)
	readOnlyMessage := "planning mode permits read-only tools"
	if readOnlyRole {
		readOnlyMessage = "read-only role permits read-only tools"
	}
	// A concurrent read the reply already asked for, with the same
	// arguments, in the same run of concurrent reads, would run at the same
	// moment and read the same state, so it is answered by reference instead.
	// Any other call ends the run; effects may be meant twice and run.
	firsts := map[string]string{}
	concurrentRead := func(tool *Tool, call canonical.ToolRequest) bool {
		return tool != nil && tool.ReadOnly && tool.Concurrent && !x.hooks.WrapsTool(call.Name)
	}
	for i, call := range calls {
		if x.streamReads.read(call.ID) != nil {
			if key := repeatKey(call); key != "" && firsts[key] == "" {
				firsts[key] = call.ID
			}
			executable[i] = true
			marks = append(marks, r.executableRecord(x, call, true))
			continue
		}
		if step.Err() != nil {
			break
		}
		tool := findTool(x.tools, call.Name)
		if !concurrentRead(tool, call) {
			clear(firsts)
		}
		if call.Error != nil {
			results[i] = ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: call.Error.Message}}
			continue
		}
		// A captured deferred tool called by name runs under its own approval:
		// the saved request already loaded it, as in the Rust catalog.
		if tool == nil {
			// A name the turn does not bind is answered with how to find a
			// real one, so the model does not retry an invented name.
			text := fmt.Sprintf("This tool call was not handled: no tool named %q is available.", call.Name)
			if findTool(x.tools, "tool_search") != nil {
				text += " Use tool_search to find a tool by name or capability."
			}
			results[i] = ToolResult{Error: true, Value: map[string]any{"content": []any{map[string]any{"type": "text", "text": text}}, "isError": true}}
			continue
		}
		// A main planning turn can ask the user and write its own artifacts.
		planException := !child && (tool.Delegation || call.Name == "ask_user" ||
			tool.Binding != nil && tool.Binding.Operation != nil && *tool.Binding.Operation == protocol.ToolOperationWriteArtifact)
		if !tool.ReadOnly && (readOnlyChild || planning && !planException) {
			results[i] = ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: readOnlyMessage}}
			continue
		}
		if invalid := validateToolArguments(tool, call); invalid.Failure != nil {
			results[i] = invalid
			continue
		}
		key := repeatKey(call)
		if first := firsts[key]; key != "" && first != "" && concurrentRead(tool, call) {
			results[i] = repeatedCall(first)
			continue
		}
		if tool.prepareAtAdmission() {
			if tool.ObserveHistory != nil {
				historyLimit = i
			}
			if tool.Prepare != nil && tool.ReleasePrepared != nil {
				defer tool.ReleasePrepared(call)
			}
			asks[i] = true
			clear(firsts)
			continue
		}
		if tool.ObserveHistory != nil {
			tool.ObserveHistory(state.Records)
		}
		if tool.Prepare != nil {
			if tool.ReleasePrepared != nil {
				defer tool.ReleasePrepared(call)
			}
			if err := tool.Prepare(x.ctx, call); err != nil {
				results[i] = toolFault(err)
				continue
			}
		}
		confirm := tool.Confirm != nil && !tool.Approval && tool.Confirm(call)
		if readOnlyChild && confirm {
			results[i] = ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: readOnlyMessage}}
			continue
		}
		if asks[i] = tool.Approval || confirm; !asks[i] {
			executable[i] = true
			marks = append(marks, r.executableRecord(x, call, true))
			// Only a read that will run answers its repeats.
			if key != "" && concurrentRead(tool, call) && firsts[key] == "" {
				firsts[key] = call.ID
			}
		} else {
			clear(firsts)
		}
	}
	if err := r.save(x.ctx, x.inv.Session.ID, marks...); err != nil {
		return err
	}
	if x.streamReads != nil {
		for _, call := range calls {
			if read := x.streamReads.read(call.ID); read != nil {
				read.publishActivity(func() func() { return r.toolActivity(x, call) })
			}
		}
	}
	concurrent := func(i int) bool {
		tool := findTool(x.tools, calls[i].Name)
		return !asks[i] && (!executable[i] || tool != nil && tool.ReadOnly && tool.Concurrent && !x.hooks.WrapsTool(calls[i].Name))
	}
	toolContext := context.WithValue(x.ctx, foregroundToolKey{}, step)
	var slots chan struct{}
	if x.streamReads != nil && x.streamReads.slots != nil {
		slots = x.streamReads.slots
	} else {
		slots = make(chan struct{}, maxConcurrentTools)
	}
	// Earlier writes may finish out of order. Preparation sees completed
	// predecessors in the same order as their eventual canonical receipts.
	receipts := make([]canonical.Record, historyLimit)
	completed := make([]bool, historyLimit)
	recordCompletion := func(i int) {
		if i >= historyLimit {
			return
		}
		completed[i] = true
		if results[i].Value == nil && results[i].Failure == nil {
			return
		}
		rec := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, responseBlock(uint64(i), calls[i], results[i])), r.clock.Now())
		rec.ID = "batch:" + calls[i].ID
		rec.Provenance.Author = "tool"
		receipts[i] = rec
	}
	prefix := 0
	history := state.Records[:len(state.Records):len(state.Records)]
	historyBefore := func(i int) []canonical.Record {
		// An unadmitted call is incomplete, so the prefix cannot pass i
		// even when an independent later call was admitted first.
		for prefix < i && completed[prefix] {
			if receipts[prefix].ID != "" {
				history = append(history, receipts[prefix])
			}
			prefix++
		}
		visible := history[:len(history):len(history)]
		for j := prefix + 1; j < i; j++ {
			if receipts[j].ID != "" {
				visible = append(visible, receipts[j])
			}
		}
		return visible
	}
	admit := func(i int) error {
		if !asks[i] {
			return nil
		}
		call := calls[i]
		tool := findTool(x.tools, call.Name)
		if tool.prepareAtAdmission() {
			if tool.ObserveHistory != nil {
				tool.ObserveHistory(historyBefore(i))
			}
			if tool.Prepare != nil {
				if err := tool.Prepare(x.ctx, call); err != nil {
					results[i] = toolFault(err)
					return nil
				}
			}
		}
		approved, err := r.approval(x, step, call, tool)
		if err != nil {
			return err
		}
		executable[i] = approved
		if !approved {
			results[i] = ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "Tool call was denied"}}
		}
		return r.save(x.ctx, x.inv.Session.ID, r.executableRecord(x, call, approved))
	}
	execute := func(i int) {
		if read := x.streamReads.read(calls[i].ID); read != nil {
			<-read.done
			results[i], startedCalls[i] = read.result, read.ran
			return
		}
		if !executable[i] {
			return
		}
		select {
		case slots <- struct{}{}:
			defer func() { <-slots }()
		case <-step.Done():
			return
		}
		call := calls[i]
		metric, started := r.telemetry.Capture(), time.Now()
		defer func() { observeTool(metric, x.ctx, x.tools, call.Name, results[i], started) }()
		beforeRan := false
		defer func() {
			if beforeRan {
				x.hooks.After(x.ctx, call.Name, call.ID, results[i].Failure == nil && !results[i].Error && !results[i].ReturnedError)
			}
		}()
		defer containToolPanic(x, call, &results[i])
		if step.Err() != nil {
			return
		}
		tool := findTool(x.tools, call.Name)
		hookCtx := x.ctx
		if tool != nil && tool.ReadOnly && !asks[i] && !tool.Delegation {
			hookCtx = step
		}
		if err := x.hooks.Before(hookCtx, call.Name, call.ID); err != nil {
			if step.Err() != nil {
				return
			}
			results[i] = toolFault(err)
			return
		}
		if step.Err() != nil {
			return
		}
		beforeRan = true
		ctx := toolContext
		if tool.ReadOnly && !asks[i] && !tool.Delegation {
			ctx = step
		}
		defer r.toolActivity(x, call)()
		defer x.timeline.tool(call, tool.Delegation)()
		startedCalls[i] = true
		results[i] = tool.Execute(ctx, call)
	}
	access := make([]toolAccess, len(calls))
	for i, call := range calls {
		tool := findTool(x.tools, call.Name)
		if tool != nil && (executable[i] || asks[i]) && !x.hooks.WrapsTool(call.Name) {
			access[i] = parallelToolAccess(tool, call)
		}
	}
	var batchErr error
	for start := 0; start < len(calls) && step.Err() == nil; {
		end := start + 1
		reads, mixed := concurrent(start), access[start].parallel
		for end < len(calls) {
			reads = reads && concurrent(end)
			mixed = mixed && access[end].parallel
			if !reads && !mixed {
				break
			}
			end++
		}
		err := runQueuedTools(step, start, access[start:end], admit, execute, recordCompletion)
		if err != nil {
			if err != errQueuedSteer {
				batchErr = err
			}
			break
		}
		start = end
	}
	blocks := make([]canonical.Block, 0, len(calls))
	var contextDelta canonical.Context
	for i, call := range calls {
		if read := x.streamReads.read(call.ID); read != nil {
			<-read.done
			results[i], startedCalls[i] = read.result, read.ran
		}
		if startedCalls[i] && (results[i].Failure != nil || results[i].ReturnedFailure != nil) {
			recordToolFailure(step, findTool(x.tools, call.Name), results[i])
		}
		if (executable[i] || step.Err() != nil || batchErr != nil) && results[i].Value == nil && results[i].Failure == nil && (!results[i].Error || step.Err() != nil) {
			results[i] = interruptedTool(startedCalls[i])
			if !startedCalls[i] && Steered(step) {
				results[i].Failure.Message = "Tool call did not start before queued guidance redirected the turn"
			}
		}
	}
	doomAborted := false
	if step.Err() == nil && batchErr == nil && len(calls) > 0 {
		doomAborted = trackDoomLoop(x, calls, results)
	}
	for i, call := range calls {
		blocks = append(blocks, responseBlock(uint64(i), call, results[i]))
		if delta := results[i].Context; delta != nil {
			if delta.Title != nil {
				contextDelta.Title = delta.Title
			}
			if delta.Policy != nil {
				contextDelta.Policy = delta.Policy
			}
			if delta.LoadedTools != nil {
				if contextDelta.LoadedTools == nil {
					names := []string{}
					contextDelta.LoadedTools = &names
				}
				for _, name := range *delta.LoadedTools {
					if !slices.Contains(*contextDelta.LoadedTools, name) {
						*contextDelta.LoadedTools = append(*contextDelta.LoadedTools, name)
					}
				}
			}
		}
	}
	record := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, blocks...), r.clock.Now())
	record.Provenance.Author = "tool"
	for i, result := range results {
		if result.ReturnedError {
			if record.Metadata.Presentation == nil {
				record.Metadata.Presentation = &canonical.Presentation{}
			}
			record.Metadata.Presentation.Failures = append(record.Metadata.Presentation.Failures, canonical.ToolFailure{Index: uint64(i), ID: calls[i].ID, Name: calls[i].Name, Failure: returnedToolFailure(result)})
		}
	}
	if contextDelta.Title != nil || contextDelta.LoadedTools != nil || contextDelta.Policy != nil {
		record.Metadata.Context = &contextDelta
	}
	if err := r.save(context.WithoutCancel(x.ctx), x.inv.Session.ID, record); err != nil {
		return err
	}
	for _, result := range results {
		if result.Collect != nil {
			if err := r.consumeChild(x, result.Collect.session, &result.Collect.turn); err != nil {
				return err
			}
		}
	}
	if doomAborted {
		return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: fmt.Sprintf("turn stopped after %d consecutive identical failing tool calls", x.doomStreak)}
	}
	return batchErr
}

// Report only evidence from an executed native tool. Preparation and hook
// refusals, interrupted calls, and package or MCP results are not that evidence.
func recordToolFailure(step context.Context, tool *Tool, result ToolResult) {
	if step.Err() != nil || tool == nil || !nativeTool(*tool) || tool.MCPAuthority != (mcp.AuthorityKey{}) {
		return
	}
	var code protocol.ErrorCode
	var cause string
	if fault := result.Failure; fault != nil {
		if fault.ReportOwned && logging.ValidReportRef(fault.ReportRef) {
			return
		}
		switch fault.Code {
		case protocol.ErrorCodeInternal:
			code = fault.Code
		case protocol.ErrorCodeUnavailable:
			switch result.modelFailure {
			case model.Network, model.Unavailable, model.InvalidResponse:
				code, cause = fault.Code, "model_"+string(result.modelFailure)
			}
		}
	} else if failure := result.ReturnedFailure; result.ReturnedError && failure != nil && failure.Origin == protocol.ToolFailureOriginRuntime &&
		failure.Code != nil && *failure.Code == protocol.ErrorCodeOutcomeUnknown && tool.Binding != nil && tool.Binding.Operation != nil &&
		*tool.Binding.Operation == protocol.ToolOperationRunCommand {
		// run_command returns this outcome only for a foreground completion.
		// read_command and stop_command can replay a background snapshot.
		code, cause = *failure.Code, "foreground_command"
	}
	if code != "" {
		logging.Emit(slog.LevelError, slog.String("area", "agent"), slog.String("operation", "tool_execution"),
			slog.String("phase", logging.Failed), slog.String("code", string(code)), slog.String("cause_code", cause))
	}
}

const (
	doomLoopWarnAt  = 3
	doomLoopAbortAt = 5
)

func toolResultFailed(result ToolResult) bool {
	return result.Failure != nil || result.Error || result.ReturnedError
}

func batchFailureSignature(calls []canonical.ToolRequest, results []ToolResult) (string, bool) {
	if len(calls) == 0 {
		return "", false
	}
	var b strings.Builder
	for i, call := range calls {
		if !toolResultFailed(results[i]) {
			return "", false
		}
		if results[i].Failure != nil && results[i].Failure.Code == protocol.ErrorCodeCancelled {
			return "", false
		}
		b.WriteString(call.Name)
		b.WriteByte(':')
		var normalized any
		if json.Unmarshal(call.Arguments, &normalized) == nil {
			if raw, err := json.Marshal(normalized); err == nil {
				b.Write(raw)
			} else {
				b.Write(call.Arguments)
			}
		} else {
			b.Write(call.Arguments)
		}
		b.WriteByte('\n')
	}
	return b.String(), true
}

// trackDoomLoop detects repeated consecutive identical failing tool batches.
// At doomLoopWarnAt it appends recovery guidance to the last failing result;
// at doomLoopAbortAt it returns true so the caller aborts after persisting the
// paired tool response record.
func trackDoomLoop(x *execution, calls []canonical.ToolRequest, results []ToolResult) bool {
	sig, allFailed := batchFailureSignature(calls, results)
	if !allFailed {
		x.doomSignature = ""
		x.doomStreak = 0
		return false
	}
	if sig == x.doomSignature {
		x.doomStreak++
	} else {
		x.doomSignature = sig
		x.doomStreak = 1
	}
	if x.doomStreak >= doomLoopAbortAt {
		return true
	}
	if x.doomStreak >= doomLoopWarnAt {
		last := &results[len(results)-1]
		note := fmt.Sprintf(" [Repeated identical failure %d times: stop retrying the same call and arguments; inspect the error, verify state, or change approach.]", x.doomStreak)
		if last.Failure != nil {
			copied := *last.Failure
			copied.Message += note
			last.Failure = &copied
		} else if mcpRes, ok := last.Value.(*sdk.CallToolResult); ok && mcpRes != nil {
			cloned := *mcpRes
			cloned.Content = append(slices.Clone(mcpRes.Content), &sdk.TextContent{Text: strings.TrimSpace(note)})
			last.Value = &cloned
		} else if m, ok := last.Value.(map[string]any); ok {
			cloned := make(map[string]any, len(m)+1)
			for k, v := range m {
				cloned[k] = v
			}
			if errObj, ok := cloned["error"].(map[string]any); ok {
				errCopy := make(map[string]any, len(errObj))
				for k, v := range errObj {
					errCopy[k] = v
				}
				if msg, ok := errCopy["message"].(string); ok {
					errCopy["message"] = msg + note
				} else {
					errCopy["message"] = strings.TrimSpace(note)
				}
				cloned["error"] = errCopy
			}
			cloned["doom_loop_warning"] = strings.TrimSpace(note)
			last.Value = cloned
		} else {
			last.Value = map[string]any{
				"error":             map[string]any{"message": strings.TrimSpace(note)},
				"result":            last.Value,
				"doom_loop_warning": strings.TrimSpace(note),
			}
		}
	}
	return false
}

// Each worker owns a provisional activity independently of the ordered batch
// result. Finishing one call clears its indicator while its siblings still run.
func (r *Runtime) toolActivity(x *execution, call canonical.ToolRequest) func() {
	draftID := protocol.NewRequestID().String()
	data, _ := json.Marshal(struct {
		Type string `json:"type"`
		ID   string `json:"id"`
		Name string `json:"name"`
	}{"tool_started", call.ID, call.Name})
	draft := protocol.ConversationDraft{ID: draftID, Turn: x.inv.Run.Turn, Author: "sailry", Parts: []protocol.ConversationPart{{Kind: "resource", Data: json.RawMessage(data)}}}
	if err := r.feed.Delta(x.inv.Session.ID, draft); err != nil {
		return func() {}
	}
	return func() {
		r.feed.Publish(x.inv.Session.ID, protocol.ConversationChange{Kind: "hidden", Data: protocol.ConversationChangeHiddenPayload{Turn: draft.Turn, ID: draftID}})
	}
}

func (r *Runtime) executableRecord(x *execution, call canonical.ToolRequest, allowed bool) canonical.Record {
	patch, _ := json.Marshal(map[string]bool{canonical.ExecutableKey: allowed})
	record := canonical.NewRecord(x.inv.Run.Turn, "operation", canonical.Operation{Kind: "tool_meta", Data: &canonical.ToolMeta{Call: call.ID, Patch: patch}}, r.clock.Now())
	record.Provenance.Author = "assistant"
	return record
}
func responseBlock(index uint64, call canonical.ToolRequest, result ToolResult) canonical.Block {
	value := result.Value
	status := canonical.ToolSuccess
	format := result.Format
	if result.ReturnedError {
		status = canonical.ToolErrorStatus
	}
	if result.Error {
		status = canonical.ToolErrorStatus
		format = canonical.MCP
	}
	if result.Failure != nil {
		status = canonical.ToolErrorStatus
		format = canonical.MCPError
		value = map[string]any{"code": -32000, "message": result.Failure.Message, "data": result.Failure}
	}
	if value == nil {
		value = map[string]any{}
	}
	raw, _ := json.Marshal(value)
	var normalized any
	if json.Unmarshal(raw, &normalized) == nil {
		raw, _ = json.Marshal(normalized)
	}
	sum := blake3.Sum256(raw)
	return canonical.Block{Index: index, Kind: "tool_response", Data: &canonical.ToolResponse{ID: call.ID, Name: call.Name, Status: status, Format: format, Result: canonical.Payload{Value: raw}, Revision: fmt.Sprintf("%x", sum), Provider: call.Provider, Meta: result.Meta, Attachments: result.Attachments}}
}

func interruptedTool(mayHaveStarted bool) ToolResult {
	if !mayHaveStarted {
		return ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "Tool call did not start before the turn ended"}}
	}
	return ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Tool call was interrupted before completing; inspect the original operation before retrying"}}
}

// settleCalls records an interrupted result for every call the stopped or
// failed turn left unanswered, as the source finalization does.
func (r *Runtime) settleCalls(ctx context.Context, x *execution) error {
	state, err := r.load(ctx, x.inv.Session.ID)
	if err != nil {
		return err
	}
	admitted := make(map[string]bool)
	calls := pendingCalls(state.Records, admitted)
	if len(calls) == 0 {
		return nil
	}
	blocks := make([]canonical.Block, 0, len(calls))
	for _, call := range calls {
		blocks = append(blocks, responseBlock(uint64(len(blocks)), call, interruptedTool(admitted[call.ID])))
	}
	record := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, blocks...), r.clock.Now())
	record.Provenance.Author = "tool"
	return r.save(ctx, x.inv.Session.ID, record)
}

// logPanic records a recovered panic. A log field is one bounded line, so the
// stack is reduced to its innermost frames.
func logPanic(x *execution, operation, call string, value any) {
	var frames []string
	counters := make([]uintptr, 32)
	trace := runtime.CallersFrames(counters[:runtime.Callers(2, counters)])
	for {
		frame, more := trace.Next()
		if strings.HasPrefix(frame.Function, "runtime.") {
			frames = frames[:0]
		} else if len(frames) < 6 {
			frames = append(frames, frame.Function[strings.LastIndexByte(frame.Function, '/')+1:]+":"+strconv.Itoa(frame.Line))
		}
		if !more {
			break
		}
	}
	logging.Emit(slog.LevelError, slog.String("area", "agent"), slog.String("phase", logging.Failed), slog.String("operation", operation),
		slog.String("session", x.inv.Session.ID.String()), slog.String("turn", x.inv.Run.Turn.String()), slog.String("call", call),
		slog.String("exception", fmt.Sprint(value)), slog.String("detail", strings.Join(frames, " < ")))
}

func containToolPanic(x *execution, call canonical.ToolRequest, result *ToolResult) {
	if value := recover(); value != nil {
		logPanic(x, "tool", call.ID, value)
		*result = ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Tool call failed unexpectedly"}}
	}
}

func validateToolArguments(tool *Tool, call canonical.ToolRequest) ToolResult {
	var arguments any
	if err := json.Unmarshal(call.Arguments, &arguments); err != nil {
		return invalidTool("invalid tool arguments")
	}
	if err := tool.schema.Validate(arguments); err != nil {
		if validation, ok := errors.AsType[*jsonschema.ValidationError](err); ok {
			orderValidationErrors(validation)
		}
		return invalidTool(fmt.Sprintf("invalid tool arguments: %s", err))
	}
	if tool.parentSchema != nil && tool.parentSchema.Validate(arguments) != nil {
		return ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "tool arguments exceed the parent turn capture"}}
	}
	return ToolResult{}
}

func invalidTool(message string) ToolResult {
	return ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}}
}

func orderValidationErrors(err *jsonschema.ValidationError) {
	for _, cause := range err.Causes {
		orderValidationErrors(cause)
	}
	switch detail := err.ErrorKind.(type) {
	case *kind.AdditionalProperties:
		slices.Sort(detail.Properties)
	case *kind.Schema, *kind.Group, *kind.Reference, *kind.PropertyNames:
		// These groups can collect causes in map order. Combinators keep their declared branch order.
		slices.SortFunc(err.Causes, func(a, b *jsonschema.ValidationError) int {
			if order := slices.Compare(a.InstanceLocation, b.InstanceLocation); order != 0 {
				return order
			}
			return strings.Compare(a.Error(), b.Error())
		})
	}
}

func jsonTool(name, description, schema string, readOnly, concurrent bool, execute func(context.Context, canonical.ToolRequest) ToolResult) Tool {
	var input map[string]any
	if err := json.Unmarshal([]byte(schema), &input); err != nil {
		panic(err)
	}
	return Tool{Definition: fantasy.FunctionTool{Name: name, Description: description, InputSchema: input}, SchemaJSON: json.RawMessage(schema), ReadOnly: readOnly, Concurrent: concurrent, Execute: execute}
}
func returnedToolFailure(result ToolResult) protocol.ToolFailure {
	if result.ReturnedFailure != nil {
		return *result.ReturnedFailure
	}
	return protocol.ToolFailure{Origin: protocol.ToolFailureOriginTool}
}

// repeatKey identifies a call by its tool and its arguments read as JSON, so
// key order and spacing do not make two equal requests differ; "" when the
// arguments are not JSON.
func repeatKey(call canonical.ToolRequest) string {
	var arguments any
	if json.Unmarshal(call.Arguments, &arguments) != nil {
		return ""
	}
	normalized, err := json.Marshal(arguments)
	if err != nil {
		return ""
	}
	return call.Name + "\x00" + string(normalized)
}

// repeatedCall answers a read that repeats an earlier call of the same reply.
func repeatedCall(first string) ToolResult {
	text := fmt.Sprintf("Not run: this repeats call %s of the same reply, with the same tool and arguments. Use that call's result.", first)
	return ToolResult{Value: map[string]any{"content": []any{map[string]any{"type": "text", "text": text}}}}
}
