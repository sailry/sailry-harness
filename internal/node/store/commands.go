package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func ExecuteCommand(ctx context.Context, db *sql.Conn, node protocol.NodeID, profile string, request protocol.Request, prepared PreparedPaths) (protocol.Output, []protocol.Event, error) {
	switch request.Command.Kind {
	case "create_project":
		return createProject(ctx, db, *request.Command.Data.(*protocol.ProjectsDraft), prepared)
	case "update_project":
		return updateProject(ctx, db, *request.Command.Data.(*protocol.CommandUpdateProjectPayload), prepared)
	case "remove_project":
		return removeProject(ctx, db, request.Command.Data.(*protocol.CommandRemoveProjectPayload).Expected)
	case "register_worktree":
		return registerWorktree(ctx, db, *request.Command.Data.(*protocol.CommandRegisterWorktreePayload), prepared)
	case "set_defaults":
		return setDefaults(ctx, db, node, *request.Command.Data.(*protocol.CommandSetDefaultsPayload))
	case "create_session":
		return createSession(ctx, db, node, profile, *request.Command.Data.(*protocol.CommandCreateSessionPayload))
	case "bind_connection_session":
		return bindConnectionSession(ctx, db, profile, *request.Command.Data.(*protocol.CommandBindConnectionSessionPayload))
	case "save_database", "remove_database":
		return ExecuteDatabaseCommand(ctx, db, request.Command)
	case "rename_session":
		return renameSession(ctx, db, *request.Command.Data.(*protocol.CommandRenameSessionPayload))
	case "set_session_archived":
		return archiveSession(ctx, db, *request.Command.Data.(*protocol.CommandSetSessionArchivedPayload))
	case "set_session_read":
		return setSessionRead(ctx, db, *request.Command.Data.(*protocol.CommandSetSessionReadPayload))
	case "archive_sessions":
		return batchSessions(ctx, db, request.Command.Data.(*protocol.CommandArchiveSessionsPayload).Expected, false)
	case "remove_sessions":
		return batchSessions(ctx, db, request.Command.Data.(*protocol.CommandRemoveSessionsPayload).Expected, true)
	case "remove_session":
		input := request.Command.Data.(*protocol.CommandRemoveSessionPayload)
		return removeSession(ctx, db, input.Session, input.ExpectedRevision)
	case "set_session_order":
		return reorderSessions(ctx, db, *request.Command.Data.(*protocol.CommandSetSessionOrderPayload))
	case "publish_notification", "mark_notification_read", "mark_notifications_read", "dismiss_read_notifications", "dismiss_notifications":
		return notificationCommand(ctx, db, request.Command)
	case "save_power_settings":
		return savePower(ctx, db, *request.Command.Data.(*protocol.PowerSettings))
	case "save_telemetry_settings":
		return saveTelemetry(ctx, db, *request.Command.Data.(*protocol.TelemetryUpdate))
	default:
		return protocol.Output{}, nil, fault(protocol.ErrorCodeNotConfigured, "command is unavailable")
	}
}
