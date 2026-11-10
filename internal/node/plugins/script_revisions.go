package plugins

import (
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func scriptCommandRevisions(value any) error {
	if err := scriptConversationRevisions(value); err != nil {
		return err
	}
	kind, _ := scriptAt(value, "kind").(string)
	revisionPath := []string{"data", "package", "settings_revision"}
	if kind == "request_provider_http" || kind == "execute_provider_http" {
		revisionPath = []string{"data", "expected_revision"}
	}
	if object, ok := scriptAt(value, revisionPath[:len(revisionPath)-1]...).(map[string]any); ok {
		if revision, exists := object[revisionPath[len(revisionPath)-1]]; exists {
			converted, err := scriptRevision(revision, true, "revision must be a decimal string")
			if err != nil {
				return err
			}
			scriptSet(value, converted, revisionPath...)
		}
	}
	if kind == "plugin_transaction" {
		if err := scriptTransactionRevisions(scriptAt(value, "data", "operations")); err != nil {
			return err
		}
	}
	switch kind {
	case "write_plugin_value", "write_plugin_conversation_value", "write_indexed_plugin_value",
		"remove_plugin_value", "remove_plugin_conversation_value", "read_conversation_run",
		"submit_turn", "queue_turn", "set_session_config", "set_session_team", "continue_turn",
		"rebuild_knowledge_collection", "delete_knowledge_collection", "start_connector_login":
		revision := scriptAt(value, "data", "expected_revision")
		if revision != nil {
			converted, err := scriptRevision(revision, false, "revision must be an exact integer or decimal string")
			if err != nil {
				return err
			}
			scriptSet(value, converted, "data", "expected_revision")
		}
	}
	return nil
}

func scriptConversationRevisions(value any) error {
	switch scriptAt(value, "kind") {
	case "start_session", "set_session_team":
		if revision := scriptAt(value, "data", "team", "revision"); revision != nil {
			converted, err := scriptRevision(revision, false, "team revision must be exact")
			if err != nil {
				return err
			}
			scriptSet(value, converted, "data", "team", "revision")
		}
	case "dispatch":
		return scriptHandlerCommandRevisions(scriptAt(value, "data", "action"))
	}
	return nil
}

func scriptHandlerCommandRevisions(value any) error {
	if scriptAt(value, "kind") == "save_handler" {
		return scriptConversationRevisions(scriptAt(value, "data", "callback", "command"))
	}
	return nil
}

func scriptTransactionRevisions(value any) error {
	operations, ok := value.([]any)
	if !ok || len(operations) == 0 || len(operations) > 64 {
		return scriptInvalid("invalid plugin transaction or revision")
	}
	turns := 0
	for _, operation := range operations {
		var revisionPath []string
		switch scriptAt(operation, "kind") {
		case "write", "index", "remove", "conversation_write", "conversation_remove", "queue_pause", "submit", "continue":
			revisionPath = []string{"data", "expected_revision"}
			if scriptAt(operation, "kind") == "submit" || scriptAt(operation, "kind") == "continue" {
				turns++
			}
		case "dispatch":
			dispatch := scriptAt(operation, "data")
			if err := scriptHandlerCommandRevisions(dispatch); err != nil {
				return err
			}
			switch scriptAt(dispatch, "kind") {
			case "save_handler", "save_schedule":
				revisionPath = []string{"data", "data", "revision"}
			case "remove_handler", "remove_schedule":
				revisionPath = []string{"data", "data", "expected_revision"}
			case "list_handlers", "list_schedules", "list_jobs", "read_job", "read_result":
				return scriptInvalid("plugin transactions require mutations")
			}
		}
		if len(revisionPath) > 0 {
			revision, err := scriptRevision(scriptAt(operation, revisionPath...), false, "invalid plugin transaction or revision")
			if err != nil {
				return err
			}
			scriptSet(operation, revision, revisionPath...)
		}
	}
	if turns > 1 {
		return scriptInvalid("a transaction may admit only one turn")
	}
	if encoded, err := json.Marshal(operations); err != nil || len(encoded) > ScriptMaxDataBytes {
		return scriptInvalid("invalid plugin transaction size")
	}
	return nil
}

func scriptOutput(output protocol.Output) any {
	value, err := scriptJSONValue(output)
	if err != nil {
		return nil
	}
	data := scriptAt(value, "data")
	switch output.Kind {
	case "local_models":
		scriptSet(value, scriptAt(data, "models"), "data")
	case "roles", "teams", "knowledge_collections":
		scriptEach(data, func(row any) { scriptStringRevision(row, "revision") })
	case "team", "plugin_value", "plugin_conversation_value", "knowledge_collection":
		scriptStringRevision(data, "revision")
	case "connector_types":
		scriptEach(data, func(row any) { scriptStringRevision(row, "package", "settings_revision") })
	case "connections":
		scriptEach(data, scriptConnectionRevisions)
	case "connection", "connector_login":
		scriptConnectionRevisions(data)
	case "installations":
		scriptEach(data, func(row any) {
			scriptStringRevision(row, "revision")
			scriptSet(row, "node", "target")
		})
	case "session":
		scriptSessionRevisions(data)
	case "associated_sessions":
		scriptEach(scriptAt(data, "sessions"), scriptSessionRevisions)
	case "queued_turn":
		scriptStringRevision(data, "roles", "team", "revision")
	case "conversation":
		scriptStringRevision(data, "page", "queue", "revision")
	case "conversation_runs":
		scriptStringRevision(data, "revision")
		scriptStringRevision(data, "queue_revision")
	case "queue":
		scriptStringRevision(data, "queue", "revision")
	case "plugin_settings":
		scriptStringRevision(data, "package", "settings_revision")
	case "plugin_search":
		scriptEach(scriptAt(data, "entries"), func(row any) { scriptStringRevision(row, "revision") })
	case "plugin_transaction":
		scriptEach(data, scriptNestedOutput)
	case "dispatch":
		scriptDispatchOutputRevisions(data)
	}
	return value
}

func scriptNestedOutput(value any) {
	encoded, err := json.Marshal(value)
	if err != nil {
		return
	}
	var output protocol.Output
	if json.Unmarshal(encoded, &output) != nil {
		return
	}
	adapted, ok := scriptOutput(output).(map[string]any)
	object, isObject := value.(map[string]any)
	if ok && isObject {
		object["data"] = adapted["data"]
	}
}

func scriptDispatchOutputRevisions(value any) {
	switch scriptAt(value, "kind") {
	case "handler", "schedule":
		scriptStringRevision(value, "data", "revision")
	}
	switch scriptAt(value, "kind") {
	case "handler":
		scriptStringRevision(value, "data", "callback", "command", "data", "team", "revision")
	case "handlers":
		scriptEach(scriptAt(value, "data"), func(row any) {
			scriptStringRevision(row, "callback", "command", "data", "team", "revision")
		})
	}
}

func scriptConnectionRevisions(value any) {
	scriptStringRevision(value, "revision")
	scriptStringRevision(value, "package", "settings_revision")
}

func scriptSessionRevisions(value any) {
	scriptStringRevision(value, "revision")
	scriptStringRevision(value, "roles", "team", "revision")
}

func scriptStringRevision(value any, path ...string) {
	if revision, ok := scriptAt(value, path...).(json.Number); ok {
		scriptSet(value, revision.String(), path...)
	}
}

func scriptEach(value any, apply func(any)) {
	rows, _ := value.([]any)
	for _, row := range rows {
		apply(row)
	}
}
