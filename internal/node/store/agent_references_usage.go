package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"math"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const referenceUsageDefaultLimit = 12

const (
	referenceUsageCacheRead  = `coalesce(json_extract(e.body,'$.usage.cache_read'),json_extract(e.body,'$.summary.usage.cache_read'))`
	referenceUsageCacheWrite = `coalesce(json_extract(e.body,'$.usage.cache_write'),json_extract(e.body,'$.summary.usage.cache_write'))`
	referenceUsageReasoning  = `coalesce(json_extract(e.body,'$.usage.reasoning'),json_extract(e.body,'$.summary.usage.reasoning'))`
	referenceUsageTotal      = `coalesce(json_extract(e.body,'$.usage.total'),json_extract(e.body,'$.summary.usage.total'))`
	referenceUsageColumns    = agentUsageInput + `,` + agentUsageOutput + `,` + referenceUsageCacheRead + `,` + referenceUsageCacheWrite + `,` + referenceUsageReasoning + `,` + referenceUsageTotal
	referenceUsageFrom       = ` FROM agent_events e CROSS JOIN conversation_events h ON h.session=?1 AND h.sequence=e.sequence
 WHERE e.turn=?2 AND ` + agentUsageObject + ` AND coalesce(` + referenceUsageColumns + `) IS NOT NULL`
)

type referenceUsageTokens struct {
	Input         *uint64 `json:"input"`
	Output        *uint64 `json:"output"`
	CachedInput   *uint64 `json:"cached_input"`
	CacheWrite    *uint64 `json:"cache_write"`
	Reasoning     *uint64 `json:"reasoning"`
	ReportedTotal *uint64 `json:"reported_total"`
}

func (tokens *referenceUsageTokens) scanTargets() []any {
	return []any{&tokens.Input, &tokens.Output, &tokens.CachedInput, &tokens.CacheWrite, &tokens.Reasoning, &tokens.ReportedTotal}
}

type referenceUsageRequest struct {
	EntrySequence     uint64                                  `json:"entry_sequence"`
	EntryPublic       bool                                    `json:"entry_public"`
	TimestampMs       int64                                   `json:"timestamp_ms"`
	Model             string                                  `json:"model,omitempty"`
	Usage             referenceUsageTokens                    `json:"usage"`
	RequestID         string                                  `json:"request_id,omitempty"`
	Attempt           uint64                                  `json:"attempt,omitempty"`
	ResponsesBoundary *protocol.ConversationResponsesBoundary `json:"responses_boundary,omitempty"`
}

type referenceUsage struct {
	Session           protocol.SessionID          `json:"session"`
	Revision          uint64                      `json:"revision"`
	Turn              protocol.TurnID             `json:"turn"`
	Status            protocol.ConversationStatus `json:"status"`
	ReportedResponses uint64                      `json:"reported_responses"`
	MeasuredResponses uint64                      `json:"measured_responses"`
	Usage             referenceUsageTokens        `json:"usage"`
	Requests          []referenceUsageRequest     `json:"requests"`
	NextBefore        *uint64                     `json:"next_before"`
}

func readAgentReferenceUsage(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID, revision uint64, before *uint64, limit int) ([]byte, error) {
	boundary := uint64(math.MaxInt64)
	if before != nil {
		if *before == 0 || *before > math.MaxInt64 {
			return nil, invalid("usage entry cursor is invalid")
		}
		boundary = *before
	}
	status, err := referenceTurnStatus(ctx, db, session, turn)
	if err != nil {
		return nil, err
	}
	value := referenceUsage{Session: session, Revision: revision, Turn: turn, Status: status, Requests: []referenceUsageRequest{}}
	const totals = `SELECT count(*),count(CASE WHEN ` + agentUsagePair + ` THEN 1 END),sum(` + agentUsageInput + `),sum(` + agentUsageOutput + `),sum(` +
		referenceUsageCacheRead + `),sum(` + referenceUsageCacheWrite + `),sum(` + referenceUsageReasoning + `),sum(` + referenceUsageTotal + `)` + referenceUsageFrom
	targets := append([]any{&value.ReportedResponses, &value.MeasuredResponses}, value.Usage.scanTargets()...)
	if err = db.QueryRowContext(ctx, totals, session.String(), turn.String()).Scan(targets...); err != nil {
		return nil, err
	}
	const requests = `SELECT e.sequence,` + visibleCanonical + `,json_extract(e.body,'$.created'),
 substr(coalesce(json_extract(e.body,'$.metadata.execution.source.model'),json_extract(e.body,'$.model'),json_extract(e.body,'$.summary.model'),''),1,128),
 coalesce(json_extract(e.body,'$.metadata.execution.observation.id'),''),
 coalesce(json_extract(e.body,'$.metadata.execution.observation.attempts[0].index'),0),
 json_extract(e.body,'$.metadata.execution.observation.attempts[0].responses_boundary'),` +
		referenceUsageColumns + referenceUsageFrom + ` AND e.sequence<?3 ORDER BY e.sequence DESC LIMIT ?4`
	rows, err := db.QueryContext(ctx, requests, session.String(), turn.String(), boundary, limit+1)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		if len(value.Requests) == limit {
			value.NextBefore = new(value.Requests[limit-1].EntrySequence)
			break
		}
		var item referenceUsageRequest
		var created string
		var responseBoundary []byte
		targets = append([]any{&item.EntrySequence, &item.EntryPublic, &created, &item.Model, &item.RequestID, &item.Attempt, &responseBoundary}, item.Usage.scanTargets()...)
		if err = rows.Scan(targets...); err != nil {
			return nil, err
		}
		if len(responseBoundary) != 0 {
			if err = json.Unmarshal(responseBoundary, &item.ResponsesBoundary); err != nil {
				return nil, err
			}
		}
		at, err := time.Parse(time.RFC3339Nano, created)
		if err != nil {
			return nil, err
		}
		item.TimestampMs = at.UnixMilli()
		item.Model = referencePrefix(item.Model, 128)
		value.Requests = append(value.Requests, item)
	}
	if err = rows.Err(); err != nil {
		return nil, err
	}
	return encode(value)
}
