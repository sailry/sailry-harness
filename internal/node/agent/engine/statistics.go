package engine

import (
	"database/sql"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The store's supervised operation capacity also bounds statistical readers.
const statisticsCapacity = 32

const (
	statisticsHistoryCapacity = 16
	statisticsHistoryBudget   = 2 << 20
	statisticsDependencyLimit = 256
)

type statisticsFlight struct {
	dirty    bool
	revision uint64
}

type statisticsCompleted struct {
	session protocol.SessionID
	history *store.AgentStatisticsHistory
	weight  int
}

type statisticsDependencies struct {
	wake               chan struct{}
	pending            statisticsDependencyBatch
	revision, resolved uint64
}

type statisticsDependencyBatch struct {
	// Omit the directly refreshed session when all changes have that source.
	source protocol.SessionID
	turns  map[protocol.TurnID]struct{}
	all    bool
}

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
	r.refreshStatisticsLocked(session)
}

// Appends also affect observers of inherited turns and delegated usage, even
// when nobody is observing the writer's conversation.
func (r *Runtime) refreshStatisticsForTurnsCommitted(session protocol.SessionID, turns []protocol.TurnID) {
	r.statisticsMu.Lock()
	defer r.statisticsMu.Unlock()
	r.refreshStatisticsLocked(session)
	if len(turns) == 0 || r.statisticsStopped || r.ctx.Err() != nil {
		return
	}
	for _, candidate := range r.feed.Sessions() {
		if candidate != session && r.feed.Observed(candidate) {
			r.queueStatisticsDependenciesLocked(session, turns)
			return
		}
	}
}

func (r *Runtime) queueStatisticsDependenciesLocked(session protocol.SessionID, turns []protocol.TurnID) {
	changes := &r.statisticsChanges
	// Fence older snapshots before releasing publication, even if the lookup
	// has not yet found which observers this commit affects.
	changes.revision++
	batch := &changes.pending
	if batch.turns == nil && !batch.all {
		batch.source = session
		batch.turns = make(map[protocol.TurnID]struct{})
	} else if batch.source != session {
		batch.source = protocol.SessionID{}
	}
	if !batch.all {
		for _, turn := range turns {
			batch.turns[turn] = struct{}{}
			if len(batch.turns) > statisticsDependencyLimit {
				// Bound the backlog without dropping any observer's refresh.
				batch.turns, batch.all = nil, true
				break
			}
		}
	}
	if changes.wake == nil {
		changes.wake = make(chan struct{}, 1)
		r.statisticsOwners.Add(1)
		go r.resolveStatisticsDependencies(changes.wake)
	}
	select {
	case changes.wake <- struct{}{}:
	default:
	}
}

func (r *Runtime) resolveStatisticsDependencies(wake <-chan struct{}) {
	defer r.statisticsOwners.Done()
	for r.ctx.Err() == nil {
		select {
		case <-r.ctx.Done():
			return
		case <-wake:
		}
		r.statisticsMu.Lock()
		batch, revision := r.statisticsChanges.pending, r.statisticsChanges.revision
		r.statisticsChanges.pending = statisticsDependencyBatch{}
		r.statisticsMu.Unlock()
		if len(batch.turns) == 0 && !batch.all {
			continue
		}

		candidates := slices.DeleteFunc(r.feed.Sessions(), func(session protocol.SessionID) bool {
			return session == batch.source || !r.feed.Observed(session)
		})
		affected := candidates
		if len(candidates) != 0 && !batch.all {
			turns := make([]protocol.TurnID, 0, len(batch.turns))
			for turn := range batch.turns {
				turns = append(turns, turn)
			}
			if err := r.db.Read(r.ctx, func(db *sql.Conn) error {
				var err error
				affected, err = store.AgentStatisticsAffected(r.ctx, db, candidates, turns)
				return err
			}); err != nil {
				// A failed lookup cannot prove any observer is unaffected.
				affected = candidates
			}
		}
		r.finishStatisticsDependencies(revision, affected)
	}
}

func (r *Runtime) finishStatisticsDependencies(revision uint64, affected []protocol.SessionID) {
	unlock := r.LockPublication()
	defer unlock()
	r.statisticsMu.Lock()
	defer r.statisticsMu.Unlock()
	if r.statisticsStopped || r.ctx.Err() != nil {
		return
	}
	for _, session := range affected {
		if flight, pending := r.statistics[session]; pending && flight.revision >= revision {
			continue // This read already follows the entire batch.
		}
		r.refreshStatisticsLocked(session)
	}
	r.statisticsChanges.resolved = revision
}

// RequestStatisticsCommitted fences the new reservation's snapshot. An older
// flight may predate writes made after the last observer closed, when metadata
// and policy changes need not have scheduled statistics.
func (r *Runtime) RequestStatisticsCommitted(session protocol.SessionID) {
	r.RefreshStatisticsCommitted(session)
}

func (r *Runtime) refreshStatisticsLocked(session protocol.SessionID) {
	if r.statisticsStopped {
		return
	}
	if flight, pending := r.statistics[session]; pending {
		flight.dirty = true
		r.statistics[session] = flight
		return
	}
	// Statistics are derived from durable history. A new subscription requests
	// current totals after reserving its stream, including changes made unseen.
	if !r.feed.Observed(session) {
		return
	}
	if len(r.statistics) >= statisticsCapacity {
		r.feed.Publish(session, protocol.ConversationChange{Kind: "statistics_failed", Data: protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "statistics refresh did not complete"}})
		return
	}
	r.statistics[session] = statisticsFlight{}
	var previous *store.AgentStatisticsHistory
	for _, cached := range r.statisticsHistory {
		if cached.session == session {
			previous = cached.history
			break
		}
	}
	r.statisticsOwners.Add(1)
	go func() {
		defer r.statisticsOwners.Done()
		for {
			r.beginStatisticsRead(session)
			var statistics protocol.ConversationStatistics
			completed := statisticsCompleted{session: session}
			err := r.db.Read(r.ctx, func(db *sql.Conn) error {
				var err error
				statistics, completed.history, err = store.ReadAgentStatisticsReusing(r.ctx, db, session, previous)
				return err
			})
			if err == nil {
				completed.weight = completed.history.Weight()
				if completed.weight <= statisticsHistoryBudget {
					// A later commit can invalidate publication without making
					// the completed read's immutable prefixes unusable on retry.
					previous = completed.history
				} else {
					previous = nil
				}
			}
			if r.finishStatistics(completed, statistics, err) {
				return
			}
		}
	}()
}

// Requests and commits before this boundary share the next snapshot. A later
// request cannot prove it was included in the read about to publish.
func (r *Runtime) beginStatisticsRead(session protocol.SessionID) {
	r.statisticsMu.Lock()
	r.statistics[session] = statisticsFlight{revision: r.statisticsChanges.revision}
	r.statisticsMu.Unlock()
}

func (r *Runtime) finishStatistics(completed statisticsCompleted, statistics protocol.ConversationStatistics, err error) bool {
	unlock := r.LockPublication()
	defer unlock()
	r.statisticsMu.Lock()
	defer r.statisticsMu.Unlock()
	session := completed.session
	if !r.feed.Observed(session) {
		delete(r.statistics, session)
		return true
	}
	flight, changes := r.statistics[session], &r.statisticsChanges
	unresolved := flight.revision < changes.revision && changes.resolved < changes.revision
	if flight.dirty || unresolved {
		stopped := r.ctx.Err()
		if stopped == nil {
			return false
		}
		if err == nil {
			err = stopped
		}
	}
	if err == nil {
		r.rememberStatisticsLocked(completed)
		r.feed.Publish(session, protocol.ConversationChange{Kind: "statistics", Data: statistics})
	} else {
		r.feed.Publish(session, protocol.ConversationChange{Kind: "statistics_failed", Data: *model.Fault(err)})
	}
	delete(r.statistics, session)
	return true
}

func (r *Runtime) rememberStatisticsLocked(completed statisticsCompleted) {
	r.statisticsHistory = slices.DeleteFunc(r.statisticsHistory, func(cached statisticsCompleted) bool {
		return cached.session == completed.session
	})
	if completed.history == nil || completed.weight > statisticsHistoryBudget {
		return
	}
	weight := completed.weight
	first := len(r.statisticsHistory)
	for first > 0 && len(r.statisticsHistory)-first < statisticsHistoryCapacity-1 {
		weight += r.statisticsHistory[first-1].weight
		if weight > statisticsHistoryBudget {
			break
		}
		first--
	}
	r.statisticsHistory = append(slices.Delete(r.statisticsHistory, 0, first), completed)
}
