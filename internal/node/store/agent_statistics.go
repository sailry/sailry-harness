package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// ReadAgentStatistics derives session totals from canonical turn membership.
// The caller's transaction keeps usage, observation and policy reads consistent.
func ReadAgentStatistics(ctx context.Context, db *sql.Conn, session protocol.SessionID) (protocol.ConversationStatistics, error) {
	result := protocol.DefaultConversationStatistics()
	current, err := ReadSession(ctx, db, session)
	if err != nil {
		return result, err
	}
	if err = db.QueryRowContext(ctx, `SELECT count(*) FROM conversation_turns h JOIN turns t ON t.id=h.turn
 WHERE h.session=?1 AND t.kind='task' AND EXISTS(SELECT 1 FROM agent_events e WHERE e.turn=h.turn)`, session.String()).Scan(&result.Turns); err != nil {
		return result, err
	}
	metrics, delegated, err := agentStatisticsMetrics(ctx, db, session)
	if err != nil {
		return result, err
	}
	result.Responses, result.Usage = metrics.Responses, metrics.Tokens
	result.Cost, result.Generation = metrics.Cost, metrics.Generation
	result.Delegated = delegated
	if result.Observations, err = agentStatisticsObservations(ctx, db, session); err != nil {
		return result, err
	}
	if err = agentStatisticsContext(ctx, db, session, &result); err != nil {
		return result, err
	}
	if result.Budget, err = agentStatisticsBudget(ctx, db, current); err != nil {
		return result, err
	}
	result.LastCompaction, err = agentStatisticsCompaction(ctx, db, session)
	return result, err
}

// Frozen execution sources override the original turn's provider for helper
// requests. Inherited turns keep their original capture, never the fork's model.
// The selection is the session's own turns plus the child turns delegated from
// them, which are Sailry's own sessions and are totalled apart.
const agentStatisticsUsageRows = `WITH selected AS (
 SELECT turn,0 AS delegated FROM conversation_turns WHERE session=?1
 UNION
 SELECT d.turn,1 FROM agent_delegations d JOIN conversation_turns h ON h.turn=d.parent_turn
 WHERE h.session=?1 AND NOT EXISTS(SELECT 1 FROM conversation_turns own WHERE own.session=?1 AND own.turn=d.turn)),
 responses AS (
 SELECT h.delegated,coalesce(json_extract(e.body,'$.usage'),json_extract(e.body,'$.summary.usage')) AS usage,
 coalesce(json_extract(e.body,'$.metadata.execution.source.model'),
  CASE WHEN json_extract(e.body,'$.metadata.execution.media') IS NULL OR m.value IS NULL
   THEN json_extract(r.config,'$.model') ELSE json_extract(m.value,'$.model') END) AS model,
 coalesce(json_extract(e.body,'$.metadata.execution.source'),
  CASE WHEN json_extract(e.body,'$.metadata.execution.media') IS NULL OR m.value IS NULL
   THEN a.provider ELSE json_extract(m.value,'$.provider') END) AS provider,
 json_extract(e.body,'$.metadata.execution.timing.elapsed_us') AS elapsed,
 json_extract(e.body,'$.metadata.execution.media') AS media
 FROM selected h JOIN agent_events e ON e.turn=h.turn
 JOIN turns t ON t.id=e.turn JOIN agent_runs a ON a.turn=t.id
 JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision
 LEFT JOIN session_media media ON media.session=t.session
 LEFT JOIN json_each(media.body) m ON m.key=json_extract(e.body,'$.metadata.execution.media')
 WHERE coalesce(json_type(e.body,'$.usage'),json_type(e.body,'$.summary.usage'))='object'
 AND coalesce(json_extract(e.body,'$.usage.input'),json_extract(e.body,'$.summary.usage.input'))>=0
 AND coalesce(json_extract(e.body,'$.usage.output'),json_extract(e.body,'$.summary.usage.output'))>=0)
 SELECT usage,model,json_extract(provider,'$.api'),elapsed,media,delegated FROM responses`

// agentStatisticsMetrics totals the session's own usage and, apart from it,
// the usage of the children its turns delegated.
func agentStatisticsMetrics(ctx context.Context, db *sql.Conn, session protocol.SessionID) (protocol.UsageMetrics, protocol.UsageMetrics, error) {
	result, delegated := protocol.DefaultUsageMetrics(), protocol.DefaultUsageMetrics()
	type row struct {
		usage      canonical.Usage
		model      string
		api, media sql.NullString
		elapsed    sql.NullInt64
		delegated  bool
	}
	rows, err := db.QueryContext(ctx, agentStatisticsUsageRows, session.String())
	if err != nil {
		return result, delegated, err
	}
	var records []row
	for rows.Next() {
		var value row
		var body []byte
		if err = rows.Scan(&body, &value.model, &value.api, &value.elapsed, &value.media, &value.delegated); err != nil {
			rows.Close()
			return result, delegated, err
		}
		if err = json.Unmarshal(body, &value.usage); err != nil {
			rows.Close()
			return result, delegated, err
		}
		records = append(records, value)
	}
	if err = errors.Join(rows.Err(), rows.Close()); err != nil {
		return result, delegated, err
	}
	prices := modelPrices{}
	for _, row := range records {
		var cost *protocol.UsageCost
		if !row.media.Valid || row.media.String != "image" && row.media.String != "video" {
			cost, err = estimateAgentCost(ctx, db, prices, row.api.String, row.model, row.usage)
			if err != nil {
				return result, delegated, err
			}
		}
		var elapsed *uint64
		if row.elapsed.Valid && row.elapsed.Int64 >= 0 {
			value := uint64(row.elapsed.Int64)
			elapsed = &value
		}
		tokens := protocol.ConversationUsage{Input: optionalUint(row.usage.Input), Output: optionalUint(row.usage.Output), CachedInput: optionalUint(row.usage.CacheRead), Reasoning: optionalUint(row.usage.Reasoning)}
		total := &result
		if row.delegated {
			total = &delegated
		}
		if err = addUsageMetrics(total, tokens, cost, elapsed); err != nil {
			return result, delegated, err
		}
	}
	return result, delegated, nil
}

// AgentCompactionBudget describes the same captured threshold used by execution.
func AgentCompactionBudget(selected protocol.ConversationModel, percent uint8) *protocol.ConversationCompactionBudget {
	if selected.Context == 0 {
		return nil
	}
	trigger := uint64(selected.Context) * uint64(percent) / 100
	if selected.CompactionTokens != nil {
		trigger = uint64(*selected.CompactionTokens)
	}
	binding := model.Binding{Config: protocol.SessionConfig{Model: selected.ID}, Provider: protocol.ConversationProvider{Models: []protocol.ConversationModel{selected}}}
	output := uint32(model.OutputLimit(binding, true))
	return &protocol.ConversationCompactionBudget{Model: selected.ID, ContextLimit: selected.Context, TriggerTokens: trigger, OutputReserve: selected.Output, SummaryInput: uint64(selected.Context - min(selected.Context, output)), SummaryOutput: output}
}

func agentStatisticsBudget(ctx context.Context, db *sql.Conn, session protocol.Session) (*protocol.ConversationCompactionBudget, error) {
	var provider, config []byte
	var percent uint8
	err := db.QueryRowContext(ctx, `SELECT r.provider,revision.config,r.compaction_percent FROM agent_runs r JOIN turns t ON t.id=r.turn
 JOIN session_revisions revision ON revision.session=t.session AND revision.revision=t.revision
 LEFT JOIN agent_pending pending ON pending.turn=r.turn
 WHERE t.session=?1 AND json_extract(r.body,'$.status') IN ('running','stopping','queued')
 ORDER BY CASE WHEN json_extract(r.body,'$.status')='queued' THEN 1 ELSE 0 END,pending.position,t.rowid LIMIT 1`, session.ID.String()).Scan(&provider, &config, &percent)
	var selected *protocol.ConversationProvider
	if err == nil {
		if len(provider) > 0 {
			if err = json.Unmarshal(provider, &selected); err != nil {
				return nil, err
			}
		}
		if err = json.Unmarshal(config, &session.Config); err != nil {
			return nil, err
		}
	} else if errors.Is(err, sql.ErrNoRows) {
		if session.Profile != nil {
			selected = &session.Profile.Provider
		} else {
			saved, e := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", session.Config.Provider.String())
			if e != nil {
				return nil, e
			}
			if saved != nil && saved.Listed {
				selected = saved.Provider
			}
		}
		settings, e := ReadCompaction(ctx, db)
		if e != nil {
			return nil, e
		}
		percent = settings.Percent
	} else {
		return nil, err
	}
	if selected != nil {
		if i := slices.IndexFunc(selected.Models, func(m protocol.ConversationModel) bool { return m.ID == session.Config.Model }); i >= 0 {
			return AgentCompactionBudget(selected.Models[i], percent), nil
		}
	}
	return nil, nil
}

func agentStatisticsCompaction(ctx context.Context, db *sql.Conn, session protocol.SessionID) (*protocol.ConversationCompactionRecord, error) {
	var result protocol.ConversationCompactionRecord
	var body []byte
	err := db.QueryRowContext(ctx, `SELECT h.sequence,e.body,t.rowid FROM conversation_events h
 JOIN agent_events e ON e.sequence=h.sequence JOIN turns t ON t.id=e.turn WHERE h.session=?1 AND
 (json_extract(e.body,'$.kind')='compaction' OR json_type(e.body,'$.metadata.compaction.failure')='object')
 ORDER BY h.sequence DESC LIMIT 1`, session.String()).Scan(&result.Sequence, &body, &result.TurnSequence)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var record canonical.Record
	if err = json.Unmarshal(body, &record); err != nil {
		return nil, err
	}
	result.ID, result.Turn, result.TimestampMs = record.ID, record.Turn, record.Created.UnixMilli()
	result.Outcome.Kind = "succeeded"
	if record.Kind != "compaction" {
		if record.Metadata.Compaction == nil || record.Metadata.Compaction.Failure == nil {
			return nil, invalid("compaction record is missing its failure")
		}
		result.Outcome = protocol.ConversationCompactionOutcome{Kind: "failed", Data: record.Metadata.Compaction.Failure}
	}
	return &result, nil
}
