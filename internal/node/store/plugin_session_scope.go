package store

import "github.com/sailry/sailry-harness/internal/protocol"

// Package data and unlinked notices outlive the conversation that scheduled a
// callback. The invocation retains its resource bounds; commands issued inside
// it and every transaction operation are authorized separately.
func pluginSessionIndependent(command protocol.Command) bool {
	switch command.Kind {
	case "call_plugin", "read_plugin_value", "list_plugin_keys", "search_plugin_values",
		"write_plugin_value", "write_indexed_plugin_value", "remove_plugin_value", "plugin_transaction":
		return true
	case "publish_notification":
		input, ok := dispatchValue[protocol.CommandPublishNotificationPayload](command.Data)
		return ok && input.Content.Session == nil
	default:
		return false
	}
}
