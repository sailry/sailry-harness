package node

import (
	"context"
	"database/sql"
	"time"

	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
)

// commandStalled records a background command's silent period and lets the
// existing delivery path wake its session, as a completion does.
func (node *Node) commandStalled(ctx context.Context, stalled process.Stalled) error {
	unlock := node.agent.LockPublication()
	now := time.Now()
	recorded := false
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		changes, written, err := store.StallCommand(ctx, db, stalled, now)
		if err != nil || !written {
			return err
		}
		recorded = true
		_, scheduled, err := store.ScheduleBackground(ctx, db, node.ID(), stalled.Snapshot.Info.Session, now)
		if err != nil {
			return err
		}
		_, err = store.RecordEvents(ctx, db, node.ID(), append(changes, scheduled...))
		return err
	})
	recorded = recorded && err == nil
	if recorded {
		node.publish()
		node.agent.RefreshCommitted(stalled.Snapshot.Info.Session)
	}
	unlock()
	if recorded {
		node.agent.Committed()
	}
	return err
}
