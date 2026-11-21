package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"maps"
	"reflect"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// ReadAgentStatistics derives session totals from canonical turn membership.
// The caller's transaction keeps usage, observation and policy reads consistent.
func ReadAgentStatistics(ctx context.Context, db *sql.Conn, session protocol.SessionID) (protocol.ConversationStatistics, error) {
	result, _, err := ReadAgentStatisticsReusing(ctx, db, session, nil)
	return result, err
}

// Frozen execution sources override the original turn's provider for helper
// requests. Inherited turns keep their original capture, never the fork's model.
// Ranges come from exact own and delegated membership in the same snapshot.
// Only legacy Anthropic cache-write pricing needs the raw provider usage.
const agentStatisticsUsageRows = `WITH selected AS (
 SELECT json_extract(value,'$.turn') AS turn,json_extract(value,'$.after') AS start_seq FROM json_each(?1)),
 responses AS (
 SELECT coalesce(json_extract(e.body,'$.usage'),json_extract(e.body,'$.summary.usage')) AS usage,
 coalesce(json_extract(e.body,'$.metadata.execution.source.model'),
  CASE WHEN json_extract(e.body,'$.metadata.execution.media') IS NULL OR m.value IS NULL
   THEN json_extract(r.config,'$.model') ELSE json_extract(m.value,'$.model') END) AS model,
 coalesce(json_extract(e.body,'$.metadata.execution.source'),
  CASE WHEN json_extract(e.body,'$.metadata.execution.media') IS NULL OR m.value IS NULL
   THEN a.provider ELSE json_extract(m.value,'$.provider') END) AS provider,
 json_extract(e.body,'$.metadata.execution.timing.elapsed_us') AS elapsed,
 json_extract(e.body,'$.metadata.execution.timing.first_token_us') AS first,
 coalesce(json_extract(e.body,'$.metadata.execution.observation.attempts[0].reasoning_streamed'),0) AS reasoning_streamed,
 json_extract(e.body,'$.metadata.execution.media') AS media,
 a.provider AS captured,json_extract(m.value,'$.provider') AS assigned
 FROM selected h CROSS JOIN agent_events e ON e.turn=h.turn AND e.sequence>h.start_seq
 JOIN turns t ON t.id=e.turn JOIN agent_runs a ON a.turn=t.id
 JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision
 LEFT JOIN session_media media ON media.session=t.session
 LEFT JOIN json_each(media.body) m ON m.key=json_extract(e.body,'$.metadata.execution.media')
 WHERE ` + agentReportableUsage + `)
 SELECT CASE WHEN json_extract(provider,'$.api')='anthropic' AND json_extract(usage,'$.cache_write')>0
  AND coalesce(json_type(usage,'$.pricing'),'null')='null'
  THEN usage ELSE json_remove(usage,'$.provider') END,model,json_extract(provider,'$.api'),elapsed,first,reasoning_streamed,media,
 json_extract(provider,'$.id'),coalesce(
 CASE WHEN json_extract(captured,'$.id')=json_extract(provider,'$.id') AND json_extract(captured,'$.preset.source.kind')='builtin' THEN json_extract(captured,'$.preset.id') END,
 CASE WHEN json_extract(assigned,'$.id')=json_extract(provider,'$.id') AND json_extract(assigned,'$.preset.source.kind')='builtin' THEN json_extract(assigned,'$.preset.id') END)
 FROM responses`

type agentStatisticsPriceKey struct {
	api, model       string
	provider, preset sql.NullString
}

type agentStatisticsPrice struct {
	preset  string
	channel pricedChannel
}

type agentStatisticsPricing struct {
	models  modelPrices
	presets map[string]sql.NullString
	prices  map[agentStatisticsPriceKey]agentStatisticsPrice
}

func (p *agentStatisticsPricing) read(ctx context.Context, db *sql.Conn, key agentStatisticsPriceKey) (agentStatisticsPrice, error) {
	if value, exists := p.prices[key]; exists {
		return value, nil
	}
	preset := key.preset
	if !preset.Valid && key.provider.Valid {
		var exists bool
		if preset, exists = p.presets[key.provider.String]; !exists {
			err := db.QueryRowContext(ctx, `SELECT json_extract(body,'$.provider.preset.id') FROM providers
 WHERE id=?1 AND json_extract(body,'$.provider.preset.source.kind')='builtin'`, key.provider.String).Scan(&preset)
			if err != nil && !errors.Is(err, sql.ErrNoRows) {
				return agentStatisticsPrice{}, err
			}
			p.presets[key.provider.String] = preset
		}
	}
	price, supplier, err := p.models.channel(ctx, db, preset.String, key.api, key.model)
	value := agentStatisticsPrice{preset: preset.String, channel: pricedChannel{price: price, supplier: supplier}}
	if err == nil {
		p.prices[key] = value
	}
	return value, err
}

type agentStatisticsUsage struct {
	metrics protocol.UsageMetrics
	// Effective catalog prices, including a missing price, for legacy rows only.
	prices map[agentStatisticsPriceKey]pricedChannel
}

func (usage agentStatisticsUsage) valid(ctx context.Context, db *sql.Conn, prices *agentStatisticsPricing) (bool, error) {
	for key, previous := range usage.prices {
		current, err := prices.read(ctx, db, key)
		if err != nil || !reflect.DeepEqual(previous, current.channel) {
			return false, err
		}
	}
	return true, nil
}

func (usage *agentStatisticsUsage) extend(ctx context.Context, db *sql.Conn, ranges []agentStatisticsRange, prices *agentStatisticsPricing, work *agentStatisticsWork) error {
	if len(ranges) == 0 {
		return nil
	}
	type row struct {
		usage             canonical.Usage
		model             string
		api, media        sql.NullString
		elapsed, first    sql.NullInt64
		reasoningStreamed bool
		provider, preset  sql.NullString
	}
	keys, err := json.Marshal(ranges)
	if err != nil {
		return err
	}
	rows, err := db.QueryContext(ctx, agentStatisticsUsageRows, string(keys))
	if err != nil {
		return err
	}
	var records []row
	for rows.Next() {
		var value row
		var body []byte
		if err = rows.Scan(&body, &value.model, &value.api, &value.elapsed, &value.first, &value.reasoningStreamed, &value.media, &value.provider, &value.preset); err != nil {
			rows.Close()
			return err
		}
		if err = json.Unmarshal(body, &value.usage); err != nil {
			rows.Close()
			return err
		}
		records = append(records, value)
	}
	if err = errors.Join(rows.Err(), rows.Close()); err != nil {
		return err
	}
	if len(records) == 0 {
		return nil
	}
	work.usageRows += len(records)
	usage.metrics = cloneAgentStatisticsUsage(usage.metrics)
	usage.prices = maps.Clone(usage.prices)
	if usage.prices == nil {
		usage.prices = map[agentStatisticsPriceKey]pricedChannel{}
	}
	for _, row := range records {
		preset := row.preset.String
		tokens := agentUsageTokens(row.usage)
		var complete *protocol.ConversationUsage
		if row.usage.Input != nil && row.usage.Output != nil {
			complete = &tokens
		}
		if row.usage.Pricing == nil && tokenPricedMedia(row.media.String) && complete != nil {
			key := agentStatisticsPriceKey{api: row.api.String, model: row.model, preset: row.preset}
			if !key.preset.Valid {
				key.provider = row.provider
			}
			price, err := prices.read(ctx, db, key)
			if err != nil {
				return err
			}
			preset, usage.prices[key] = price.preset, price.channel
		}
		cost, err := readAgentCost(ctx, db, &prices.models, row.media.String, preset, row.api.String, row.model, row.usage)
		if err != nil {
			return err
		}
		var generation *protocol.UsageGeneration
		if row.usage.Output != nil {
			generation = tokens.DecodeGeneration(usageTime(row.elapsed), usageTime(row.first), row.reasoningStreamed)
		}
		if err = addUsageMetrics(&usage.metrics, complete, cost, generation); err != nil {
			return err
		}
	}
	return nil
}

func cloneAgentStatisticsUsage(result protocol.UsageMetrics) protocol.UsageMetrics {
	if result.Tokens != nil {
		tokens := *result.Tokens
		result.Tokens = &tokens
	}
	if result.Generation != nil {
		generation := *result.Generation
		result.Generation = &generation
	}
	if result.Cost != nil {
		cost := *result.Cost
		if cost.Breakdown != nil {
			breakdown := *cost.Breakdown
			cost.Breakdown = &breakdown
		}
		result.Cost = &cost
	}
	return result
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
			saved, e := readProviderConfiguration(ctx, db, session.Config.Provider)
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
		if len(selected.Models) == 0 && session.Config.Model != "" {
			// A provider without a model list runs any name on a derived window.
			unlisted := protocol.DefaultConversationModel()
			unlisted.ID, unlisted.Context = session.Config.Model, model.ContextLimit(model.Binding{Provider: *selected, Config: session.Config})
			return AgentCompactionBudget(unlisted, percent), nil
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
