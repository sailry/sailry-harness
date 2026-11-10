package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func CheckPluginPlanAction(ctx context.Context, db *sql.Conn, request protocol.Request, action protocol.PluginAction) error {
	if request.Plugin == nil || request.Plugin.Turn == nil {
		return nil
	}
	capture, err := readTurnCapture(ctx, db, *request.Plugin.Turn)
	if err != nil {
		return err
	}
	if capture.Config.Mode != protocol.WorkModePlan {
		return nil
	}
	allowed := action.ReadOnly() || request.Command.Kind == "write_artifact" && action == protocol.PluginActionWriteArtifacts
	switch request.Command.Kind {
	case "read_installations", "dismiss_installation", "read_plugin_settings", "list_plugin_models", "guide_turn":
		allowed = true
	case "dispatch":
		input, valid := dispatchValue[protocol.CommandDispatchPayload](request.Command.Data)
		if valid {
			switch input.Action.Kind {
			case "list_handlers", "list_schedules", "list_jobs", "read_job", "read_result":
				allowed = true
			}
		}
	}
	writes := false
	switch request.Command.Kind {
	case "query_database":
		input, valid := dispatchValue[protocol.CommandQueryDatabasePayload](request.Command.Data)
		if !valid {
			return invalid("invalid database query")
		}
		writes = !input.ReadOnly
	case "use_browser":
		input, valid := dispatchValue[protocol.CommandUseBrowserPayload](request.Command.Data)
		if !valid {
			return invalid("invalid browser operation")
		}
		writes = pluginBrowserRequiresApproval(input.Action)
	}
	if !allowed || writes {
		message := "planning turns cannot dispatch plugin actions with side effects"
		if action == protocol.PluginActionWriteStorage {
			message = "planning turns cannot write plugin storage"
		}
		return fault(protocol.ErrorCodePermissionDenied, message)
	}
	return nil
}

func pluginBrowserRequiresApproval(action protocol.BrowserAction) bool {
	switch action.Kind {
	case "tabs", "read", "navigate", "scroll", "back", "forward", "refresh", "open", "focus", "frame", "wait":
		return false
	case "screenshot":
		input, valid := dispatchValue[protocol.BrowserActionScreenshotPayload](action.Data)
		return !valid || input.Save
	default:
		return true
	}
}
