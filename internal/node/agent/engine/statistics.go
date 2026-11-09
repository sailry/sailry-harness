package engine

import (
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The store's supervised operation capacity also bounds statistical readers.
const statisticsCapacity = 32

// RefreshStatistics coalesces committed changes under the Node lifetime. A read
// made dirty by a later change cannot replace that change's controller state.
func (r *Runtime) RefreshStatistics(session protocol.SessionID) {
	unlock := r.LockPublication()
	defer unlock()
	r.RefreshStatisticsCommitted(session)
}

// RefreshStatisticsCommitted follows this commit's frames under publication.
func (r *Runtime) RefreshStatisticsCommitted(session protocol.SessionID) {
	r.statisticsMu.Lock()
	defer r.statisticsMu.Unlock()
	r.refreshStatisticsLocked(session, true)
}

// RequestStatisticsCommitted shares an existing flight after a snapshot
// reservation without making an otherwise unchanged read dirty.
func (r *Runtime) RequestStatisticsCommitted(session protocol.SessionID) {
	r.statisticsMu.Lock()
	defer r.statisticsMu.Unlock()
	r.refreshStatisticsLocked(session, false)
}

func (r *Runtime) refreshStatisticsLocked(session protocol.SessionID, dirty bool) {
	if r.statisticsStopped {
		return
	}
	if _, pending := r.statistics[session]; pending {
		if dirty {
			r.statistics[session] = true
		}
		return
	}
	if len(r.statistics) >= statisticsCapacity {
		r.feed.Publish(session, protocol.ConversationChange{Kind: "statistics_failed", Data: protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "statistics refresh did not complete"}})
		return
	}
	r.statistics[session] = false
	r.statisticsOwners.Add(1)
	go func() {
		defer r.statisticsOwners.Done()
		for {
			var statistics protocol.ConversationStatistics
			err := r.db.Read(r.ctx, func(db *sql.Conn) error {
				var err error
				statistics, err = store.ReadAgentStatistics(r.ctx, db, session)
				return err
			})
			if r.finishStatistics(session, statistics, err) {
				return
			}
		}
	}()
}

func (r *Runtime) finishStatistics(session protocol.SessionID, statistics protocol.ConversationStatistics, err error) bool {
	unlock := r.LockPublication()
	defer unlock()
	r.statisticsMu.Lock()
	defer r.statisticsMu.Unlock()
	if r.statistics[session] && r.ctx.Err() == nil {
		r.statistics[session] = false
		return false
	}
	if err == nil {
		r.feed.Publish(session, protocol.ConversationChange{Kind: "statistics", Data: statistics})
	} else {
		r.feed.Publish(session, protocol.ConversationChange{Kind: "statistics_failed", Data: *model.Fault(err)})
	}
	delete(r.statistics, session)
	return true
}
