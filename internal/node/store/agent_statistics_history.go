package store

import (
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// AgentStatisticsHistory is a completed, read-only projection for one session.
// It retains aggregates and their dependencies, not canonical records.
type AgentStatisticsHistory struct {
	session               protocol.SessionID
	revision, visible     uint64
	turns                 map[string]agentStatisticsTurn
	usage                 [2]agentStatisticsUsage
	events                agentStatisticEventTotals
	result                protocol.ConversationStatistics
	callBytes, faultBytes int
	untrackedCalls        bool
	work                  agentStatisticsWork
}

type agentStatisticsWork struct {
	usageRows, eventRows                     int
	callReads, contextReads, compactionReads int
}

type agentStatisticsTurn struct {
	sequence, position         uint64
	own, child, task, terminal bool
	capture                    [sha256.Size]byte
}

type agentStatisticsRange struct {
	Turn  string `json:"turn"`
	After uint64 `json:"after"`
}

// Sequence is an append cursor, not a fingerprint. AppendCanonical never
// replaces an event and only appends its membership. Rewind changes the
// history revision when it rebuilds all event membership; a fork has a new ID.
// Compare the actual turn sets, as delegation membership changes independently
// of both the session revision and its last visible event.
const agentStatisticsTurns = `WITH membership AS (
 SELECT turn,1 AS own,0 AS child FROM conversation_turns WHERE session=?1
 UNION ALL
 SELECT d.turn,0,1 FROM conversation_turns h JOIN agent_delegations d ON d.parent_turn=h.turn WHERE h.session=?1),
 selected AS (SELECT turn,max(own) AS own,max(child) AS child FROM membership GROUP BY turn)
 SELECT t.id,t.rowid,t.kind='task',h.own,h.child,
 CASE WHEN h.own THEN coalesce(json_extract(a.body,'$.status') IN ('completed','failed','cancelled','interrupted'),0) ELSE 0 END,
 coalesce((SELECT e.sequence FROM agent_events e WHERE e.turn=t.id ORDER BY e.sequence DESC LIMIT 1),0),
 json_array(a.turn IS NOT NULL,r.session IS NOT NULL,json_valid(r.config),json_valid(a.provider),
 CASE WHEN json_valid(r.config) THEN json_extract(r.config,'$.model') ELSE CAST(r.config AS TEXT) END,
 CASE WHEN json_valid(a.provider) THEN json_array(json_extract(a.provider,'$.id'),json_extract(a.provider,'$.api'),
  json_extract(a.provider,'$.preset.source.kind'),json_extract(a.provider,'$.preset.id'),
  (SELECT json_extract(model.value,'$.context') FROM json_each(a.provider,'$.models') model
   WHERE CASE WHEN json_valid(model.value) AND json_valid(r.config) THEN json_extract(model.value,'$.id')=json_extract(r.config,'$.model') END LIMIT 1))
 ELSE CAST(a.provider AS TEXT) END,CAST(media.body AS TEXT))
 FROM selected h JOIN turns t ON t.id=h.turn LEFT JOIN agent_runs a ON a.turn=t.id
 LEFT JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision
 LEFT JOIN session_media media ON media.session=t.session`

func readAgentStatisticsTurns(ctx context.Context, db *sql.Conn, session protocol.SessionID) (map[string]agentStatisticsTurn, uint64, error) {
	rows, err := db.QueryContext(ctx, agentStatisticsTurns, session.String())
	if err != nil {
		return nil, 0, err
	}
	turns := map[string]agentStatisticsTurn{}
	var rounds uint64
	for rows.Next() {
		var id string
		var capture []byte
		var turn agentStatisticsTurn
		if err := rows.Scan(&id, &turn.position, &turn.task, &turn.own, &turn.child, &turn.terminal, &turn.sequence, &capture); err != nil {
			rows.Close()
			return nil, 0, err
		}
		turn.capture = sha256.Sum256(capture)
		turns[id] = turn
		if turn.own && turn.task && turn.sequence != 0 {
			rounds++
		}
	}
	return turns, rounds, errors.Join(rows.Err(), rows.Close())
}

func (t agentStatisticsTurn) group() int {
	if t.own {
		return 0
	}
	return 1
}

func (t agentStatisticsTurn) round() bool {
	return t.own && t.task && t.sequence != 0
}

type agentStatisticsChanges struct {
	usage                              [2]bool
	events, captures, rounds, outcomes bool
}

// Reuse a group's prefix only while all its old facts still belong to it.
// A newly selected turn can contribute preexisting events, so it starts at 0.
func (h *AgentStatisticsHistory) changes(previous *AgentStatisticsHistory, rewrote bool) agentStatisticsChanges {
	changes := agentStatisticsChanges{events: rewrote}
	if previous == nil {
		return changes
	}
	var lastRound uint64
	for id, old := range previous.turns {
		turn, exists := h.turns[id]
		if old.child != turn.child || (old.own && old.terminal) != (turn.own && turn.terminal) {
			changes.outcomes = true
		}
		if old.round() {
			lastRound = max(lastRound, old.position)
			if !turn.round() || old.position != turn.position {
				changes.rounds = true
			}
		}
		if old.sequence == 0 {
			continue
		}
		if !exists || old.group() != turn.group() || old.sequence > turn.sequence || old.capture != turn.capture {
			changes.usage[old.group()] = true
		}
		if old.own && (!turn.own || old.sequence > turn.sequence) {
			changes.events = true
		}
		if old.own && (!turn.own || old.capture != turn.capture || old.sequence > turn.sequence) {
			changes.captures = true
		}
	}
	for id, turn := range h.turns {
		old, exists := previous.turns[id]
		if !exists && (turn.child || turn.own && turn.terminal) {
			changes.outcomes = true
		}
		// A new round at the end cannot shift any existing call's index.
		if turn.round() && !old.round() && turn.position <= lastRound {
			changes.rounds = true
		}
	}
	return changes
}

func (h *AgentStatisticsHistory) ranges(old map[string]agentStatisticsTurn, group int) []agentStatisticsRange {
	var ranges []agentStatisticsRange
	for id, turn := range h.turns {
		if turn.group() != group {
			continue
		}
		var after uint64
		if prior, exists := old[id]; exists && prior.group() == group {
			after = prior.sequence
		}
		if turn.sequence > after {
			ranges = append(ranges, agentStatisticsRange{Turn: id, After: after})
		}
	}
	return ranges
}

const agentStatisticsHeads = `SELECT s.history_revision,
 coalesce((SELECT h.sequence FROM conversation_events h WHERE h.session=s.id ORDER BY h.sequence DESC LIMIT 1),0)
 FROM sessions s WHERE s.id=?1`

const agentStatisticsVisibleTail = `SELECT
 EXISTS(SELECT 1 FROM conversation_events h CROSS JOIN agent_events e ON e.sequence=h.sequence
 WHERE h.session=?1 AND h.sequence>?2 AND json_type(e.body,'$.metadata.execution.observation')='object'),
 EXISTS(SELECT 1 FROM conversation_events h CROSS JOIN agent_events e ON e.sequence=h.sequence
 WHERE h.session=?1 AND h.sequence>?2 AND (json_extract(e.body,'$.kind')='compaction' OR json_type(e.body,'$.metadata.compaction.failure')='object')),
 EXISTS(SELECT 1 FROM conversation_events h CROSS JOIN agent_events e ON e.sequence=h.sequence
 WHERE h.session=?1 AND h.sequence>?2 AND ((` + agentContextCandidates + `) OR (?3 AND ((` + agentContextTail + `)
 OR (json_extract(e.body,'$.kind')='operation' AND json_extract(e.body,'$.operation')='visibility')))))`

// ReadAgentStatisticsReusing validates and extends the historical segments in
// the caller's snapshot. Keep previous and the returned projection read-only;
// do not retain a new projection from an unsuccessful transaction.
func ReadAgentStatisticsReusing(ctx context.Context, db *sql.Conn, session protocol.SessionID, previous *AgentStatisticsHistory) (protocol.ConversationStatistics, *AgentStatisticsHistory, error) {
	result := protocol.DefaultConversationStatistics()
	current, err := ReadSession(ctx, db, session)
	if err != nil {
		return result, nil, err
	}
	next := &AgentStatisticsHistory{session: session}
	if err = db.QueryRowContext(ctx, agentStatisticsHeads, session.String()).Scan(&next.revision, &next.visible); err != nil {
		return result, nil, err
	}
	if previous != nil && previous.session != session {
		previous = nil
	}
	rewrote := previous == nil || previous.revision != next.revision || previous.visible > next.visible
	newContext, newObservation, newCompaction := rewrote, rewrote, rewrote
	var old map[string]agentStatisticsTurn
	if previous != nil {
		old, result = previous.turns, previous.result
		next.callBytes, next.faultBytes = previous.callBytes, previous.faultBytes
		next.untrackedCalls = previous.untrackedCalls
		if !rewrote && previous.visible != next.visible {
			err = db.QueryRowContext(ctx, agentStatisticsVisibleTail, session.String(), previous.visible, result.ContextEstimate != nil).Scan(&newObservation, &newCompaction, &newContext)
			if err != nil {
				return result, nil, err
			}
		}
	}
	next.turns, result.Turns, err = readAgentStatisticsTurns(ctx, db, session)
	if err != nil {
		return result, nil, err
	}
	changes := next.changes(previous, rewrote)
	prices := agentStatisticsPricing{presets: map[string]sql.NullString{}, prices: map[agentStatisticsPriceKey]agentStatisticsPrice{}}
	for group := range next.usage {
		start := old
		if previous != nil && !changes.usage[group] {
			valid, err := previous.usage[group].valid(ctx, db, &prices)
			if err != nil {
				return result, nil, err
			}
			if valid {
				next.usage[group] = previous.usage[group]
			} else {
				start = nil
			}
		} else {
			start = nil
		}
		if err := next.usage[group].extend(ctx, db, next.ranges(start, group), &prices, &next.work); err != nil {
			return result, nil, err
		}
	}
	metrics := next.usage[0].metrics
	result.Responses, result.Usage = metrics.Responses, metrics.Tokens
	result.IncompleteTokenResponses = metrics.IncompleteTokenResponses
	result.Cost, result.Generation = metrics.Cost, metrics.Generation
	result.Delegated = next.usage[1].metrics

	rebuildCalls := newObservation || next.untrackedCalls || (changes.captures || changes.rounds) && result.Observations.Calls != 0
	if rebuildCalls {
		var observed []string
		next.work.callReads++
		calls, err := readAgentStatisticCalls(ctx, db, session, &observed)
		if err != nil {
			return result, nil, err
		}
		next.untrackedCalls = false
		for _, id := range observed {
			if !next.turns[id].own {
				next.untrackedCalls = true
				break
			}
		}
		result.Observations = agentStatisticsCallsSummary(calls)
		next.callBytes = agentStatisticsProjectedBytes(result.Observations.Recent)
	}
	if changes.events {
		old = nil
	} else {
		next.events = previous.events
	}
	eventRanges := next.ranges(old, 0)
	if err = next.events.extend(ctx, db, session, eventRanges, &next.work); err != nil {
		return result, nil, err
	}
	if rebuildCalls || changes.events || changes.outcomes || len(eventRanges) != 0 {
		next.events.apply(&result.Observations, next.turns)
	}

	if newContext || changes.captures {
		result.ContextTokens, result.ContextEstimate, result.FirstTokenUs = nil, nil, nil
		result.ContextLimit, result.ContextModel, result.ContextTimestampMs = nil, nil, nil
		next.work.contextReads++
		if err = agentStatisticsContext(ctx, db, session, &result); err != nil {
			return result, nil, err
		}
	}
	// The current policy can change without any canonical append (active/queued
	// selection, saved provider, compaction settings or the session's profile).
	if result.Budget, err = agentStatisticsBudget(ctx, db, current); err != nil {
		return result, nil, err
	}
	if newCompaction {
		next.work.compactionReads++
		if result.LastCompaction, err = agentStatisticsCompaction(ctx, db, session); err != nil {
			return result, nil, err
		}
		next.faultBytes = agentStatisticsProjectedBytes(result.LastCompaction)
	}
	next.result = result
	return result, next, nil
}

func agentStatisticsProjectedBytes(value any) int {
	body, err := json.Marshal(value)
	if err != nil {
		return protocol.MAX_FRAME_BYTES
	}
	return len(body)
}

// Weight charges the retained projection and its dependency keys against the
// Runtime cache, including the variable-size policy and observation fields.
func (h *AgentStatisticsHistory) Weight() int {
	weight := 1024 + len(h.turns)*128 + len(h.events.phases)*128 + len(h.events.replies)*64 + h.callBytes*4 + h.faultBytes*2
	for tool := range h.events.tools {
		weight += 64 + len(tool)
	}
	for _, usage := range h.usage {
		for key, channel := range usage.prices {
			weight += 256 + len(key.api) + len(key.model) + len(key.provider.String) + len(key.preset.String) + len(channel.supplier)
			if channel.price != nil {
				weight += len(channel.price.tiers) * 128
			}
		}
	}
	if h.result.ContextModel != nil {
		weight += len(*h.result.ContextModel)
	}
	if h.result.Budget != nil {
		weight += len(h.result.Budget.Model)
	}
	return weight
}

// AgentStatisticsAffected finds other observed sessions selecting the changed
// turns, either as inherited history or as usage of a direct delegation.
func AgentStatisticsAffected(ctx context.Context, db *sql.Conn, candidates []protocol.SessionID, turns []protocol.TurnID) ([]protocol.SessionID, error) {
	sessions, err := json.Marshal(candidates)
	if err != nil {
		return nil, err
	}
	keys, err := json.Marshal(turns)
	if err != nil {
		return nil, err
	}
	rows, err := db.QueryContext(ctx, `WITH affected AS (
 SELECT value AS turn FROM json_each(?2)
 UNION SELECT d.parent_turn FROM json_each(?2) changed JOIN agent_delegations d ON d.turn=changed.value)
 SELECT candidates.value FROM json_each(?1) candidates WHERE EXISTS(
 SELECT 1 FROM affected a CROSS JOIN conversation_turns h ON h.session=candidates.value AND h.turn=a.turn)`, string(sessions), string(keys))
	if err != nil {
		return nil, err
	}
	var result []protocol.SessionID
	for rows.Next() {
		var key string
		if err = rows.Scan(&key); err != nil {
			rows.Close()
			return nil, err
		}
		session, err := protocol.ParseSessionID(key)
		if err != nil {
			rows.Close()
			return nil, err
		}
		result = append(result, session)
	}
	return result, errors.Join(rows.Err(), rows.Close())
}
