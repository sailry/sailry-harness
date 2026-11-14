package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/tokens"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type agentContextObservation struct {
	sequence                uint64
	tokens, estimate, first *uint64
	limit                   *uint32
	model                   *string
	timestamp               *int64
	summary                 bool
}

const agentContextObservationQuery = `SELECT h.sequence,
 CASE WHEN json_extract(e.body,'$.kind')!='compaction' AND json_extract(e.body,'$.usage.input')>=0
  AND json_extract(e.body,'$.usage.output')>=0 THEN json_extract(e.body,'$.usage.input')+json_extract(e.body,'$.usage.output') END,
 CASE WHEN json_extract(e.body,'$.kind')!='compaction' AND json_extract(e.body,'$.usage.input')>=0
  AND json_extract(e.body,'$.usage.output')>=0 AND json_type(e.body,'$.metadata.execution.timing.first_token_us')='integer'
  AND json_extract(e.body,'$.metadata.execution.timing.first_token_us')>=0 THEN json_extract(e.body,'$.metadata.execution.timing.first_token_us') END,
 (SELECT json_extract(model.value,'$.context') FROM json_each(r.provider,'$.models') model
  WHERE json_extract(model.value,'$.id')=json_extract(revision.config,'$.model') LIMIT 1),
 coalesce(json_extract(e.body,'$.kind')='compaction',0),json_extract(revision.config,'$.model'),json_extract(e.body,'$.created'),
 CASE WHEN json_extract(e.body,'$.kind')!='compaction' AND json_type(e.body,'$.metadata.execution.context_tokens')='integer' AND json_extract(e.body,'$.metadata.execution.context_tokens')>=0
  THEN json_extract(e.body,'$.metadata.execution.context_tokens') END
 FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
 LEFT JOIN agent_runs r ON r.turn=e.turn LEFT JOIN turns t ON t.id=e.turn
 LEFT JOIN session_revisions revision ON revision.session=t.session AND revision.revision=t.revision
 WHERE h.session=?1 AND json_extract(e.body,'$.metadata.execution.media') IS NULL
 AND coalesce(json_extract(e.body,'$.metadata.execution.purpose'),'')!='search'
 AND (coalesce(json_extract(e.body,'$.metadata.execution.purpose'),'')!='compaction' OR json_extract(e.body,'$.kind')='compaction')
 AND ((json_extract(e.body,'$.role')='assistant' AND json_extract(e.body,'$.part_count')>0)
  OR json_type(e.body,'$.usage')='object' OR json_extract(e.body,'$.kind')='compaction'
  OR json_type(e.body,'$.metadata.execution.context_tokens')='integer')
 AND (?2=0 OR json_extract(e.body,'$.kind')='compaction' OR json_type(e.body,'$.metadata.execution.context_tokens')='integer'
  OR (json_extract(e.body,'$.usage.input')>=0 AND json_extract(e.body,'$.usage.output')>=0))
 ORDER BY h.sequence DESC LIMIT 1`

func readAgentContextObservation(ctx context.Context, db *sql.Conn, session protocol.SessionID, measured bool) (*agentContextObservation, error) {
	value := agentContextObservation{}
	var count, first, limit, estimate sql.NullInt64
	var model, timestamp sql.NullString
	err := db.QueryRowContext(ctx, agentContextObservationQuery, session.String(), measured).Scan(&value.sequence, &count, &first, &limit, &value.summary, &model, &timestamp, &estimate)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	if count.Valid {
		v := uint64(count.Int64)
		value.tokens = &v
	}
	if estimate.Valid {
		v := uint64(estimate.Int64)
		value.estimate = &v
	}
	if first.Valid {
		v := uint64(first.Int64)
		value.first = &v
	}
	if count.Valid || estimate.Valid || value.summary {
		if limit.Valid && limit.Int64 > 0 {
			v := uint32(limit.Int64)
			value.limit = &v
		}
		if model.Valid {
			value.model = &model.String
		}
		if timestamp.Valid {
			if created, err := time.Parse(time.RFC3339Nano, timestamp.String); err == nil {
				v := created.UnixMilli()
				value.timestamp = &v
			}
		}
	}
	return &value, nil
}

func agentStatisticsContext(ctx context.Context, db *sql.Conn, session protocol.SessionID, result *protocol.ConversationStatistics) error {
	observation, err := readAgentContextObservation(ctx, db, session, false)
	if err != nil || observation == nil {
		return err
	}
	if observation.tokens == nil && observation.estimate == nil && !observation.summary {
		reset, err := readAgentContextObservation(ctx, db, session, true)
		if err != nil {
			return err
		}
		if reset != nil && (reset.summary || reset.estimate != nil) {
			observation = reset
		}
	}
	result.ContextTokens, result.ContextLimit = observation.tokens, observation.limit
	result.ContextModel, result.ContextTimestampMs = observation.model, observation.timestamp
	result.FirstTokenUs = observation.first
	if !observation.summary && observation.estimate == nil {
		return nil
	}
	sequence := observation.sequence
	count := uint64(0)
	var pending []canonical.Record
	if observation.tokens != nil {
		count = *observation.tokens
	} else if observation.estimate != nil {
		count = *observation.estimate
	} else {
		summary, err := readBody[canonical.Record](ctx, db, `SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
 WHERE h.session=?1 AND json_extract(e.body,'$.kind')='compaction' ORDER BY h.sequence DESC LIMIT 1`, session.String())
		if err != nil || summary == nil {
			return err
		}
		compaction := summary.Data.(*canonical.Compaction)
		if err = db.QueryRowContext(ctx, `SELECT h.sequence FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
 WHERE h.session=?1 AND e.id=?2`, session.String(), compaction.Boundary).Scan(&sequence); err != nil {
			return err
		}
		pending = append(pending, *summary)
	}
	records, err := readBodies[canonical.Record](ctx, db, `SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
 WHERE h.session=?1 AND h.sequence>?2 AND json_extract(e.body,'$.kind')!='compaction'
 AND json_extract(e.body,'$.metadata.execution.media') IS NULL
 AND coalesce(json_extract(e.body,'$.metadata.execution.purpose'),'') NOT IN ('compaction','search') AND `+visibleCanonical+` ORDER BY h.sequence`, session.String(), sequence)
	if err != nil {
		return err
	}
	pending = append(pending, records...)
	count = addAgentStatistic(count, estimateAgentRecords(pending))
	result.ContextEstimate = &count
	return nil
}

// Raw canonical estimation intentionally differs from the outgoing chat budget:
// it counts stored tool payloads and preserves unknown provider measurements.
func estimateAgentRecords(records []canonical.Record) uint64 {
	var total uint64
	for _, record := range records {
		message := record.MutableMessage()
		if message == nil {
			continue
		}
		count := uint64(7)
		for _, block := range message.Content {
			switch value := block.Data.(type) {
			case *canonical.TextContent:
				count = addAgentStatistic(count, tokens.Text(value.Text))
			case *canonical.ToolRequest:
				count = addAgentStatistic(count, tokens.Text(value.ID+":"+value.Name+":"+agentStatisticDebug(agentStatisticJSON(value.Arguments))))
			case *canonical.ToolResponse:
				var body bytes.Buffer
				encoder := json.NewEncoder(&body)
				encoder.SetEscapeHTML(false)
				_ = encoder.Encode(agentStatisticJSON(value.Result.Value))
				count = addAgentStatistic(count, tokens.Text(strings.TrimSuffix(body.String(), "\n")))
			}
		}
		total = addAgentStatistic(total, count)
	}
	return total
}

func agentStatisticJSON(raw json.RawMessage) any {
	var value any
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.UseNumber()
	_ = decoder.Decode(&value)
	return value
}

func agentStatisticDebug(value any) string {
	switch v := value.(type) {
	case nil:
		return "Null"
	case bool:
		return fmt.Sprintf("Bool(%t)", v)
	case string:
		return "String(" + strconv.Quote(v) + ")"
	case json.Number:
		return "Number(" + string(v) + ")"
	case []any:
		parts := make([]string, len(v))
		for i, item := range v {
			parts[i] = agentStatisticDebug(item)
		}
		return "Array [" + strings.Join(parts, ", ") + "]"
	case map[string]any:
		keys := make([]string, 0, len(v))
		for key := range v {
			keys = append(keys, key)
		}
		slices.Sort(keys)
		parts := make([]string, 0, len(keys))
		for _, key := range keys {
			parts = append(parts, strconv.Quote(key)+": "+agentStatisticDebug(v[key]))
		}
		return "Object {" + strings.Join(parts, ", ") + "}"
	}
	panic("unsupported canonical JSON value")
}
