package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func ReadPluginCommand(ctx context.Context, db *sql.Conn, request protocol.Request) (protocol.Output, error) {
	switch request.Command.Kind {
	case "list_plugins":
		value, err := ReadPlugins(ctx, db)
		return protocol.Output{Kind: "plugins", Data: value}, err
	case "read_plugin":
		input := request.Command.Data.(*protocol.CommandReadPluginPayload)
		value, err := ReadPlugin(ctx, db, input.Name)
		if err == nil && value == nil {
			err = fault(protocol.ErrorCodeNotFound, "plugin is not installed")
		}
		return protocol.Output{Kind: "plugin", Data: value}, err
	case "read_plugin_version":
		value, err := ReadPluginVersion(ctx, db, request.Command.Data.(*protocol.CommandReadPluginVersionPayload).Package)
		return protocol.Output{Kind: "plugin", Data: value}, err
	case "read_plugin_settings":
		value, err := ReadPluginSettings(ctx, db, request.Command.Data.(*protocol.CommandReadPluginSettingsPayload).Package)
		return protocol.Output{Kind: "plugin_settings", Data: value}, err
	}
	if request.Plugin == nil {
		return protocol.Output{}, fault(protocol.ErrorCodePermissionDenied, "plugin provenance is required")
	}
	reference := request.Plugin.Package
	switch input := request.Command.Data.(type) {
	case *protocol.CommandReadPluginValuePayload:
		value, err := ReadPluginValue(ctx, db, reference, input.Key)
		return protocol.Output{Kind: "plugin_value", Data: value}, err
	case *protocol.CommandReadPluginConversationValuePayload:
		if request.Plugin.Session == nil {
			return protocol.Output{}, fault(protocol.ErrorCodePermissionDenied, "plugin has no conversation scope")
		}
		value, err := ReadPluginConversationValue(ctx, db, reference, *request.Plugin.Session, input.Key)
		return protocol.Output{Kind: "plugin_conversation_value", Data: value}, err
	case *protocol.CommandListPluginKeysPayload:
		value, err := ListPluginKeys(ctx, db, reference, input.Prefix, input.After, input.Limit)
		return protocol.Output{Kind: "plugin_keys", Data: value}, err
	case *protocol.PluginStorageSearch:
		value, err := SearchPluginValues(ctx, db, reference, *input)
		return protocol.Output{Kind: "plugin_search", Data: value}, err
	default:
		return protocol.Output{}, fault(protocol.ErrorCodeNotConfigured, "plugin command is unavailable")
	}
}

func ExecutePluginStorageCommand(ctx context.Context, db *sql.Conn, node protocol.NodeID, profile string, caller protocol.NodeID, request protocol.Request) (protocol.Output, []protocol.Event, error) {
	if request.Plugin == nil {
		return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "plugin provenance is required")
	}
	scope := request.Plugin
	var value any
	var err error
	kind := "plugin_value"
	switch input := request.Command.Data.(type) {
	case *protocol.CommandWritePluginValuePayload:
		value, err = WritePluginValue(ctx, db, scope.Package, input.Key, input.Value, input.ExpectedRevision, nil)
	case *protocol.CommandWriteIndexedPluginValuePayload:
		value, err = WritePluginValue(ctx, db, scope.Package, input.Key, input.Value, input.ExpectedRevision, &input.Index)
	case *protocol.CommandRemovePluginValuePayload:
		value, err = RemovePluginValue(ctx, db, scope.Package, input.Key, input.ExpectedRevision)
	case *protocol.CommandWritePluginConversationValuePayload:
		if scope.Session == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "plugin has no conversation scope")
		}
		kind = "plugin_conversation_value"
		value, err = WritePluginConversationValue(ctx, db, scope.Package, *scope.Session, scope.Turn, input.Key, input.Value, input.ExpectedRevision)
	case *protocol.CommandRemovePluginConversationValuePayload:
		if scope.Session == nil {
			return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "plugin has no conversation scope")
		}
		kind = "plugin_conversation_value"
		value, err = WritePluginConversationValue(ctx, db, scope.Package, *scope.Session, scope.Turn, input.Key, nil, input.ExpectedRevision)
	case *protocol.CommandPluginTransactionPayload:
		if len(input.Operations) == 0 || len(input.Operations) > 64 {
			return protocol.Output{}, nil, invalid("plugin transaction must contain 1 to 64 operations")
		}
		outputs := make([]protocol.Output, 0, len(input.Operations))
		var checkpoint uint64
		if err := db.QueryRowContext(ctx, "SELECT coalesce(max(sequence),0) FROM plugin_conversation_records").Scan(&checkpoint); err != nil {
			return protocol.Output{}, nil, err
		}
		var admitted *protocol.QueuedTurn
		var events []protocol.Event
		for _, operation := range input.Operations {
			command, err := pluginTransactionCommand(operation, scope.Package)
			if err != nil {
				return protocol.Output{}, nil, err
			}
			nested := request
			nested.Command = command
			info, err := ReadPluginVersion(ctx, db, scope.Package)
			if err != nil {
				return protocol.Output{}, nil, err
			}
			info.Summary.Enabled = true
			if err := CheckPluginRequest(ctx, db, nested, info); err != nil {
				return protocol.Output{}, nil, err
			}
			var output protocol.Output
			var changes []protocol.Event
			if SupportsAgentCommand(command.Kind) {
				output, changes, err = ExecuteAgentCommand(ctx, db, node, profile, caller, nested, time.Now())
			} else if command.Kind == "continue_turn" {
				output, changes, err = ExecuteBackgroundCommand(ctx, db, node, caller, nested, time.Now())
			} else if command.Kind == "dispatch" {
				input := command.Data.(*protocol.CommandDispatchPayload)
				output, changes, err = ExecuteDispatchAdmitted(ctx, db, node, input.Package, input.Action, time.Now().UnixMilli())
			} else if command.Kind == "publish_notification" {
				output, changes, err = notificationCommand(ctx, db, command)
			} else {
				output, changes, err = ExecutePluginStorageCommand(ctx, db, node, profile, caller, nested)
			}
			if err != nil {
				return protocol.Output{}, nil, err
			}
			outputs, events = append(outputs, output), append(events, changes...)
			if output.Kind == "queued_turn" {
				var turn protocol.QueuedTurn
				if err := decodeProviderData(output.Data, &turn); err != nil {
					return protocol.Output{}, nil, err
				}
				admitted = &turn
			}
		}
		if admitted != nil {
			if _, err := db.ExecContext(ctx, "UPDATE plugin_conversation_history SET turn=?3 WHERE session=?1 AND record>?2", admitted.Session.String(), checkpoint, admitted.ID.String()); err != nil {
				return protocol.Output{}, nil, err
			}
		}
		return protocol.Output{Kind: "plugin_transaction", Data: outputs}, events, nil
	case *protocol.CommandStopDispatchTurnPayload:
		turn, err := ReadDispatchTurn(ctx, db, node, input.Package.Name, input.Job)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		stopping := request
		stopping.Command = protocol.Command{Kind: "stop_turn", Data: &protocol.CommandStopTurnPayload{Turn: turn}}
		return ExecuteAgentCommand(ctx, db, node, profile, caller, stopping, time.Now())
	default:
		return protocol.Output{}, nil, fault(protocol.ErrorCodeNotConfigured, "plugin command is unavailable")
	}
	return protocol.Output{Kind: kind, Data: value}, []protocol.Event{{Kind: "plugin_values_changed", Data: protocol.EventPluginValuesChangedPayload{Name: scope.Package.Name}}}, err
}

func pluginTransactionCommand(operation protocol.PluginTransactionOperation, reference protocol.PluginReference) (protocol.Command, error) {
	body, err := json.Marshal(operation.Data)
	if err != nil {
		return protocol.Command{}, invalid("plugin transaction operation is invalid")
	}
	kind := ""
	switch operation.Kind {
	case "dispatch":
		var action protocol.DispatchCommand
		if err := json.Unmarshal(body, &action); err != nil {
			return protocol.Command{}, invalid("plugin dispatch transaction is invalid")
		}
		return protocol.Command{Kind: "dispatch", Data: &protocol.CommandDispatchPayload{Package: reference, Action: action}}, nil
	case "notify":
		var content protocol.NotificationDraft
		if err := json.Unmarshal(body, &content); err != nil {
			return protocol.Command{}, invalid("plugin notification transaction is invalid")
		}
		return protocol.Command{Kind: "publish_notification", Data: &protocol.CommandPublishNotificationPayload{Package: reference, Content: content}}, nil
	case "write":
		kind = "write_plugin_value"
	case "index":
		kind = "write_indexed_plugin_value"
	case "remove":
		kind = "remove_plugin_value"
	case "conversation_write":
		kind = "write_plugin_conversation_value"
	case "conversation_remove":
		kind = "remove_plugin_conversation_value"
	case "queue_pause":
		kind = "set_queue_paused"
	case "submit":
		kind = "submit_turn"
	case "continue":
		kind = "continue_turn"
	case "stop":
		kind = "stop_turn"
	default:
		return protocol.Command{}, invalid("plugin transaction operation is unavailable")
	}
	encoded, err := json.Marshal(struct {
		Kind string          `json:"kind"`
		Data json.RawMessage `json:"data"`
	}{kind, body})
	if err != nil {
		return protocol.Command{}, err
	}
	var command protocol.Command
	err = json.Unmarshal(encoded, &command)
	return command, err
}
