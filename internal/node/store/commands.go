package store

import (
	"context"
	"database/sql"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func ExecuteCommand(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, profile string, request protocol.Request, prepared PreparedPaths) (protocol.Output, []protocol.Event, error) {
	switch request.Command.Kind {
	case "create_project":
		user, err := CurrentUser(ctx, db)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		return createProject(ctx, db, user.ID, *request.Command.Data.(*protocol.ProjectsDraft), prepared)
	case "register_project":
		user, err := CurrentUser(ctx, db)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		input := request.Command.Data.(*protocol.CommandRegisterProjectPayload)
		return createProject(ctx, db, user.ID, protocol.ProjectsDraft{Name: input.Name, Path: input.Path, Appearance: protocol.DefaultProjectsAppearance(), Visibility: input.Visibility}, prepared)
	case "update_project":
		return updateProject(ctx, db, *request.Command.Data.(*protocol.CommandUpdateProjectPayload), prepared)
	case "remove_project":
		return removeProject(ctx, db, request.Command.Data.(*protocol.CommandRemoveProjectPayload).Expected, prepared.TerminalsClosed)
	case "create_session":
		return createSession(ctx, db, node, profile, *request.Command.Data.(*protocol.CommandCreateSessionPayload))
	case "import_session":
		return importSession(ctx, db, node, caller, profile, *request.Command.Data.(*protocol.SessionImport))
	case "save_database":
		if err := RecordConnectionSave(ctx, db, request); err != nil {
			return protocol.Output{}, nil, err
		}
		return ExecuteDatabaseCommand(ctx, db, request.Command)
	case "remove_database":
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
	case "report_diagnostic":
		return reportDiagnostic(ctx, db, request, time.Now().UnixMilli())
	case "save_media_settings":
		return saveMediaSettings(ctx, db, *request.Command.Data.(*protocol.MediaSettings))
	case "save_computer_settings":
		value, events, err := SaveComputerSettings(ctx, db, *request.Command.Data.(*protocol.ComputerSettings))
		return protocol.Output{Kind: "computer_settings", Data: value}, events, err
	case "save_external_browser_settings":
		value, events, err := SaveExternalBrowserSettings(ctx, db, *request.Command.Data.(*protocol.ExternalBrowserSettings))
		return protocol.Output{Kind: "external_browser_settings", Data: value}, events, err
	case "save_power_settings":
		return savePower(ctx, db, *request.Command.Data.(*protocol.PowerSettings))
	case "save_telemetry_settings":
		return saveTelemetry(ctx, db, *request.Command.Data.(*protocol.TelemetryUpdate))
	case "reset_telemetry_identity":
		return resetTelemetry(ctx, db, request)
	default:
		return protocol.Output{}, nil, fault(protocol.ErrorCodeNotConfigured, "command is unavailable")
	}
}
