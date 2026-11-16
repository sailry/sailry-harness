package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDispatchRetainsExecutionNodeAndQueuedCallback(t *testing.T) {
	t.Parallel()
	fixture := newDispatchFixture(t, "execution")
	fixture.write(t, func(ctx context.Context, db *sql.Conn) error {
		schedule, err := saveDispatchSchedule(ctx, db, fixture.reference.Name, protocol.DispatchSchedule{
			ID: protocol.NewScheduleID(), Enabled: true, Topic: fixture.handler.Source.Topic,
			Payload: json.RawMessage("null"),
			Timing:  protocol.DispatchTiming{Kind: "once", Data: protocol.DispatchTimingOncePayload{AtMs: 1000}},
		}, 0)
		if err != nil {
			return err
		}
		if fixture.handler.ExecutionNode == nil || *fixture.handler.ExecutionNode != fixture.node ||
			schedule.ExecutionNode == nil || *schedule.ExecutionNode != fixture.node {
			return fmt.Errorf("dispatch configuration lost its execution Node")
		}
		job, err := enqueueDispatch(ctx, db, &fixture.reference, fixture.handler, fixture.event(), 1, false)
		if err != nil {
			return err
		}
		if job.ExecutionNode == nil || *job.ExecutionNode != fixture.node {
			return fmt.Errorf("queue lost its execution Node")
		}
		// Editing the source handler affects future enqueues. The
		// admitted job keeps its original callback and Node.
		handler := fixture.handler
		replacement := protocol.Command{Kind: "read_plugin_value", Data: protocol.CommandReadPluginValuePayload{Key: "new-handler"}}
		handler.Callback.Command = &replacement
		if _, err := saveDispatchHandler(ctx, db, fixture.reference, handler); err != nil {
			return err
		}
		batch, _, err := DispatchTick(ctx, db, fixture.node, 2, false)
		if err != nil {
			return err
		}
		if len(batch.Work) != 1 || batch.Work[0].Request.Target != fixture.node {
			return fmt.Errorf("queued callback moved to another Node: %+v", batch.Work)
		}
		input, ok := dispatchValue[protocol.CommandReadPluginValuePayload](batch.Work[0].Request.Command.Data)
		if !ok || input.Key != "fixture" {
			return fmt.Errorf("queued callback changed with its handler")
		}
		if _, err := dispatchRequest(protocol.NodeID{99}, job, fixture.handler.Callback, &fixture.reference); !providerFixtureFault(err, protocol.ErrorCodeWrongTarget) {
			return fmt.Errorf("queued callback accepted another execution Node: %v", err)
		}
		return nil
	})
}

func TestDispatchConfigurationRejectsAnotherExecutionNode(t *testing.T) {
	t.Parallel()
	fixture := newDispatchFixture(t, "node-bound")
	other := protocol.NodeID{99}
	for _, kind := range []string{"handler", "schedule"} {
		t.Run(kind, func(t *testing.T) {
			fixture.write(t, func(ctx context.Context, db *sql.Conn) error {
				var err error
				if kind == "handler" {
					handler := fixture.handler
					handler.ExecutionNode = &other
					_, err = saveDispatchHandler(ctx, db, fixture.reference, handler)
				} else {
					_, err = saveDispatchSchedule(ctx, db, fixture.reference.Name, protocol.DispatchSchedule{
						ExecutionNode: &other, ID: protocol.NewScheduleID(), Topic: "node-bound", Payload: json.RawMessage("null"),
						Timing: protocol.DispatchTiming{Kind: "once", Data: protocol.DispatchTimingOncePayload{AtMs: 1000}},
					}, 0)
				}
				if !providerFixtureFault(err, protocol.ErrorCodeWrongTarget) {
					return fmt.Errorf("%s accepted another execution Node: %v", kind, err)
				}
				return nil
			})
		})
	}
}

func TestLegacyDispatchRecordsUseStorageNode(t *testing.T) {
	t.Parallel()
	fixture := newDispatchFixture(t, "legacy-node")
	fixture.write(t, func(ctx context.Context, db *sql.Conn) error {
		job, err := enqueueDispatch(ctx, db, &fixture.reference, fixture.handler, fixture.event(), 1, false)
		if err != nil {
			return err
		}
		schedule, err := fixture.weekly(ctx, db, 1)
		if err != nil {
			return err
		}
		for _, table := range []string{"dispatch_user_handlers", "dispatch_user_schedules", "dispatch_jobs"} {
			if _, err := db.ExecContext(ctx, "UPDATE "+table+" SET body=json_remove(body,'$.execution_node')"); err != nil {
				return err
			}
		}
		handlers, err := dispatchList[protocol.DispatchHandler](ctx, db, "dispatch_user_handlers", fixture.reference.Name)
		if err != nil || len(handlers) != 1 {
			return fmt.Errorf("legacy handlers are unavailable: %v", err)
		}
		current, err := dispatchRequired[protocol.DispatchSchedule](ctx, db, "dispatch_user_schedules", fixture.reference.Name, "id", schedule.ID.String())
		if err != nil {
			return err
		}
		page, err := listDispatchJobs(ctx, db, fixture.reference.Name, nil, 10)
		if err != nil || len(page.Jobs) != 1 {
			return fmt.Errorf("legacy queue is unavailable: %v", err)
		}
		invocation, _, _, err := readDispatchInvocation(ctx, db, job.ID.String())
		if err != nil {
			return err
		}
		for _, node := range []*protocol.NodeID{handlers[0].ExecutionNode, current.ExecutionNode, page.Jobs[0].ExecutionNode, invocation.ExecutionNode} {
			if node == nil || *node != fixture.node {
				return fmt.Errorf("legacy dispatch did not inherit its storage Node")
			}
		}
		return nil
	})
}
