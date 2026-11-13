package plugins

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"strconv"
	"strings"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func decodeHostJSON(body []byte) (any, error) {
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.UseNumber()
	var value any
	if err := decoder.Decode(&value); err != nil {
		return nil, hostInvalid("invalid plugin JSON")
	}
	return value, nil
}

func hostRevision(value any) (json.Number, error) {
	text, stringValue := value.(string)
	if !stringValue {
		number, numeric := value.(json.Number)
		if !numeric {
			return "", hostInvalid("revision must be an exact integer or decimal string")
		}
		text = number.String()
	}
	revision, err := strconv.ParseUint(text, 10, 64)
	if err != nil || strconv.FormatUint(revision, 10) != text || !stringValue && revision > 9_007_199_254_740_991 {
		return "", hostInvalid("revision must be an exact integer or decimal string")
	}
	return json.Number(text), nil
}

func hostAt(value any, path ...string) any {
	for _, key := range path {
		object, ok := value.(map[string]any)
		if !ok {
			return nil
		}
		value = object[key]
	}
	return value
}

func hostSet(value any, replacement any, path ...string) {
	if len(path) == 0 {
		return
	}
	object, ok := hostAt(value, path[:len(path)-1]...).(map[string]any)
	if ok {
		object[path[len(path)-1]] = replacement
	}
}

func commandRevision(value any, path ...string) error {
	current := hostAt(value, path...)
	if current == nil {
		return nil
	}
	converted, err := hostRevision(current)
	if err == nil {
		hostSet(value, converted, path...)
	}
	return err
}

func hostCommandRevisions(value any) error {
	if err := commandRevision(value, "data", "package", "settings_revision"); err != nil {
		return err
	}
	kind, _ := hostAt(value, "kind").(string)
	switch kind {
	case "read_tool_detail", "read_git_change", "read_git_context", "read_file_lines", "write_file", "edit_file", "delete_file", "move_file", "trash_file":
	default:
		if err := commandRevision(value, "data", "expected_revision"); err != nil {
			return err
		}
	}
	switch kind {
	case "start_session", "set_session_team":
		return commandRevision(value, "data", "team", "revision")
	case "dispatch":
		action := hostAt(value, "data", "action")
		if hostAt(action, "kind") == "save_handler" {
			return hostCommandRevisions(hostAt(action, "data", "callback", "command"))
		}
	case "plugin_transaction":
		operations, _ := hostAt(value, "data", "operations").([]any)
		for _, operation := range operations {
			switch hostAt(operation, "kind") {
			case "write", "index", "remove", "conversation_write", "conversation_remove", "queue_pause", "submit", "continue":
				if err := commandRevision(operation, "data", "expected_revision"); err != nil {
					return err
				}
			case "dispatch":
				action := hostAt(operation, "data")
				switch hostAt(action, "kind") {
				case "save_handler", "save_schedule":
					if err := commandRevision(action, "data", "revision"); err != nil {
						return err
					}
				case "remove_handler", "remove_schedule":
					if err := commandRevision(action, "data", "expected_revision"); err != nil {
						return err
					}
				}
				if hostAt(action, "kind") == "save_handler" {
					if err := hostCommandRevisions(hostAt(action, "data", "callback", "command")); err != nil {
						return err
					}
				}
			}
		}
	}
	return nil
}

func decodeHostCommand(body []byte) (protocol.Command, error) {
	value, err := decodeHostJSON(body)
	if err != nil {
		return protocol.Command{}, err
	}
	if err := hostCommandRevisions(value); err != nil {
		return protocol.Command{}, err
	}
	encoded, err := json.Marshal(value)
	if err != nil {
		return protocol.Command{}, err
	}
	var command protocol.Command
	if json.Unmarshal(encoded, &command) != nil {
		return command, hostInvalid("invalid plugin command")
	}
	return command, nil
}

func commandFrom(kind string, data any) (protocol.Command, error) {
	envelope := map[string]any{"kind": kind}
	if data != nil {
		envelope["data"] = data
	}
	body, err := json.Marshal(envelope)
	if err != nil {
		return protocol.Command{}, err
	}
	return decodeHostCommand(body)
}

func hostStringRevision(value any, path ...string) {
	if revision, ok := hostAt(value, path...).(json.Number); ok {
		hostSet(value, revision.String(), path...)
	}
}

func hostRows(value any, operation func(any)) {
	rows, _ := value.([]any)
	for _, row := range rows {
		operation(row)
	}
}

func publicHostOutput(output protocol.Output) (any, error) {
	if err := publicHostOutputAllowed(output); err != nil {
		return nil, err
	}
	body, err := json.Marshal(output)
	if err != nil {
		return nil, err
	}
	value, err := decodeHostJSON(body)
	if err != nil {
		return nil, err
	}
	data := hostAt(value, "data")
	switch output.Kind {
	case "plugin_transaction":
		hostRows(data, func(row any) {
			body, _ := json.Marshal(row)
			var nested protocol.Output
			if json.Unmarshal(body, &nested) == nil {
				adapted, err := publicHostOutput(nested)
				if err == nil {
					hostSet(row, hostAt(adapted, "data"), "data")
				}
			}
		})
	case "local_models":
		hostSet(value, hostAt(data, "models"), "data")
	case "team", "plugin_value", "plugin_conversation_value", "knowledge_collection":
		hostStringRevision(data, "revision")
	case "roles", "teams", "knowledge_collections":
		hostRows(data, func(row any) { hostStringRevision(row, "revision") })
	case "session":
		hostStringRevision(data, "revision")
		hostStringRevision(data, "roles", "team", "revision")
	case "associated_sessions":
		hostRows(hostAt(data, "sessions"), func(row any) {
			hostStringRevision(row, "revision")
			hostStringRevision(row, "roles", "team", "revision")
		})
	case "queued_turn":
		hostStringRevision(data, "roles", "team", "revision")
	case "conversation":
		hostStringRevision(data, "page", "queue", "revision")
	case "conversation_runs":
		hostStringRevision(data, "revision")
		hostStringRevision(data, "queue_revision")
	case "queue":
		hostStringRevision(data, "queue", "revision")
	case "plugin_settings":
		hostStringRevision(data, "package", "settings_revision")
	case "plugin_search":
		hostRows(hostAt(data, "entries"), func(row any) { hostStringRevision(row, "revision") })
	case "connector_types":
		hostRows(data, func(row any) { hostStringRevision(row, "package", "settings_revision") })
	case "connections":
		hostRows(data, hostConnectionRevisions)
	case "connection", "connector_login":
		hostConnectionRevisions(data)
	case "database_outcome":
		if hostAt(data, "kind") == "query" {
			hostStringRevision(data, "data", "affected_rows")
			hostRows(hostAt(data, "data", "rows"), func(row any) {
				hostRows(row, func(cell any) {
					if hostAt(cell, "kind") == "integer" {
						hostStringRevision(cell, "value")
					}
				})
			})
		}
	case "installations":
		hostRows(data, func(row any) {
			hostStringRevision(row, "revision")
			hostSet(row, "node", "target")
		})
	case "dispatch":
		if hostAt(data, "kind") == "handler" || hostAt(data, "kind") == "schedule" {
			hostStringRevision(data, "data", "revision")
		}
		if hostAt(data, "kind") == "handler" {
			hostStringRevision(data, "data", "callback", "command", "data", "team", "revision")
		}
		if hostAt(data, "kind") == "handlers" {
			hostRows(hostAt(data, "data"), func(row any) {
				hostStringRevision(row, "callback", "command", "data", "team", "revision")
			})
		}
	}
	return value, nil
}

func publicHostOutputAllowed(output protocol.Output) error {
	if output.Kind == "plugin_secret" || output.Kind == "provider_key" {
		return hostInvalid("protected credentials are not exposed to scripts")
	}
	if output.Kind == "plugin_transaction" {
		switch nested := output.Data.(type) {
		case []protocol.Output:
			for _, item := range nested {
				if err := publicHostOutputAllowed(item); err != nil {
					return err
				}
			}
		case *[]protocol.Output:
			if nested != nil {
				for _, item := range *nested {
					if err := publicHostOutputAllowed(item); err != nil {
						return err
					}
				}
			}
		}
	}
	return nil
}

func hostConnectionRevisions(value any) {
	hostStringRevision(value, "revision")
	hostStringRevision(value, "package", "settings_revision")
}

func hostErrorCode(message string) string {
	if tail, ok := strings.CutPrefix(message, "`sailry/sdk."); ok {
		method, code, found := strings.Cut(tail, "`: ")
		if found && hostIdentifier(method) {
			message = code
		}
	}
	switch message {
	case "configure", "invalidResponse", "unconfirmed", "timedOut":
		return message
	}
	return "failed"
}

func hostFaultCode(message string) string {
	if strings.HasPrefix(message, "`sailry/") {
		_, code, found := strings.Cut(message, "`: ")
		if found {
			message = code
		}
	}
	var failure protocol.Fault
	if json.Unmarshal([]byte(message), &failure) == nil {
		return string(failure.Code)
	}
	if message == "unconfirmed" {
		return string(protocol.ErrorCodeOutcomeUnknown)
	}
	return "failed"
}

func hostIdentifier(value string) bool {
	for index, character := range value {
		if character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' || character == '_' || character == '$' || index > 0 && character >= '0' && character <= '9' {
			continue
		}
		return false
	}
	return value != ""
}

func hostNativeError(module, operation string, err error) error {
	var transported *link.FaultError
	var failure *protocol.Fault
	if errors.As(err, &transported) || errors.As(err, &failure) {
		body, marshalErr := json.Marshal(hostFault(err))
		if marshalErr == nil {
			return fmt.Errorf("`%s.%s`: %s", module, operation, body)
		}
	}
	return fmt.Errorf("`%s.%s`: %w", module, operation, err)
}

// PublicToolResult is a tool's result as a script reads it: a Node output
// takes the scripts' public form, and any other value, or an output scripts
// may not read in that form, stays as the model received it.
func PublicToolResult(result json.RawMessage) json.RawMessage {
	var output protocol.Output
	if json.Unmarshal(result, &output) != nil {
		return result
	}
	value, err := publicHostOutput(output)
	if err != nil {
		return result
	}
	body, err := json.Marshal(value)
	if err != nil {
		return result
	}
	return body
}
