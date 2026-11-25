package node

import (
	"context"
	"database/sql"
	"errors"
	"slices"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func sessionRemoval(kind string) bool {
	return kind == "remove_session" || kind == "remove_sessions"
}

// stopRemovedSessions stops the running turns of a session removal and waits
// for every turn it covers to settle, so the removal records only finished
// history. Stops use the stop_turn path and stay committed if the removal then
// fails. A stale or invalid removal stops nothing; its fault is the result.
// Queued input is left for the removing transaction to discard.
func (node *Node) stopRemovedSessions(ctx context.Context, command protocol.Command) error {
	for {
		// Take the signal before reading so a settlement in between still wakes us.
		settled := node.agent.ActivitySignal()
		var active []protocol.ConversationRun
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			active, err = store.ActiveSessionRemoval(ctx, db, command)
			return err
		})
		if err != nil || len(active) == 0 {
			return err
		}
		// A stopping turn only needs to settle; writing again would stop nothing.
		if slices.ContainsFunc(active, func(run protocol.ConversationRun) bool { return run.Status == protocol.ConversationStatusRunning }) {
			if err = node.stopSessionRuns(ctx, command); err != nil {
				return err
			}
		}
		select {
		case <-settled:
		case <-ctx.Done():
			return unavailable()
		}
	}
}

// executeSessionRemoval records a removal whose turns stopRemovedSessions has
// settled. The scheduler can claim queued input of the removal after that pass;
// the removing transaction then stores nothing, and the newly started turns are
// stopped and awaited before the removal is recorded again.
func (node *Node) executeSessionRemoval(ctx context.Context, job commandJob, prepared store.PreparedPaths, prepareErr error) (link.Response, error) {
	for {
		result, err := node.execute(job, prepared, prepareErr)
		if !errors.Is(err, store.ErrSessionRemovalActive) {
			return result, err
		}
		prepareErr = node.stopRemovedSessions(ctx, job.request.Command)
	}
}

// stopSessionRuns commits the stops under publication and delivers them to
// their owners before another commit can be published.
func (node *Node) stopSessionRuns(ctx context.Context, command protocol.Command) error {
	unlock := node.agent.LockPublication()
	var stopped []protocol.ConversationRun
	var changes []protocol.Event
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		stopped, changes, err = store.StopSessionRemoval(ctx, db, command, time.Now())
		if err != nil {
			return err
		}
		_, err = store.RecordEvents(ctx, db, node.ID(), changes)
		return err
	})
	if err == nil {
		for _, run := range stopped {
			node.agent.StopCommitted(run)
		}
		node.conversationEvents(changes)
	}
	unlock()
	if err == nil && len(stopped) > 0 {
		node.publish()
		node.agent.Committed()
		node.RuntimeCredentialsChanged()
	}
	return err
}
