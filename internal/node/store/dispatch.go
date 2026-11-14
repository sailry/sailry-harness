package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"sort"
	"strconv"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	dispatchMaxItems        = 256
	dispatchMaxPayloadBytes = 64 * 1024
	dispatchMaxPending      = 1024
	dispatchNativeOwner     = ""
)

func ExecuteDispatch(ctx context.Context, db *sql.Conn, node protocol.NodeID, reference protocol.PluginReference, action protocol.DispatchCommand, timestamp int64) (protocol.Output, []protocol.Event, error) {
	if err := dispatchAvailable(ctx, db, reference); err != nil {
		return protocol.Output{}, nil, err
	}
	return ExecuteDispatchAdmitted(ctx, db, node, reference, action, timestamp)
}

func ExecuteDispatchAdmitted(ctx context.Context, db *sql.Conn, node protocol.NodeID, reference protocol.PluginReference, action protocol.DispatchCommand, timestamp int64) (protocol.Output, []protocol.Event, error) {
	owners := []string{reference.Name}
	var output protocol.DispatchOutput
	var changed bool
	var err error
	switch action.Kind {
	case "list_handlers":
		output.Kind = "handlers"
		output.Data, err = dispatchList[protocol.DispatchHandler](ctx, db, "dispatch_handlers", reference.Name)
	case "save_handler":
		handler, ok := dispatchValue[protocol.DispatchHandler](action.Data)
		if !ok {
			return protocol.Output{}, nil, invalid("invalid dispatch handler")
		}
		output.Kind, changed = "handler", true
		output.Data, err = saveDispatchHandler(ctx, db, reference, handler)
	case "remove_handler":
		input, ok := dispatchValue[protocol.DispatchCommandRemoveHandlerPayload](action.Data)
		if !ok {
			return protocol.Output{}, nil, invalid("invalid dispatch handler removal")
		}
		handler, readErr := dispatchRequired[protocol.DispatchHandler](ctx, db, "dispatch_handlers", reference.Name, "name", input.Name)
		if readErr != nil {
			return protocol.Output{}, nil, readErr
		}
		if err = checkRevision(handler.Revision, input.ExpectedRevision); err != nil {
			return protocol.Output{}, nil, err
		}
		if input.CancelPending {
			if err = cancelDispatchPending(ctx, db, reference.Name, input.Name, timestamp); err != nil {
				return protocol.Output{}, nil, err
			}
		}
		_, err = db.ExecContext(ctx, "DELETE FROM dispatch_handlers WHERE package=?1 AND name=?2", reference.Name, input.Name)
		output.Kind, changed = "removed", true
	case "list_schedules":
		output.Kind = "schedules"
		output.Data, err = dispatchList[protocol.DispatchSchedule](ctx, db, "dispatch_schedules", reference.Name)
	case "save_schedule":
		schedule, ok := dispatchValue[protocol.DispatchSchedule](action.Data)
		if !ok {
			return protocol.Output{}, nil, invalid("invalid dispatch schedule")
		}
		output.Kind, changed = "schedule", true
		output.Data, err = saveDispatchSchedule(ctx, db, reference.Name, schedule, timestamp)
	case "remove_schedule":
		input, ok := dispatchValue[protocol.DispatchCommandRemoveSchedulePayload](action.Data)
		if !ok {
			return protocol.Output{}, nil, invalid("invalid dispatch schedule removal")
		}
		schedule, readErr := dispatchRequired[protocol.DispatchSchedule](ctx, db, "dispatch_schedules", reference.Name, "id", input.ID.String())
		if readErr != nil {
			return protocol.Output{}, nil, readErr
		}
		if err = checkRevision(schedule.Revision, input.ExpectedRevision); err != nil {
			return protocol.Output{}, nil, err
		}
		_, err = db.ExecContext(ctx, "DELETE FROM dispatch_schedules WHERE package=?1 AND id=?2", reference.Name, input.ID.String())
		output.Kind, changed = "removed", true
	case "publish":
		input, ok := dispatchValue[protocol.DispatchCommandPublishPayload](action.Data)
		if !ok {
			return protocol.Output{}, nil, invalid("invalid dispatch publication")
		}
		if err = dispatchName(input.Topic); err != nil {
			return protocol.Output{}, nil, err
		}
		if err = dispatchData(input.Payload); err != nil {
			return protocol.Output{}, nil, err
		}
		event := protocol.DispatchEvent{ID: protocol.NewEventID(), Source: protocol.DispatchSource{Package: reference.Name, Topic: input.Topic}, Payload: input.Payload, TimestampMs: timestamp}
		var jobs []protocol.JobID
		jobs, owners, err = publishDispatch(ctx, db, event, timestamp)
		output = protocol.DispatchOutput{Kind: "published", Data: protocol.DispatchOutputPublishedPayload{Event: event.ID, Jobs: jobs}}
		changed = true
	case "enqueue":
		input, ok := dispatchValue[protocol.DispatchCommandEnqueuePayload](action.Data)
		if !ok {
			return protocol.Output{}, nil, invalid("invalid dispatch enqueue")
		}
		if err = dispatchData(input.Payload); err != nil {
			return protocol.Output{}, nil, err
		}
		handler, readErr := dispatchRequired[protocol.DispatchHandler](ctx, db, "dispatch_handlers", reference.Name, "name", input.Handler)
		if readErr != nil {
			return protocol.Output{}, nil, readErr
		}
		if !handler.Enabled {
			return protocol.Output{}, nil, invalid("event handler is disabled")
		}
		current, readErr := dispatchCurrent(ctx, db, reference.Name)
		if readErr != nil {
			return protocol.Output{}, nil, readErr
		}
		currentReference := dispatchReference(current.Summary)
		event := protocol.DispatchEvent{ID: protocol.NewEventID(), Source: protocol.DispatchSource{Package: reference.Name, Topic: handler.Source.Topic}, Payload: input.Payload, TimestampMs: timestamp}
		var sessionStart bool
		if err := db.QueryRowContext(ctx, "SELECT coalesce(json_extract(reference,'$.session_start'),0) FROM dispatch_handlers WHERE package=?1 AND name=?2", reference.Name, handler.Name).Scan(&sessionStart); err != nil {
			return protocol.Output{}, nil, err
		}
		output.Kind, changed = "job", true
		output.Data, err = enqueueDispatch(ctx, db, &currentReference, handler, event, timestamp, sessionStart)
	case "list_jobs":
		input, ok := dispatchValue[protocol.DispatchCommandListJobsPayload](action.Data)
		if !ok {
			return protocol.Output{}, nil, invalid("invalid dispatch page bounds")
		}
		output.Kind = "jobs"
		output.Data, err = listDispatchJobs(ctx, db, reference.Name, input.Before, input.Limit)
	case "read_job":
		input, ok := dispatchValue[protocol.DispatchCommandReadJobPayload](action.Data)
		if !ok {
			return protocol.Output{}, nil, invalid("invalid dispatch job")
		}
		output.Kind = "job"
		output.Data, err = dispatchRequired[protocol.DispatchJob](ctx, db, "dispatch_jobs", reference.Name, "id", input.ID.String())
	case "read_result":
		input, ok := dispatchValue[protocol.DispatchCommandReadResultPayload](action.Data)
		if !ok {
			return protocol.Output{}, nil, invalid("invalid dispatch job result")
		}
		output.Kind = "result"
		output.Data, err = dispatchResult(ctx, db, node, reference.Name, input.ID)
	case "cancel_job":
		input, ok := dispatchValue[protocol.DispatchCommandCancelJobPayload](action.Data)
		if !ok {
			return protocol.Output{}, nil, invalid("invalid dispatch job cancellation")
		}
		output.Kind, changed = "job", true
		output.Data, err = cancelDispatchJob(ctx, db, reference.Name, input.ID, timestamp)
	default:
		return protocol.Output{}, nil, invalid("unknown dispatch command")
	}
	if err != nil {
		return protocol.Output{}, nil, err
	}
	var events []protocol.Event
	if changed {
		events = []protocol.Event{dispatchChanged(owners)}
	}
	return protocol.Output{Kind: "dispatch", Data: output}, events, nil
}

func DispatchSettingsChanged(ctx context.Context, db *sql.Conn, reference protocol.PluginReference, timestamp int64) ([]protocol.Event, error) {
	body, err := encode(reference)
	if err != nil {
		return nil, err
	}
	var object map[string]any
	if err = json.Unmarshal(body, &object); err != nil {
		return nil, err
	}
	revision := strconv.FormatUint(reference.SettingsRevision, 10)
	object["settings_revision"] = revision
	payload, err := encode(map[string]any{"package": object, "settings_revision": revision})
	if err != nil {
		return nil, err
	}
	event := protocol.DispatchEvent{ID: protocol.NewEventID(), Source: protocol.DispatchSource{Package: reference.Name, Topic: "settings.changed"}, Payload: payload, TimestampMs: timestamp}
	_, owners, err := publishDispatch(ctx, db, event, timestamp)
	if err != nil {
		return nil, err
	}
	return []protocol.Event{dispatchChanged(owners)}, nil
}

func dispatchName(value string) error {
	if len(value) == 0 || len(value) > 128 {
		return invalid("dispatch names must use letters, digits, dots, underscores or hyphens")
	}
	for _, character := range []byte(value) {
		if character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' ||
			character >= '0' && character <= '9' || character == '.' || character == '_' || character == '-' {
			continue
		}
		return invalid("dispatch names must use letters, digits, dots, underscores or hyphens")
	}
	return nil
}

func dispatchData(value any) error {
	body, err := encode(value)
	if err != nil {
		return err
	}
	if len(body) > dispatchMaxPayloadBytes {
		return invalid("dispatch payload exceeds the size limit")
	}
	return nil
}

func dispatchCurrent(ctx context.Context, db *sql.Conn, name string) (protocol.PluginInfo, error) {
	info, err := ReadPlugin(ctx, db, name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	if info == nil {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeNotFound, "dispatch plugin is not installed")
	}
	if info.Summary.Compatibility.Kind != "compatible" {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeNotConfigured, "dispatch plugin is incompatible")
	}
	if !info.Summary.Enabled {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeNotConfigured, "dispatch plugin is disabled")
	}
	return *info, nil
}

func dispatchAvailable(ctx context.Context, db *sql.Conn, reference protocol.PluginReference) error {
	info, err := dispatchCurrent(ctx, db, reference.Name)
	if err != nil {
		return err
	}
	if dispatchReference(info.Summary) != reference {
		return fault(protocol.ErrorCodeRevisionConflict, "dispatch plugin changed")
	}
	return nil
}

func dispatchReference(summary protocol.PluginSummary) protocol.PluginReference {
	return protocol.PluginReference{Name: summary.Name, Digest: summary.Digest, SettingsRevision: summary.SettingsRevision}
}

func dispatchCapacity(ctx context.Context, db *sql.Conn, table, packageName string) error {
	var count int
	if err := db.QueryRowContext(ctx, "SELECT count(*) FROM "+table+" WHERE package=?1", packageName).Scan(&count); err != nil {
		return err
	}
	if count >= dispatchMaxItems {
		return fault(protocol.ErrorCodeBusy, "dispatch configuration capacity exhausted")
	}
	return nil
}

func dispatchBudget(ctx context.Context, db *sql.Conn, table, packageName, key, identity string, value any) error {
	var size int
	if err := db.QueryRowContext(ctx, "SELECT coalesce(sum(length(body)),0) FROM "+table+" WHERE package=?1 AND "+key+"!=?2", packageName, identity).Scan(&size); err != nil {
		return err
	}
	body, err := encode(value)
	if err != nil {
		return err
	}
	if size+len(body) > 4*1024*1024 {
		return fault(protocol.ErrorCodeBusy, "dispatch namespace capacity exhausted")
	}
	return nil
}

func dispatchRequired[Value any](ctx context.Context, db *sql.Conn, table, packageName, key, identity string) (Value, error) {
	value, err := readBody[Value](ctx, db, "SELECT body FROM "+table+" WHERE package=?1 AND "+key+"=?2", packageName, identity)
	if err != nil {
		var empty Value
		return empty, err
	}
	if value == nil {
		var empty Value
		return empty, fault(protocol.ErrorCodeNotFound, "dispatch resource is unavailable")
	}
	return *value, nil
}

func dispatchList[Value any](ctx context.Context, db *sql.Conn, table, packageName string) ([]Value, error) {
	return readBodies[Value](ctx, db, "SELECT body FROM "+table+" WHERE package=?1 ORDER BY rowid", packageName)
}

func dispatchChanged(owners []string) protocol.Event {
	sort.Strings(owners)
	unique := []string{}
	for _, owner := range owners {
		if len(unique) == 0 || unique[len(unique)-1] != owner {
			unique = append(unique, owner)
		}
	}
	return protocol.Event{Kind: "dispatch_changed", Data: protocol.EventDispatchChangedPayload{Packages: unique}}
}

func dispatchValue[Value any](input any) (Value, bool) {
	switch value := input.(type) {
	case Value:
		return value, true
	case *Value:
		if value != nil {
			return *value, true
		}
	}
	var empty Value
	return empty, false
}

func dispatchFault(err error) *protocol.Fault {
	if err == nil {
		return nil
	}
	var failure *protocol.Fault
	if errors.As(err, &failure) {
		return failure
	}
	if errors.Is(err, context.Canceled) {
		return &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "callback was cancelled"}
	}
	return &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "callback execution failed"}
}
