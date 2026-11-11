package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// TurnTokens totals recorded model usage. Cached input and reasoning are
// parts of Input and Output, not additions to them.
type TurnTokens struct {
	Input       uint64 `json:"input"`
	Output      uint64 `json:"output"`
	CachedInput uint64 `json:"cached_input"`
	Reasoning   uint64 `json:"reasoning"`
}

// TurnFacts is what a session's stored state says once a turn has ended. A
// package loop decides from it whether to continue, wait or pause.
type TurnFacts struct {
	// Usage is the turn's own model usage plus that of the children it
	// delegated; SessionUsage is the same for every turn of the session.
	Usage        TurnTokens
	SessionUsage TurnTokens
	// Running counts background work that will start a notice turn when it
	// ends; Pending counts ended results no turn has taken yet.
	Running int
	Pending int
	Chain   AutomaticChain
}

const turnUsageTotals = `SELECT coalesce(sum(coalesce(json_extract(e.body,'$.usage.input'),json_extract(e.body,'$.summary.usage.input'))),0),
 coalesce(sum(coalesce(json_extract(e.body,'$.usage.output'),json_extract(e.body,'$.summary.usage.output'))),0),
 coalesce(sum(coalesce(json_extract(e.body,'$.usage.cache_read'),json_extract(e.body,'$.summary.usage.cache_read'))),0),
 coalesce(sum(coalesce(json_extract(e.body,'$.usage.reasoning'),json_extract(e.body,'$.summary.usage.reasoning'))),0)
 FROM selected h JOIN agent_events e ON e.turn=h.turn
 WHERE coalesce(json_type(e.body,'$.usage'),json_type(e.body,'$.summary.usage'))='object'
 AND coalesce(json_extract(e.body,'$.usage.input'),json_extract(e.body,'$.summary.usage.input'))>=0
 AND coalesce(json_extract(e.body,'$.usage.output'),json_extract(e.body,'$.summary.usage.output'))>=0`

// ReadTurnFacts reads the facts of an ended turn in the caller's transaction.
// Usage follows the statistics selection: Sailry's own turns and the child
// turns delegated from them.
func ReadTurnFacts(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID) (TurnFacts, error) {
	var facts TurnFacts
	err := db.QueryRowContext(ctx, `WITH selected AS (SELECT ?1 AS turn UNION SELECT turn FROM agent_delegations WHERE parent_turn=?1) `+turnUsageTotals, turn.String()).
		Scan(&facts.Usage.Input, &facts.Usage.Output, &facts.Usage.CachedInput, &facts.Usage.Reasoning)
	if err != nil {
		return facts, err
	}
	err = db.QueryRowContext(ctx, `WITH selected AS (
 SELECT turn FROM conversation_turns WHERE session=?1
 UNION
 SELECT d.turn FROM agent_delegations d JOIN conversation_turns h ON h.turn=d.parent_turn WHERE h.session=?1) `+turnUsageTotals, session.String()).
		Scan(&facts.SessionUsage.Input, &facts.SessionUsage.Output, &facts.SessionUsage.CachedInput, &facts.SessionUsage.Reasoning)
	if err != nil {
		return facts, err
	}
	err = db.QueryRowContext(ctx, `SELECT
 (SELECT count(*) FROM agent_background WHERE session=?1 AND state='running' AND (kind='agent' OR json_extract(body,'$.follow_up')=1)),
 (SELECT count(*) FROM agent_background WHERE session=?1 AND state='pending' AND delivery IS NULL)`, session.String()).Scan(&facts.Running, &facts.Pending)
	if err != nil {
		return facts, err
	}
	facts.Chain, err = ReadAutomaticChain(ctx, db, session)
	return facts, err
}
