package node

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"os"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/link"
	dispatcher "github.com/sailry/sailry-harness/internal/node/dispatch"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type dispatchControls struct {
	clock clockwork.Clock
	wake  chan struct{}
}

func (node *Node) initDispatch(clock clockwork.Clock) {
	if clock == nil {
		clock = clockwork.NewRealClock()
	}
	node.dispatch = &dispatchControls{clock: clock, wake: make(chan struct{}, 1)}
	runner := dispatcher.Runner{
		Clock: clock, Wake: node.dispatch.wake, Tick: node.dispatchTick,
		Execute: node.executeDispatchCallback, Finished: node.finishDispatch,
		Report: func(err error) { _, _ = fmt.Fprintln(os.Stderr, "dispatch service failed: "+err.Error()) },
	}
	node.work.Add(1)
	go func() {
		defer node.work.Done()
		runner.Serve(node.ctx)
	}()
}

func (node *Node) wakeDispatch() {
	if node.dispatch == nil {
		return
	}
	select {
	case node.dispatch.wake <- struct{}{}:
	default:
	}
}

// CheckSchedules makes the runner read the wall clock now. The application
// calls it when the system wakes; without that signal the runner's own
// recheck finds a fire that became due during sleep.
func (node *Node) CheckSchedules() { node.wakeDispatch() }

func (node *Node) dispatchTick(ctx context.Context, recovering bool) (dispatcher.Batch, error) {
	batch := dispatcher.Batch{Work: []dispatcher.Work{}}
	changed := false
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		users, err := store.ListUsers(ctx, db)
		if err != nil {
			return err
		}
		for _, user := range users {
			if user.State != "active" {
				continue
			}
			owner := store.WithUser(ctx, user)
			current, events, err := store.DispatchTick(owner, db, node.ID(), node.dispatch.clock.Now().UnixMilli(), recovering)
			if err != nil {
				return err
			}
			if _, err := store.RecordEvents(owner, db, node.ID(), events); err != nil {
				return err
			}
			changed = changed || len(events) != 0
			batch.Work = append(batch.Work, current.Work...)
			batch.Pending = batch.Pending || current.Pending
			if current.NextMs != nil && (batch.NextMs == nil || *current.NextMs < *batch.NextMs) {
				next := *current.NextMs
				batch.NextMs = &next
			}
		}
		return nil
	})
	if err == nil && changed {
		node.publish()
	}
	return batch, err
}

func (node *Node) finishDispatch(ctx context.Context, job protocol.JobID, result error) error {
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		owner, err := store.DispatchJobContext(ctx, db, job)
		if errors.Is(err, sql.ErrNoRows) {
			return nil
		}
		if err != nil {
			return err
		}
		ctx = owner
		changes, err := store.FinishDispatch(ctx, db, job, result, node.dispatch.clock.Now().UnixMilli())
		if err != nil {
			return err
		}
		_, err = store.RecordEvents(ctx, db, node.ID(), changes)
		return err
	})
	if err == nil {
		node.publish()
		node.wakeDispatch()
	}
	return err
}

func (node *Node) executeDispatchCallback(ctx context.Context, work dispatcher.Work) error {
	if work.User.ID == (protocol.UserID{}) || work.StorageUser == (protocol.UserID{}) {
		return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "scheduled operation has no user capture"}
	}
	ctx = store.WithCapturedUser(store.WithUser(ctx, work.User), work.StorageUser)
	output, err := node.dispatchCallbackResult(ctx, work.Request)
	if err != nil || work.Completion == protocol.DispatchCompletionCommand {
		return err
	}
	turn, ok := dispatchPayload[protocol.QueuedTurn](output.Data)
	if output.Kind != "queued_turn" || !ok {
		return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "turn callback did not admit a turn"}
	}
	return node.waitDispatchTurn(ctx, turn)
}

func (node *Node) dispatchCallbackResult(ctx context.Context, request protocol.Request) (protocol.Output, error) {
	accepted, err := node.Dispatch(ctx, node.ID(), request)
	if err != nil {
		return protocol.Output{}, err
	}
	select {
	case response, open := <-accepted.Completion:
		if !open {
			return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "callback result was not received"}
		}
		if response.Fault != nil {
			return protocol.Output{}, response.Fault
		}
		return response.Output, nil
	case <-ctx.Done():
		return protocol.Output{}, ctx.Err()
	}
}

func (node *Node) waitDispatchTurn(ctx context.Context, turn protocol.QueuedTurn) error {
	for ctx.Err() == nil {
		stream, err := node.subscribeConversation(ctx, turn.Session)
		if err != nil {
			return err
		}
		finished, result := node.observeDispatchTurn(ctx, stream, turn)
		_ = stream.Close()
		if finished {
			return result
		}
	}
	return ctx.Err()
}

func (node *Node) observeDispatchTurn(ctx context.Context, stream link.Subscription, turn protocol.QueuedTurn) (bool, error) {
	for {
		update, err := stream.Next(ctx)
		if err != nil {
			return true, err
		}
		var run *protocol.ConversationRun
		switch update.Kind {
		case "conversation_snapshot":
			snapshot, ok := dispatchPayload[protocol.ConversationSnapshot](update.Data)
			if !ok {
				return true, &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "unexpected task state"}
			}
			for _, current := range snapshot.Page.Runs {
				if current.Turn == turn.ID {
					run = &current
					break
				}
			}
			if run == nil {
				request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "read_turn", Data: protocol.CommandReadTurnPayload{
					Session: turn.Session, Turn: turn.ID, ExpectedRevision: snapshot.Page.Revision, Limit: 1,
				}})
				output, err := node.dispatchCallbackResult(ctx, request)
				if err != nil {
					var failure *protocol.Fault
					if errors.As(err, &failure) && failure.Code == protocol.ErrorCodeRevisionConflict {
						return false, nil
					}
					return true, err
				}
				history, ok := dispatchPayload[protocol.ConversationTurnHistory](output.Data)
				if output.Kind != "turn_history" || !ok {
					return true, &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "unexpected task state"}
				}
				run = &history.Run
			}
		case "conversation_frame":
			frame, ok := dispatchPayload[protocol.ConversationFrame](update.Data)
			if ok && frame.Change.Kind == "run" {
				current, ok := dispatchPayload[protocol.ConversationRun](frame.Change.Data)
				if ok && current.Turn == turn.ID {
					run = &current
				}
			}
		case "reset_required":
			return false, nil
		}
		if run != nil {
			if finished, result := dispatcher.TurnResult(*run); finished {
				return true, result
			}
		}
	}
}

func dispatchPayload[Value any](input any) (Value, bool) {
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
