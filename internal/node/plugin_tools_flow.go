package node

import (
	"context"
	"encoding/binary"
	"encoding/json"
	"slices"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// pluginStorageHandler reports whether a handler's declared effects are
// confined to its package's own stored values.
func pluginStorageHandler(handler protocol.ToolHandler) bool {
	declared := []protocol.ToolOperation{}
	if handler.Operation != nil {
		declared = append(declared, *handler.Operation)
	}
	if handler.Flow != nil {
		declared = append(declared, handler.Flow.Operations...)
	}
	return len(declared) != 0 && !slices.ContainsFunc(declared, func(operation protocol.ToolOperation) bool {
		return operation != protocol.ToolOperationSetValue && operation != protocol.ToolOperationDeleteValue && operation != protocol.ToolOperationStorageTransaction
	})
}

// pluginFlowRuns reports whether a flow step may run an operation. A step has
// no approval of its own, so it reads, writes its package's own values or
// sends a declared request; a workspace change runs only as the bound
// operation of a handler, under that tool's approval.
func pluginFlowRuns(operation protocol.ToolOperation) bool {
	switch operation {
	case protocol.ToolOperationGetValue, protocol.ToolOperationListKeys, protocol.ToolOperationSetValue, protocol.ToolOperationDeleteValue,
		protocol.ToolOperationStorageTransaction, protocol.ToolOperationReadSettings, protocol.ToolOperationHttp:
		return true
	}
	readOnly, _ := operations.Policy(operation)
	return readOnly
}

// pluginFlowLimit is the refusal for an operation a flow step cannot run.
func pluginFlowLimit(operation protocol.ToolOperation) *protocol.Fault {
	return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin flows cannot run " + string(operation) + ": a flow step reads, writes its package's storage or sends a declared request; bind a workspace change as the handler's operation"}
}

func pluginOperationCommand(owner *pluginScriptTurn, operation protocol.ToolOperation, arguments json.RawMessage) (protocol.Command, error) {
	kind := ""
	switch operation {
	case protocol.ToolOperationGetValue:
		kind = "read_plugin_value"
	case protocol.ToolOperationListKeys:
		kind = "list_plugin_keys"
	case protocol.ToolOperationSetValue:
		kind = "write_plugin_value"
	case protocol.ToolOperationDeleteValue:
		kind = "remove_plugin_value"
	case protocol.ToolOperationStorageTransaction:
		kind = "plugin_transaction"
	case protocol.ToolOperationReadSettings:
		return protocol.Command{Kind: "read_plugin_settings", Data: &protocol.CommandReadPluginSettingsPayload{Package: owner.scope.Package}}, nil
	case protocol.ToolOperationHttp:
		kind = "request_plugin_http"
	default:
		if !pluginFlowRuns(operation) {
			return protocol.Command{}, pluginFlowLimit(operation)
		}
		return operations.Command(operation, arguments, owner.invocation.Run.Worktree, owner.invocation.Session.ID, owner.invocation.Run.Turn, owner.invocation.Worktree.Project, true)
	}
	command, err := plugins.ToolCommand(kind, arguments)
	if err != nil {
		return protocol.Command{}, err
	}
	if transaction, ok := command.Data.(*protocol.CommandPluginTransactionPayload); ok {
		for _, item := range transaction.Operations {
			if !slices.Contains([]string{"write", "index", "remove", "dispatch"}, item.Kind) {
				return protocol.Command{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "storage tools can only mutate private values and declared schedules"}
			}
		}
	}
	return command, nil
}

func (owner *pluginScriptTurn) operation(ctx context.Context, tool string, call canonical.ToolRequest, step uint64, operation protocol.ToolOperation, arguments json.RawMessage) engine.ToolResult {
	command, err := pluginOperationCommand(owner, operation, arguments)
	if err != nil {
		return workspaceToolFault(err)
	}
	if command.Durable() && owner.invocation.Turn.Config.Mode == protocol.WorkModePlan {
		return workspaceToolFault(&protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "planning mode permits read-only tools"})
	}
	request := protocol.NewRequest(owner.node.ID(), command)
	request.Plugin = &owner.scope
	identity := plugins.ToolCallIdentity(owner.invocation.Run.Turn, owner.scope.Package, plugins.ToolAlias(owner.info.Summary.Name, tool), call.ID)
	material := binary.LittleEndian.AppendUint64([]byte(identity.String()), step)
	var digest [32]byte
	blake3.DeriveKey(digest[:], "Sailry plugin bound operation v1", material)
	copy(request.ID[:], digest[:16])
	request.ID[6] = request.ID[6]&0x0f | 0x80
	request.ID[8] = request.ID[8]&0x3f | 0x80
	output, err := owner.node.dispatchCallbackResult(ctx, request)
	if err != nil {
		return workspaceToolFault(err)
	}
	value := plugins.ToolOutput(output)
	return engine.ToolResult{Value: value, ReturnedError: pluginResultFailed(value)}
}

func (owner *pluginScriptTurn) flow(ctx context.Context, name string, handler protocol.ToolHandler, call canonical.ToolRequest, arguments any, readOnly bool) engine.ToolResult {
	started := time.Now()
	var state, outcome any
	outputs := []engine.ToolResult{}
	var failure *engine.ToolResult
	message := ""
	if limit := handler.Flow.MessageChars; limit != nil {
		characters := []rune(owner.invocation.Message.Text)
		if *limit < uint64(len(characters)) {
			characters = characters[:*limit]
		}
		message = string(characters)
	}
	for step := uint64(0); step <= 8; step++ {
		body, err := owner.evaluate(ctx, handler.Name, name, call.ID, map[string]any{
			"arguments": arguments, "message": message, "state": state, "outcome": outcome, "step": step, "elapsed_ms": time.Since(started).Milliseconds(),
		}, readOnly)
		if err != nil {
			if failure != nil {
				return *failure
			}
			if len(outputs) != 0 {
				return outputs[len(outputs)-1]
			}
			return workspaceToolFault(err)
		}
		var transition struct {
			Call *struct {
				Operation protocol.ToolOperation `json:"operation"`
				Arguments json.RawMessage        `json:"arguments"`
			} `json:"call"`
			State    json.RawMessage `json:"state"`
			Optional bool            `json:"optional"`
			Result   json.RawMessage `json:"result"`
			Output   *uint64         `json:"output"`
		}
		if json.Unmarshal(body, &transition) != nil {
			return workspaceToolFault(&protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid plugin flow transition"})
		}
		if transition.Call == nil && transition.Result != nil {
			if failure != nil {
				return *failure
			}
			var result any
			_ = json.Unmarshal(transition.Result, &result)
			if transition.Output != nil {
				if *transition.Output >= uint64(len(outputs)) {
					return workspaceToolFault(&protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin flow references an unavailable output"})
				}
				captured := outputs[*transition.Output]
				if captured.Failure != nil || captured.ReturnedError {
					return captured
				}
			}
			return engine.ToolResult{Value: result, ReturnedError: pluginResultFailed(result)}
		}
		if transition.Call == nil || transition.Result != nil || step == 8 || !slices.Contains(handler.Flow.Operations, transition.Call.Operation) {
			return workspaceToolFault(&protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin flow requested an undeclared or excessive operation"})
		}
		if failure != nil {
			if failure.Failure == nil || failure.Failure.Code != protocol.ErrorCodeRevisionConflict || transition.Call.Operation != protocol.ToolOperationStorageTransaction {
				return *failure
			}
		}
		captured := owner.operation(ctx, name, call, step, transition.Call.Operation, transition.Call.Arguments)
		outputs = append(outputs, captured)
		outcome = captured.Value
		if captured.Failure != nil {
			outcome = map[string]any{"isError": true, "error": captured.Failure}
			if captured.Failure.Code == protocol.ErrorCodeOutcomeUnknown || captured.Failure.Code == protocol.ErrorCodeCancelled {
				return captured
			}
		}
		failed := captured.Failure != nil || captured.ReturnedError
		if failed && (!transition.Optional || !pluginOperationReadOnly(transition.Call.Operation)) {
			failure = &outputs[len(outputs)-1]
		} else if !failed {
			failure = nil
		}
		state = nil
		if len(transition.State) != 0 {
			_ = json.Unmarshal(transition.State, &state)
		}
	}
	return workspaceToolFault(&protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "plugin flow exceeds operation limit"})
}
