package store

import (
	"cmp"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"
	"math/bits"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func addAgentStatistic(a, b uint64) uint64 {
	if math.MaxUint64-a < b {
		return math.MaxUint64
	}
	return a + b
}

func sumAgentStatistic(a, b *uint64) *uint64 {
	if a == nil || b == nil {
		return nil
	}
	value := addAgentStatistic(*a, *b)
	return &value
}

type agentStatisticMean struct{ high, low, count uint64 }

func (m *agentStatisticMean) observe(value *uint64) {
	if value != nil {
		var carry uint64
		m.low, carry = bits.Add64(m.low, *value, 0)
		m.high += carry
		m.count++
	}
}

func (m agentStatisticMean) finish() *uint64 {
	if m.count == 0 {
		return nil
	}
	value, _ := bits.Div64(m.high, m.low, m.count)
	return &value
}

type agentStatisticLatency struct{ response, first, waiting, elapsed agentStatisticMean }

func (l *agentStatisticLatency) observe(a protocol.ConversationObservationAttempt) {
	response := a.ResponseUs
	if response == nil && a.Transport == "http" {
		response = a.HeadersUs
	}
	if response != nil && a.ElapsedUs != nil && *response > *a.ElapsedUs {
		response = nil
	}
	l.response.observe(response)
	l.first.observe(a.FirstTokenUs)
	l.elapsed.observe(a.ElapsedUs)
	if response != nil && a.FirstTokenUs != nil && *a.FirstTokenUs >= *response {
		waiting := *a.FirstTokenUs - *response
		l.waiting.observe(&waiting)
	}
}

type agentStatisticCall struct {
	sequence uint64
	request  protocol.ConversationObservationRequest
}

const agentStatisticCallsQuery = `WITH rounds AS (
 SELECT t.id,row_number() OVER (ORDER BY t.rowid) AS turn_index
 FROM conversation_turns membership JOIN turns t ON t.id=membership.turn
 WHERE membership.session=?1 AND t.kind='task' AND EXISTS(SELECT 1 FROM agent_events event WHERE event.turn=t.id))
 SELECT h.sequence,json_extract(e.body,'$.metadata.execution.observation'),
 (SELECT json_extract(model.value,'$.context') FROM agent_runs r
  JOIN turns t ON t.id=r.turn JOIN session_revisions revision ON revision.session=t.session AND revision.revision=t.revision,
  json_each(r.provider,'$.models') model WHERE r.turn=e.turn AND json_extract(model.value,'$.id')=json_extract(revision.config,'$.model') LIMIT 1),
 (SELECT turn_index FROM rounds WHERE rounds.id=e.turn)
 FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
 WHERE h.session=?1 AND json_type(e.body,'$.metadata.execution.observation')='object' ORDER BY h.sequence`

func agentStatisticCalls(ctx context.Context, db *sql.Conn, session protocol.SessionID) ([]agentStatisticCall, error) {
	rows, err := db.QueryContext(ctx, agentStatisticCallsQuery, session.String())
	if err != nil {
		return nil, err
	}
	result := []agentStatisticCall{}
	indices := map[string]int{}
	for rows.Next() {
		var sequence uint64
		var body []byte
		var limit, turnIndex sql.NullInt64
		if err = rows.Scan(&sequence, &body, &limit, &turnIndex); err != nil {
			rows.Close()
			return nil, err
		}
		var request protocol.ConversationObservationRequest
		if err = json.Unmarshal(body, &request); err != nil {
			rows.Close()
			return nil, err
		}
		request.ContextLimit, request.TurnIndex = nil, nil
		if limit.Valid {
			value := uint32(limit.Int64)
			request.ContextLimit = &value
		}
		if turnIndex.Valid {
			value := uint64(turnIndex.Int64)
			request.TurnIndex = &value
		}
		index, exists := indices[request.ID]
		if !exists {
			index = len(result)
			indices[request.ID] = index
			capture := request
			capture.Attempts = nil
			result = append(result, agentStatisticCall{sequence, capture})
		}
		current := &result[index].request
		for _, attempt := range request.Attempts {
			if i := slices.IndexFunc(current.Attempts, func(previous protocol.ConversationObservationAttempt) bool { return previous.Index == attempt.Index }); i >= 0 {
				if current.Attempts[i].ElapsedUs == nil || attempt.ElapsedUs != nil {
					current.Attempts[i] = attempt
				}
			} else {
				current.Attempts = append(current.Attempts, attempt)
			}
		}
		slices.SortFunc(current.Attempts, func(a, b protocol.ConversationObservationAttempt) int { return cmp.Compare(a.Index, b.Index) })
	}
	return result, errors.Join(rows.Err(), rows.Close())
}

// Lightweight JSON selects avoid retaining conversation or tool result bodies.
const agentStatisticEventsQuery = `SELECT e.turn,json_object(
 'kind',json_extract(e.body,'$.kind'),'phases',json_extract(e.body,'$.metadata.execution.phases'),
 'usage',json(coalesce(json_extract(e.body,'$.usage'),json_extract(e.body,'$.summary.usage'))),
 'has_reply',json_extract(e.body,'$.role')='assistant' AND EXISTS(
  SELECT 1 FROM json_each(e.body,'$.content') p WHERE length(json_extract(p.value,'$.text'))>0),
 'tools',json((SELECT json_group_array(json_extract(p.value,'$.name')) FROM json_each(e.body,'$.content') p
  WHERE json_extract(p.value,'$.type')='tool_request' AND json_type(p.value,'$.name')='text' AND json_type(p.value,'$.arguments')='object'))),
 EXISTS(SELECT 1 FROM conversation_events membership WHERE membership.session=?1 AND membership.sequence=e.sequence)
 FROM agent_events e WHERE EXISTS(SELECT 1 FROM conversation_turns h WHERE h.session=?1 AND h.turn=e.turn) ORDER BY e.sequence`

func agentStatisticsObservations(ctx context.Context, db *sql.Conn, session protocol.SessionID) (protocol.ConversationObservationSummary, error) {
	result := protocol.DefaultConversationObservationSummary()
	calls, err := agentStatisticCalls(ctx, db, session)
	if err != nil {
		return result, err
	}
	rows, err := db.QueryContext(ctx, agentStatisticEventsQuery, session.String())
	if err != nil {
		return result, err
	}
	phases := map[string]protocol.ConversationObservationPhases{}
	replies := map[string]bool{}
	var usageCount, cacheCount, reasoningCount, cacheTotal, reasoningTotal uint64
	for rows.Next() {
		var turn string
		var body []byte
		var visible bool
		if err = rows.Scan(&turn, &body, &visible); err != nil {
			rows.Close()
			return result, err
		}
		var event struct {
			Kind     string                                  `json:"kind"`
			Phases   *protocol.ConversationObservationPhases `json:"phases"`
			Usage    *canonical.Usage                        `json:"usage"`
			HasReply int                                     `json:"has_reply"`
			Tools    []string                                `json:"tools"`
		}
		if err = json.Unmarshal(body, &event); err != nil {
			rows.Close()
			return result, err
		}
		if event.Phases != nil {
			phases[turn] = *event.Phases
		}
		if usage := event.Usage; usage != nil {
			usageCount++
			if usage.CacheRead != nil {
				cacheCount++
				cacheTotal = addAgentStatistic(cacheTotal, *usage.CacheRead)
			}
			if usage.Reasoning != nil {
				reasoningCount++
				reasoningTotal = addAgentStatistic(reasoningTotal, *usage.Reasoning)
			}
		}
		if !visible {
			continue
		}
		if event.Kind == "compaction" {
			result.Compactions++
		}
		if event.HasReply == 1 {
			replies[turn] = true
		}
		for _, tool := range event.Tools {
			result.Tools[tool]++
		}
	}
	if err = errors.Join(rows.Err(), rows.Close()); err != nil {
		return result, err
	}
	result.Replies = uint64(len(replies))
	if usageCount > 0 && cacheCount == usageCount {
		result.CacheRead = &cacheTotal
	}
	if usageCount > 0 && reasoningCount == usageCount {
		result.Reasoning = &reasoningTotal
	}
	first := true
	for _, phase := range phases {
		if first {
			result.Phases, first = phase, false
			continue
		}
		result.Phases.ActiveUs = sumAgentStatistic(result.Phases.ActiveUs, phase.ActiveUs)
		result.Phases.WaitingUs = sumAgentStatistic(result.Phases.WaitingUs, phase.WaitingUs)
		result.Phases.OutputUs = sumAgentStatistic(result.Phases.OutputUs, phase.OutputUs)
		result.Phases.ToolsUs = sumAgentStatistic(result.Phases.ToolsUs, phase.ToolsUs)
		result.Phases.OtherUs = sumAgentStatistic(result.Phases.OtherUs, phase.OtherUs)
	}
	rows, err = db.QueryContext(ctx, `SELECT h.turn FROM conversation_turns h JOIN agent_runs r ON r.turn=h.turn
 WHERE h.session=?1 AND json_extract(r.body,'$.status') IN ('completed','failed','cancelled','interrupted')`, session.String())
	if err != nil {
		return result, err
	}
	for rows.Next() {
		var turn string
		if err = rows.Scan(&turn); err != nil {
			rows.Close()
			return result, err
		}
		if _, exists := phases[turn]; !exists {
			result.Phases = protocol.DefaultConversationObservationPhases()
		}
	}
	if err = errors.Join(rows.Err(), rows.Close()); err != nil {
		return result, err
	}
	if err = db.QueryRowContext(ctx, `SELECT count(DISTINCT d.turn) FROM agent_delegations d JOIN conversation_turns h ON h.turn=d.parent_turn WHERE h.session=?1`, session.String()).Scan(&result.Children); err != nil {
		return result, err
	}
	result.Calls = uint64(len(calls))
	var latency agentStatisticLatency
	composition := map[protocol.ConversationObservationCategoryKind]protocol.ConversationObservationCategory{}
	for _, call := range calls {
		for _, attempt := range call.request.Attempts {
			latency.observe(attempt)
			if attempt.ElapsedUs != nil {
				result.ModelUs = addAgentStatistic(result.ModelUs, *attempt.ElapsedUs)
			}
			if attempt.ReasoningReported && attempt.Usage != nil && attempt.ElapsedUs != nil && attempt.FirstTokenUs != nil && *attempt.ElapsedUs > *attempt.FirstTokenUs {
				result.OutputUs = addAgentStatistic(result.OutputUs, *attempt.ElapsedUs-*attempt.FirstTokenUs)
				output := attempt.Usage.Output
				if !attempt.ReasoningStreamed {
					output -= min(output, attempt.Usage.Reasoning)
				}
				result.OutputTokens = addAgentStatistic(result.OutputTokens, output)
				result.OutputSamples++
			}
			for _, row := range attempt.Categories {
				total, exists := composition[row.Kind]
				if !exists {
					items, tokens := uint64(0), uint64(0)
					total = protocol.ConversationObservationCategory{Kind: row.Kind, Items: &items, Tokens: &tokens, Method: row.Method}
				}
				total.Items = sumAgentStatistic(total.Items, row.Items)
				total.Tokens = sumAgentStatistic(total.Tokens, row.Tokens)
				composition[row.Kind] = total
			}
		}
	}
	result.Latency = protocol.ConversationObservationLatency{ResponseUs: latency.response.finish(), FirstTokenUs: latency.first.finish(), WaitingUs: latency.waiting.finish(), ElapsedUs: latency.elapsed.finish()}
	for _, kind := range []protocol.ConversationObservationCategoryKind{"system", "tools", "user", "skills", "injected", "assistant", "results"} {
		if category, exists := composition[kind]; exists {
			result.Composition = append(result.Composition, category)
		}
	}
	rounds := map[protocol.TurnID]agentStatisticCall{}
	for _, call := range calls {
		attempts := call.request.Attempts
		if len(attempts) > 0 {
			call.request.Attempts = []protocol.ConversationObservationAttempt{attempts[len(attempts)-1]}
		}
		rounds[call.request.Turn] = call
	}
	recent := make([]agentStatisticCall, 0, len(rounds))
	for _, call := range rounds {
		recent = append(recent, call)
	}
	slices.SortFunc(recent, func(a, b agentStatisticCall) int { return cmp.Compare(a.sequence, b.sequence) })
	for _, call := range recent[max(0, len(recent)-16):] {
		result.Recent = append(result.Recent, call.request)
	}
	return result, nil
}
