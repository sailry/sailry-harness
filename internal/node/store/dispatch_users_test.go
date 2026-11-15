package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDispatchUserNamespaces(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		first, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		second, err := CreateUser(ctx, db, "Scheduled user")
		if err != nil {
			return err
		}
		users := []protocol.User{first, second}
		contexts := []context.Context{WithUser(ctx, first), WithUser(ctx, second)}
		references := make([]protocol.PluginReference, 2)
		scheduleID := protocol.NewScheduleID()
		for index, owner := range contexts {
			installed, err := InstallPlugin(owner, db, pluginTestInfo("dispatch-isolation", 'a'), 0, true)
			if err != nil {
				return err
			}
			references[index] = plugins.PackageReference(installed)
			handler := protocol.DispatchHandler{Name: "same-handler", Enabled: true, Queue: "same-queue", Source: protocol.DispatchSource{Package: references[index].Name, Topic: "same-topic"}, Callback: protocol.DispatchCallback{Completion: protocol.DispatchCompletionCommand, Bindings: map[string]string{}, Command: &protocol.Command{Kind: "read_plugin_value", Data: &protocol.CommandReadPluginValuePayload{Key: "record"}}}}
			if _, _, err := ExecuteDispatch(owner, db, node, references[index], protocol.DispatchCommand{Kind: "save_handler", Data: handler}, 100); err != nil {
				return err
			}
			schedule := protocol.DispatchSchedule{ID: scheduleID, Enabled: true, Topic: "same-topic", Payload: json.RawMessage("null"), Timing: protocol.DispatchTiming{Kind: "once", Data: protocol.DispatchTimingOncePayload{AtMs: 1000}}}
			if _, _, err := ExecuteDispatch(owner, db, node, references[index], protocol.DispatchCommand{Kind: "save_schedule", Data: schedule}, 100); err != nil {
				return err
			}
		}
		firstBatch, _, err := DispatchTick(contexts[0], db, node, 1000, false)
		if err != nil {
			return err
		}
		if len(firstBatch.Work) != 1 || firstBatch.Work[0].User.ID != first.ID || firstBatch.Work[0].StorageUser != first.ID {
			return fmt.Errorf("first schedule lost its owner: %#v", firstBatch)
		}
		other, err := dispatchRequired[protocol.DispatchSchedule](contexts[1], db, "dispatch_user_schedules", references[1].Name, "id", scheduleID.String())
		if err != nil || other.NextMs == nil || *other.NextMs != 1000 {
			return fmt.Errorf("first schedule consumed another user's occurrence: %v", err)
		}
		secondBatch, _, err := DispatchTick(contexts[1], db, node, 1000, false)
		if err != nil {
			return err
		}
		if len(secondBatch.Work) != 1 || secondBatch.Work[0].User.ID != second.ID || secondBatch.Work[0].StorageUser != second.ID {
			return fmt.Errorf("second schedule lost its owner: %#v", secondBatch)
		}
		if _, _, err := ExecuteDispatch(contexts[1], db, node, references[1], protocol.DispatchCommand{Kind: "read_job", Data: protocol.DispatchCommandReadJobPayload{ID: firstBatch.Work[0].Job}}, 1000); err == nil {
			return fmt.Errorf("dispatch job crossed user boundary")
		}
		for index, owner := range contexts {
			page, err := listDispatchJobs(owner, db, references[index].Name, nil, 10)
			if err != nil {
				return err
			}
			if len(page.Jobs) != 1 {
				return fmt.Errorf("user %s saw %d jobs", users[index].ID.String(), len(page.Jobs))
			}
		}
		// Finishing uses the durable owner rather than a runner's maintenance context.
		if _, err := FinishDispatch(ctx, db, secondBatch.Work[0].Job, nil, 1100); err != nil {
			return err
		}
		finished, err := dispatchRequired[protocol.DispatchJob](contexts[1], db, "dispatch_jobs", references[1].Name, "id", secondBatch.Work[0].Job.String())
		if err != nil || finished.Status != protocol.DispatchStatusCompleted {
			return fmt.Errorf("scheduled finish lost its principal: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
