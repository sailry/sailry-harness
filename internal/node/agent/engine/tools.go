package engine

import (
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
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/santhosh-tekuri/jsonschema/v6"
	"lukechampine.com/blake3"
)

// Tool is one captured declaration and executor. Concurrency and authority are
// properties of the captured registration, never arguments supplied by a model.
type Tool struct {
	Definition        fantasy.FunctionTool
	SchemaJSON        json.RawMessage
	ReadOnly          bool
	Concurrent        bool
	Deferred          bool
	Delegation        bool
	Approval          bool
	Binding           *store.ApprovalBinding
	Execute           func(context.Context, canonical.ToolRequest) ToolResult
	Prepare           func(context.Context, canonical.ToolRequest) error
	ConfigureApproval func(context.Context, *sql.Conn, canonical.ToolRequest, protocol.ConversationApproval) (protocol.ConversationApproval, error)
	Approved          func(canonical.ToolRequest, protocol.ApprovalID, protocol.RequestID)
	Close             func() error
	Instruction       string
	ContextParts      []string
	BeforeModel       func(context.Context, []canonical.Record) (bool, error)
	BindDelegation    func(Tool) Tool
	// Guidance makes BindDelegation receive guide_agent instead of spawn_agent.
	Guidance bool
	schema   *jsonschema.Schema
	// ExecutePrepared runs a call with arguments a package prepared. The call
	// keeps the identity the model sent; the arguments are what executes.
	ExecutePrepared func(context.Context, canonical.ToolRequest, json.RawMessage) ToolResult
}
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
	Attachments []canonical.Attachment
}

func (r *Runtime) captureTools(x *execution) error {
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
	if err := r.applyAssistant(x); err != nil {
		return err
	}
	if x.inv.Session.Delegation != nil && *x.inv.Session.Delegation != nil {
		if x.childRole != nil && x.childRole.Tools != nil {
			for _, selection := range *x.childRole.Tools {
				if !slices.ContainsFunc(x.tools, func(tool Tool) bool { return roleToolMatches(selection, tool) }) {
					return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "role references an unavailable tool"}
				}
			}
		}
		x.tools = slices.DeleteFunc(x.tools, func(tool Tool) bool { return !roleAllowsTool(x.childRole, tool) })
	}
	for i := range x.tools {
		tool := &x.tools[i]
		compiler := jsonschema.NewCompiler()
		compiler.UseLoader(jsonschema.SchemeURLLoader{})
		location := "urn:sailry:tool:" + tool.Definition.Name
		if err := compiler.AddResource(location, tool.Definition.InputSchema); err != nil {
			return err
		}
		schema, err := compiler.Compile(location)
		if err != nil {
			return err
		}
		tool.schema = schema
	}
	return nil
}

func (r *Runtime) closeTools(x *execution) error {
	var failures []error
	for _, close := range x.toolCleanup {
		failures = append(failures, close())
	}
	x.toolCleanup = nil
	if errors.Join(failures...) != nil {
		return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "tool resource cleanup could not be confirmed"}
	}
	return nil
}
func (r *Runtime) declarations(x *execution, binding model.Binding, state store.AgentState) []fantasy.FunctionTool {
	var values []fantasy.FunctionTool
	if !capturedModel(binding).Tools {
		return values
	}
	for _, tool := range x.tools {
		if !tool.Deferred || slices.Contains(state.LoadedTools, tool.Definition.Name) {
			values = append(values, tool.Definition)
		}
	}
	return values
}
func findTool(tools []Tool, name string) *Tool {
	for i := range tools {
		if tools[i].Definition.Name == name {
			return &tools[i]
		}
	}
	return nil
}

func (r *Runtime) runTools(x *execution, state store.AgentState, calls []canonical.ToolRequest) error {
	results := make([]ToolResult, len(calls))
	executable := make([]bool, len(calls))
	var marks []canonical.Record
	// Mark every allowed call before requesting the first approval.
	for i, call := range calls {
		tool := findTool(x.tools, call.Name)
		if call.Error != nil {
			results[i] = ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: call.Error.Message}}
			continue
		}
		// A captured deferred tool called by name runs under its own approval:
		// the saved request already loaded it, as in the Rust catalog.
		if tool == nil {
			results[i] = ToolResult{Error: true, Value: map[string]any{"content": []any{map[string]any{"type": "text", "text": "This tool call was not handled."}}, "isError": true}}
			continue
		}
		if x.inv.Turn.Config.Mode == protocol.WorkModePlan && !tool.ReadOnly && !tool.Delegation && call.Name != "ask_user" {
			results[i] = ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "planning mode permits read-only tools"}}
			continue
		}
		var arguments any
		if err := json.Unmarshal(call.Arguments, &arguments); err != nil {
			results[i] = invalidTool("invalid tool arguments")
			continue
		}
		if err := tool.schema.Validate(arguments); err != nil {
			results[i] = invalidTool(fmt.Sprintf("invalid tool arguments: %s", err))
			continue
		}
		if tool.Prepare != nil {
			if err := tool.Prepare(x.ctx, call); err != nil {
				results[i] = toolFault(err)
				continue
			}
		}
		if !tool.Approval {
			executable[i] = true
			marks = append(marks, r.executableRecord(x, call, true))
		}
	}
	if err := r.save(x.ctx, x.inv.Session.ID, marks...); err != nil {
		return err
	}
	for i, call := range calls {
		tool := findTool(x.tools, call.Name)
		if tool == nil || !tool.Approval || results[i].Value != nil || results[i].Failure != nil {
			continue
		}
		approved, err := r.approval(x, call, tool)
		if err != nil {
			return err
		}
		executable[i] = approved
		if !approved {
			results[i] = ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "Tool call was denied"}}
		}
		if err = r.save(x.ctx, x.inv.Session.ID, r.executableRecord(x, call, approved)); err != nil {
			return err
		}
	}
	concurrent := func(i int) bool {
		tool := findTool(x.tools, calls[i].Name)
		return !executable[i] || tool != nil && tool.ReadOnly && tool.Concurrent && !x.hooks.WrapsTools()
	}
	for start := 0; start < len(calls); {
		if x.ctx.Err() != nil {
			break
		}
		end := start + 1
		if concurrent(start) {
			for end < len(calls) && concurrent(end) {
				end++
			}
		}
		// At most eight calls of a run execute at once; a finished call frees
		// its slot for the next one.
		slots := make(chan struct{}, 8)
		var workers sync.WaitGroup
		for i := start; i < end; i++ {
			if !executable[i] {
				continue
			}
			slots <- struct{}{}
			// The first eight calls of a run always start; a stopped turn starts none after them.
			if i >= start+8 && x.ctx.Err() != nil {
				<-slots
				break
			}
			workers.Add(1)
			go func(i int) {
				defer workers.Done()
				defer func() { <-slots }()
				metric, started := r.telemetry.Capture(), time.Now()
				defer func() { observeTool(metric, x.ctx, x.tools, calls[i].Name, results[i], started) }()
				// A panicking tool fails its own call; the turn and the Node continue.
				defer func() {
					if value := recover(); value != nil {
						logPanic(x, "tool", calls[i].ID, value)
						results[i] = ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Tool call failed unexpectedly"}}
					}
				}()
				if err := x.hooks.Before(x.ctx, calls[i].Name); err != nil {
					results[i] = toolFault(err)
					return
				}
				results[i] = findTool(x.tools, calls[i].Name).Execute(x.ctx, calls[i])
				x.hooks.After(x.ctx, calls[i].Name, results[i].Failure == nil && !results[i].Error && !results[i].ReturnedError)
			}(i)
		}
		workers.Wait()
		start = end
	}
	blocks := make([]canonical.Block, 0, len(calls))
	var contextDelta canonical.Context
	for i, call := range calls {
		if executable[i] && results[i].Value == nil && results[i].Failure == nil {
			results[i] = ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Tool call was interrupted before completing"}}
		}
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
	return nil
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

// settleCalls records an interrupted result for every call the stopped or
// failed turn left unanswered, as the source finalization does.
func (r *Runtime) settleCalls(ctx context.Context, x *execution) error {
	state, err := r.load(ctx, x.inv.Session.ID)
	if err != nil {
		return err
	}
	calls := pending(state.Records)
	if len(calls) == 0 {
		return nil
	}
	blocks := make([]canonical.Block, 0, len(calls))
	for _, call := range calls {
		blocks = append(blocks, responseBlock(uint64(len(blocks)), call, ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Tool call was interrupted before completing"}}))
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

func invalidTool(message string) ToolResult {
	return ToolResult{Failure: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}}
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
