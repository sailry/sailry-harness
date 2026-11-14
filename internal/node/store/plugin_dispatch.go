package store

import (
	"context"
	"database/sql"
	"reflect"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Explicit triggers inherit the caller's bounds, even when the handler was
// originally registered by a wider controller scope. Automatic occurrences use
// their separately captured authorization.
func checkPluginDispatchTargets(ctx context.Context, db *sql.Conn, scope protocol.PluginContext, action protocol.DispatchCommand) error {
	if scope.Worktree == nil && scope.Session == nil {
		return nil
	}
	var handlers []protocol.DispatchHandler
	var topic string
	switch action.Kind {
	case "enqueue":
		input, valid := dispatchValue[protocol.DispatchCommandEnqueuePayload](action.Data)
		if !valid {
			return invalid("invalid dispatch enqueue")
		}
		handler, err := dispatchRequired[protocol.DispatchHandler](ctx, db, "dispatch_handlers", scope.Package.Name, "name", input.Handler)
		if err != nil {
			return err
		}
		handlers = append(handlers, handler)
	case "publish":
		input, valid := dispatchValue[protocol.DispatchCommandPublishPayload](action.Data)
		if !valid {
			return invalid("invalid dispatch publication")
		}
		topic = input.Topic
	case "save_schedule":
		input, valid := dispatchValue[protocol.DispatchSchedule](action.Data)
		if !valid {
			return invalid("invalid dispatch schedule")
		}
		topic = input.Topic
	default:
		return nil
	}
	if action.Kind != "enqueue" {
		var err error
		// Cross-package subscriptions are declared by their receiver. This
		// check confines explicit use of the caller package's own handlers.
		handlers, err = readBodies[protocol.DispatchHandler](ctx, db, "SELECT body FROM dispatch_handlers WHERE package=?1 AND source=?1 AND topic=?2 ORDER BY name", scope.Package.Name, topic)
		if err != nil {
			return err
		}
		if action.Kind == "save_schedule" && len(handlers) == 0 {
			return fault(protocol.ErrorCodePermissionDenied, "scoped schedules require an authorized handler")
		}
	}
	for _, handler := range handlers {
		if action.Kind == "publish" && !handler.Enabled {
			continue
		}
		target := handler.Callback.Scope
		if scope.Worktree != nil && !reflect.DeepEqual(target.Worktree, scope.Worktree) || scope.Session != nil && !reflect.DeepEqual(target.Session, scope.Session) {
			return fault(protocol.ErrorCodePermissionDenied, "dispatch target is outside the captured resource scope")
		}
		if scope.Session != nil && handler.Callback.Command != nil && handler.Callback.Command.Kind == "start_session" {
			if err := checkPluginScheduledStart(ctx, db, scope, handler.Callback); err != nil {
				return err
			}
		}
	}
	return nil
}
