package plugins

import (
	"bytes"
	"context"
	"encoding/binary"
	"encoding/json"
	"errors"
	"io"
	"strconv"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type scriptDraft struct {
	request protocol.Request
	kind    string
}

type scriptBridge struct {
	ctx           context.Context
	deadline      time.Time
	target        protocol.NodeID
	scope         protocol.PluginContext
	execution     *ScriptExecution
	dispatch      ScriptDispatch
	drafts        map[string]scriptDraft
	calls         int
	writes        uint64
	faults        map[string]*protocol.Fault
	installations map[protocol.RequestID]bool
	failure       *protocol.Fault
}

func (bridge *scriptBridge) call(operation, body string) string {
	value, err := bridge.operation(operation, body)
	response := map[string]any{"Ok": value}
	if err != nil {
		fault := scriptFault(err)
		response = map[string]any{"Err": fault}
		if len(bridge.faults) < 128 {
			identity := protocol.NewRequestID().String()
			bridge.faults[identity] = fault
			response["fault_id"] = identity
		}
	}
	encoded, err := json.Marshal(response)
	if err != nil {
		return `{"Err":{"code":"invalid_request","message":"plugin SDK result is not JSON"}}`
	}
	return string(encoded)
}

func (bridge *scriptBridge) operation(operation, body string) (any, error) {
	if bridge.ctx.Err() != nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "plugin callback stopped"}
	}
	if !time.Now().Before(bridge.deadline) {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeExpired, Message: "plugin callback response timed out"}
	}
	if len(body) > ScriptMaxDataBytes || bridge.calls >= 1024 {
		return nil, scriptInvalid("plugin callback SDK budget exhausted")
	}
	bridge.calls++
	switch operation {
	case "id.new":
		return protocol.NewRequestID(), nil
	case "id.call", "turn.read", "turn.stage":
		return bridge.toolState(operation, body)
	case "context":
		value, err := scriptJSONValue(bridge.scope)
		if err != nil {
			return nil, err
		}
		scriptStringRevision(value, "package", "settings_revision")
		return value, nil
	case "prepare", "read", "knowledge.create", "models.prepare", "models.register", "models.list":
		return bridge.prepare(operation, body)
	case "complete":
		var identity string
		if json.Unmarshal([]byte(body), &identity) != nil {
			return nil, scriptInvalid("invalid request ID")
		}
		draft, found := bridge.drafts[identity]
		if !found {
			return nil, scriptInvalid("plugin request draft is unavailable")
		}
		if draft.request.Command.Kind == "install_local_model" {
			if !bridge.installations[draft.request.ID] && len(bridge.installations) >= 64 {
				return nil, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "callback installation observations are full"}
			}
			bridge.installations[draft.request.ID] = true
		}
		value, err := bridge.send(draft)
		if err != nil {
			return map[string]any{"Err": scriptFault(err)}, nil
		}
		return map[string]any{"Ok": scriptOutput(value)}, nil
	case "forget":
		var identity string
		if json.Unmarshal([]byte(body), &identity) != nil {
			return nil, scriptInvalid("invalid request ID")
		}
		delete(bridge.drafts, identity)
		return nil, nil
	default:
		return nil, scriptInvalid("unknown host SDK operation")
	}
}

func (bridge *scriptBridge) toolState(operation, body string) (any, error) {
	execution := bridge.execution
	if execution == nil {
		return nil, scriptInvalid("operation requires an Agent tool invocation")
	}
	if operation == "id.call" {
		if execution.CallID == nil {
			return nil, scriptInvalid("operation requires an Agent tool call")
		}
		return execution.CallID, nil
	}
	if len(execution.Turn) == 0 {
		return nil, scriptInvalid("package has no turn policy")
	}
	if operation == "turn.read" {
		if len(execution.StagedTurn) != 0 {
			return execution.StagedTurn, nil
		}
		return execution.Turn, nil
	}
	if len(body) > ScriptMaxDataBytes {
		return nil, scriptInvalid("plugin turn state exceeds limit")
	}
	if !json.Valid([]byte(body)) {
		return nil, scriptInvalid("invalid turn state")
	}
	execution.StagedTurn = json.RawMessage(body)
	return execution.StagedTurn, nil
}

func (bridge *scriptBridge) prepare(operation, body string) (any, error) {
	value, err := scriptDecodeJSON([]byte(body))
	if err != nil {
		return nil, scriptInvalid("invalid SDK command")
	}
	if operation == "knowledge.create" {
		value, err = bridge.knowledgeCommand([]byte(body))
	} else if strings.HasPrefix(operation, "models.") {
		value, err = scriptModelCommand(operation, []byte(body))
	}
	if err != nil {
		return nil, err
	}
	if err := scriptCommandRevisions(value); err != nil {
		return nil, err
	}
	encoded, err := json.Marshal(value)
	if err != nil {
		return nil, scriptInvalid("invalid SDK command")
	}
	var command protocol.Command
	if json.Unmarshal(encoded, &command) != nil {
		return nil, scriptInvalid("invalid SDK command")
	}
	if err := bridge.installationOwnership(command); err != nil {
		return nil, err
	}
	if command.Durable() && bridge.execution != nil {
		if bridge.execution.Mode == protocol.WorkModePlan {
			return nil, scriptPermission("planning turns cannot dispatch durable commands")
		}
		if bridge.execution.ReadOnly {
			return nil, scriptPermission("read-only script tools cannot dispatch durable commands")
		}
		if !scriptToolWrite(command, value) {
			return nil, scriptPermission("script tools cannot dispatch durable commands")
		}
	}
	if !scriptCommandAllowed(command.Kind) {
		return nil, scriptPermission("command is not exposed to host callbacks")
	}
	request := protocol.NewRequest(bridge.target, command)
	scope := bridge.scope
	request.Plugin = &scope
	if command.Durable() && bridge.execution != nil {
		if bridge.execution.CallID == nil {
			return nil, scriptInvalid("durable writes require a tool identity")
		}
		request.ID = scriptWriteIdentity(*bridge.execution.CallID, bridge.writes)
		bridge.writes++
	}
	draft := scriptDraft{request: request, kind: command.Kind}
	if operation == "read" || operation == "models.list" {
		if command.Durable() {
			return nil, scriptInvalid("SDK reads cannot execute writes")
		}
		result, err := bridge.send(draft)
		if err != nil {
			return nil, err
		}
		return scriptOutput(result), nil
	}
	if len(bridge.drafts) >= 64 {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "plugin request drafts are full"}
	}
	identity := request.ID.String()
	bridge.drafts[identity] = draft
	return identity, nil
}

func (bridge *scriptBridge) installationOwnership(command protocol.Command) error {
	if command.Kind == "read_installations" {
		payload, ok := command.Data.(*protocol.CommandReadInstallationsPayload)
		if !ok {
			return scriptInvalid("invalid installation requests")
		}
		seen := make(map[protocol.RequestID]bool, len(payload.Requests))
		if len(payload.Requests) == 0 || len(payload.Requests) > 64 {
			return scriptInvalid("installation request IDs must be bounded and unique")
		}
		for _, identity := range payload.Requests {
			if seen[identity] {
				return scriptInvalid("installation request IDs must be bounded and unique")
			}
			seen[identity] = true
			if !bridge.installations[identity] {
				return scriptPermission("installation request is not owned by this callback")
			}
		}
	} else if command.Kind == "dismiss_installation" {
		payload, ok := command.Data.(*protocol.CommandDismissInstallationPayload)
		if !ok || !bridge.installations[payload.Request] {
			return scriptPermission("installation request is not owned by this callback")
		}
	}
	return nil
}

func (bridge *scriptBridge) send(draft scriptDraft) (protocol.Output, error) {
	if bridge.execution != nil && bridge.execution.ReadOnly && (draft.request.Command.Durable() ||
		draft.kind == "request_computer_permission" || draft.kind == "dismiss_installation" || draft.kind == "cancel_web_fetch") {
		return protocol.Output{}, scriptPermission("read-only callbacks cannot dispatch mutations")
	}
	if bridge.dispatch == nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "plugin callback dispatch is unavailable"}
	}
	ctx, cancel := context.WithDeadline(bridge.ctx, bridge.deadline)
	defer cancel()
	value, err := bridge.dispatch(ctx, draft.request)
	if err == nil {
		return value, nil
	}
	fault := scriptFault(err)
	if fault.Details != nil && fault.Details.Kind == "system_diagnostic" {
		report, ok := fault.Details.Data.(protocol.NotificationSystemSystemDiagnostic)
		if pointer, isPointer := fault.Details.Data.(*protocol.NotificationSystemSystemDiagnostic); isPointer && pointer != nil {
			report, ok = *pointer, true
		}
		if ok {
			copy := *fault
			if report.Request == nil {
				report.Request = &draft.request.ID
			}
			report.Operation = &draft.kind
			copy.Details = &protocol.FaultDetails{Kind: "system_diagnostic", Data: report}
			fault = &copy
		}
	}
	if bridge.ctx.Err() != nil || !time.Now().Before(bridge.deadline) {
		bridge.failure = fault
	}
	return protocol.Output{}, fault
}

func (bridge *scriptBridge) knowledgeCommand(encoded []byte) (any, error) {
	var draft protocol.KnowledgeDraft
	if json.Unmarshal(encoded, &draft) != nil {
		return nil, scriptInvalid("invalid knowledge collection draft")
	}
	source := protocol.KnowledgeSource{Kind: draft.Source.Kind}
	switch payload := draft.Source.Data.(type) {
	case *protocol.KnowledgeDraftSourceWorktreePayload:
		if bridge.scope.Worktree == nil {
			return nil, scriptInvalid("knowledge source requires a captured worktree")
		}
		source.Data = protocol.KnowledgeSourceWorktreePayload{Worktree: *bridge.scope.Worktree, Path: payload.Path}
	case *protocol.KnowledgeDraftSourceStoragePayload:
		source.Data = protocol.KnowledgeSourceStoragePayload{Scope: payload.Scope, Prefix: payload.Prefix}
	default:
		return nil, scriptInvalid("invalid knowledge collection draft")
	}
	definition := protocol.KnowledgeDefinition{
		Name: draft.Name, Source: source, Model: draft.Model, Globs: draft.Globs,
		Tags: draft.Tags, SharedWith: []string{},
	}
	return scriptJSONValue(protocol.Command{Kind: "create_knowledge_collection", Data: protocol.CommandCreateKnowledgeCollectionPayload{Definition: definition}})
}

func scriptModelCommand(operation string, body []byte) (any, error) {
	var command protocol.Command
	switch operation {
	case "models.list":
		var input struct {
			Target protocol.InstallationTarget `json:"target"`
			Kind   *protocol.ModelsKind        `json:"kind"`
		}
		if protocol.DecodeObject(body, &input, []string{"target"}, true) != nil {
			return nil, scriptInvalid("invalid model list")
		}
		if input.Target != protocol.InstallationTargetNode || input.Kind != nil && *input.Kind == protocol.ModelsKindSpeech {
			return nil, scriptInvalid("Node local model list required")
		}
		command = protocol.Command{Kind: "read_local_models", Data: protocol.CommandReadLocalModelsPayload{Kind: input.Kind}}
	case "models.prepare":
		var input protocol.InstallationModelRef
		if json.Unmarshal(body, &input) != nil {
			return nil, scriptInvalid("invalid model installation")
		}
		if input.Kind == "controller" {
			payload := input.Data.(*protocol.InstallationModelRefControllerPayload)
			if payload.Kind != protocol.ModelsKindSpeech {
				return nil, scriptInvalid("model installation target or identity is invalid")
			}
			return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "speech installation requires a controller"}
		}
		payload := input.Data.(*protocol.InstallationModelRefNodePayload)
		if payload.Kind == protocol.ModelsKindSpeech || !scriptModelID(payload.ID) {
			return nil, scriptInvalid("model installation target or identity is invalid")
		}
		command = protocol.Command{Kind: "install_local_model", Data: protocol.CommandInstallLocalModelPayload{ID: payload.ID}}
	case "models.register":
		var input protocol.InstallationRegistration
		if json.Unmarshal(body, &input) != nil {
			return nil, scriptInvalid("invalid model registration")
		}
		if input.Target == protocol.InstallationTargetController {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "speech registration requires a controller"}
		}
		command = protocol.Command{Kind: "register_local_model", Data: protocol.CommandRegisterLocalModelPayload{Definition: input.Definition}}
	default:
		return nil, scriptInvalid("unknown host SDK operation")
	}
	return scriptJSONValue(command)
}

func scriptWriteIdentity(call protocol.RequestID, sequence uint64) protocol.RequestID {
	material := []byte(call.String())
	material = binary.LittleEndian.AppendUint64(material, sequence)
	var identity protocol.RequestID
	blake3.DeriveKey(identity[:], "Sailry plugin tool write v1", material)
	identity[6] = identity[6]&0x0f | 0x80
	identity[8] = identity[8]&0x3f | 0x80
	return identity
}

func scriptToolWrite(command protocol.Command, value any) bool {
	switch command.Kind {
	case "write_plugin_value", "remove_plugin_value", "write_plugin_conversation_value",
		"remove_plugin_conversation_value", "register_local_model", "install_local_model":
		return true
	case "plugin_transaction":
		operations, _ := scriptAt(value, "data", "operations").([]any)
		for _, operation := range operations {
			switch scriptAt(operation, "kind") {
			case "write", "remove", "conversation_write", "conversation_remove", "queue_pause", "continue", "stop":
			default:
				return false
			}
		}
		return len(operations) > 0
	default:
		return false
	}
}

func scriptCommandAllowed(kind string) bool {
	switch kind {
	case "read_computer_permissions", "request_computer_permission", "request_provider_http",
		"list_connector_types", "list_connections", "start_connector_login", "cancel_connector_login",
		"execute_provider_http", "plugin_transaction", "list_roles", "list_teams",
		"list_associated_sessions", "read_artifact", "list_artifacts", "list_artifact_revisions",
		"read_session", "read_conversation", "read_conversation_runs", "read_conversation_run",
		"read_turn_capture", "read_turn", "submit_turn", "queue_turn", "set_session_config",
		"set_session_team", "continue_turn", "stop_turn", "start_session", "read_project_catalog",
		"resolve_plugin_model", "search_models", "read_model_versions", "resolve_model",
		"read_local_models", "decide", "register_local_model", "install_local_model",
		"read_installations", "dismiss_installation", "read_plugin_value",
		"read_plugin_conversation_value", "list_plugin_keys", "write_plugin_value",
		"write_plugin_conversation_value", "write_indexed_plugin_value", "search_plugin_values",
		"fetch_web", "cancel_web_fetch", "query_knowledge", "list_knowledge_collections",
		"create_knowledge_collection", "rebuild_knowledge_collection", "delete_knowledge_collection",
		"remove_plugin_value", "remove_plugin_conversation_value", "read_plugin_settings",
		"read_plugin_connection", "publish_notification", "dispatch", "stop_dispatch_turn",
		"read_file", "search_code", "outline_code", "graph_code", "cancel_code_index_query",
		"read_file_lines", "find_files", "write_file", "edit_file", "delete_file", "move_file",
		"create_directory", "list_directory", "search_files", "inspect_git", "read_git_diff",
		"read_git_log", "read_git_show", "read_git_blame", "list_git_changes", "read_git_change",
		"read_git_context", "read_git_commit", "list_git_branches", "resolve_git_revision":
		return true
	default:
		return false
	}
}

func scriptDecodeJSON(encoded []byte) (any, error) {
	decoder := json.NewDecoder(bytes.NewReader(encoded))
	decoder.UseNumber()
	var value any
	if err := decoder.Decode(&value); err != nil {
		return nil, err
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return nil, scriptInvalid("invalid SDK JSON")
	}
	return value, nil
}

func scriptJSONValue(value any) (any, error) {
	encoded, err := json.Marshal(value)
	if err != nil {
		return nil, err
	}
	return scriptDecodeJSON(encoded)
}

func scriptAt(value any, path ...string) any {
	for _, name := range path {
		object, ok := value.(map[string]any)
		if !ok {
			return nil
		}
		value = object[name]
	}
	return value
}

func scriptSet(value any, replacement any, path ...string) {
	if len(path) == 0 {
		return
	}
	object, ok := scriptAt(value, path[:len(path)-1]...).(map[string]any)
	if ok {
		object[path[len(path)-1]] = replacement
	}
}

func scriptPermission(message string) *protocol.Fault {
	return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: message}
}

func scriptFault(err error) *protocol.Fault {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return fault
	}
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "plugin host service failed"}
}

func scriptModelID(identity string) bool {
	if identity == "" || len(identity) > 128 {
		return false
	}
	for _, character := range identity {
		if !(character >= 'a' && character <= 'z' || character >= '0' && character <= '9' || character == '-') {
			return false
		}
	}
	return true
}

func scriptRevision(value any, stringOnly bool, invalid string) (json.Number, error) {
	text, isString := value.(string)
	if !isString {
		number, ok := value.(json.Number)
		if !ok || stringOnly {
			return "", scriptInvalid(invalid)
		}
		text = number.String()
	}
	parsed, err := strconv.ParseUint(text, 10, 64)
	if err != nil || strconv.FormatUint(parsed, 10) != text || !isString && parsed > 9_007_199_254_740_991 {
		return "", scriptInvalid(invalid)
	}
	return json.Number(text), nil
}
