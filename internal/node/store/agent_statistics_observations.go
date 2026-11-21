package store

import (
	"cmp"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"maps"
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

type agentStatisticLatency struct{ response, pairedResponse, first, waiting, elapsed agentStatisticMean }

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
		l.pairedResponse.observe(response)
		waiting := *a.FirstTokenUs - *response
		l.waiting.observe(&waiting)
	}
}

type agentStatisticCall struct {
	sequence uint64
	request  protocol.ConversationObservationRequest
}

const agentStatisticCallsQuery = `SELECT h.sequence,json_extract(e.body,'$.metadata.execution.observation'),e.turn
 FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
 WHERE h.session=?1 AND json_type(e.body,'$.metadata.execution.observation')='object' ORDER BY h.sequence`

const agentStatisticCallLimitsQuery = `SELECT t.id,
 (SELECT json_extract(model.value,'$.context') FROM json_each(r.provider,'$.models') model
  WHERE json_extract(model.value,'$.id')=json_extract(revision.config,'$.model') LIMIT 1)
 FROM json_each(?1) observed CROSS JOIN turns t ON t.id=observed.value
 CROSS JOIN agent_runs r ON r.turn=t.id
 CROSS JOIN session_revisions revision ON revision.session=t.session AND revision.revision=t.revision`

const agentStatisticRoundTurnsQuery = `SELECT t.id FROM conversation_turns membership JOIN turns t ON t.id=membership.turn
 WHERE membership.session=?1 AND t.kind='task' AND EXISTS(SELECT 1 FROM agent_events event WHERE event.turn=t.id)
 ORDER BY t.rowid`

type agentStatisticTurnMetadata struct {
	contextLimit sql.NullInt64
	turnIndex    uint64
}

func agentStatisticCalls(ctx context.Context, db *sql.Conn, session protocol.SessionID) ([]agentStatisticCall, error) {
	return readAgentStatisticCalls(ctx, db, session, nil)
}

func readAgentStatisticCalls(ctx context.Context, db *sql.Conn, session protocol.SessionID, observed *[]string) ([]agentStatisticCall, error) {
	rows, err := db.QueryContext(ctx, agentStatisticCallsQuery, session.String())
	if err != nil {
		return nil, err
	}
	result := []agentStatisticCall{}
	indices := map[string]int{}
	turns := map[string]agentStatisticTurnMetadata{}
	var observedTurns, callTurns []string
	for rows.Next() {
		var sequence uint64
		var body []byte
		var turn string
		if err = rows.Scan(&sequence, &body, &turn); err != nil {
			rows.Close()
			return nil, err
		}
		var request protocol.ConversationObservationRequest
		if err = json.Unmarshal(body, &request); err != nil {
			rows.Close()
			return nil, err
		}
		if _, exists := turns[turn]; !exists {
			turns[turn] = agentStatisticTurnMetadata{}
			observedTurns = append(observedTurns, turn)
		}
		index, exists := indices[request.ID]
		if !exists {
			index = len(result)
			indices[request.ID] = index
			capture := request
			capture.Attempts, capture.ContextLimit, capture.TurnIndex = nil, nil, nil
			result = append(result, agentStatisticCall{sequence, capture})
			callTurns = append(callTurns, turn)
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
	if err = errors.Join(rows.Err(), rows.Close()); err != nil {
		return nil, err
	}
	if observed != nil {
		*observed = observedTurns
	}
	if len(observedTurns) == 0 {
		return result, nil
	}
	if err = readAgentStatisticTurnMetadata(ctx, db, session, observedTurns, turns); err != nil {
		return nil, err
	}
	for i, turn := range callTurns {
		metadata := turns[turn]
		if metadata.contextLimit.Valid {
			value := uint32(metadata.contextLimit.Int64)
			result[i].request.ContextLimit = &value
		}
		if metadata.turnIndex != 0 {
			value := metadata.turnIndex
			result[i].request.TurnIndex = &value
		}
	}
	return result, nil
}

func readAgentStatisticTurnMetadata(ctx context.Context, db *sql.Conn, session protocol.SessionID, observedTurns []string, turns map[string]agentStatisticTurnMetadata) error {
	keys, err := json.Marshal(observedTurns)
	if err != nil {
		return err
	}
	rows, err := db.QueryContext(ctx, agentStatisticCallLimitsQuery, string(keys))
	if err != nil {
		return err
	}
	for rows.Next() {
		var turn string
		var limit sql.NullInt64
		if err = rows.Scan(&turn, &limit); err != nil {
			rows.Close()
			return err
		}
		turns[turn] = agentStatisticTurnMetadata{contextLimit: limit}
	}
	if err = errors.Join(rows.Err(), rows.Close()); err != nil {
		return err
	}

	rows, err = db.QueryContext(ctx, agentStatisticRoundTurnsQuery, session.String())
	if err != nil {
		return err
	}
	var index uint64
	for rows.Next() {
		var turn string
		if err = rows.Scan(&turn); err != nil {
			rows.Close()
			return err
		}
		index++
		if metadata, exists := turns[turn]; exists {
			metadata.turnIndex = index
			turns[turn] = metadata
		}
	}
	return errors.Join(rows.Err(), rows.Close())
}

// Lightweight JSON selects avoid retaining conversation or tool result bodies.
const agentStatisticEventsQuery = `SELECT e.turn,json_object(
 'kind',json_extract(e.body,'$.kind'),'phases',json_extract(e.body,'$.metadata.execution.phases'),
 'usage',json_remove(coalesce(json_extract(e.body,'$.usage'),json_extract(e.body,'$.summary.usage')),'$.provider'),
 'has_reply',json_extract(e.body,'$.role')='assistant' AND EXISTS(
  SELECT 1 FROM json_each(e.body,'$.content') p WHERE length(json_extract(p.value,'$.text'))>0),
 'tools',json((SELECT json_group_array(json_extract(p.value,'$.name')) FROM json_each(e.body,'$.content') p
  WHERE json_extract(p.value,'$.type')='tool_request' AND json_type(p.value,'$.name')='text' AND json_type(p.value,'$.arguments')='object'))),
 EXISTS(SELECT 1 FROM conversation_events membership WHERE membership.session=?1 AND membership.sequence=e.sequence)
 FROM json_each(?2) h CROSS JOIN agent_events e
 ON e.turn=json_extract(h.value,'$.turn') AND e.sequence>json_extract(h.value,'$.after') ORDER BY e.sequence`

type agentStatisticEventTotals struct {
	phases                                                   map[string]protocol.ConversationObservationPhases
	replies                                                  map[string]bool
	tools                                                    map[string]uint64
	compactions                                              uint64
	usageCount, cacheCount, reasoningCount, cache, reasoning uint64
}

func (totals *agentStatisticEventTotals) extend(ctx context.Context, db *sql.Conn, session protocol.SessionID, ranges []agentStatisticsRange, work *agentStatisticsWork) error {
	if len(ranges) == 0 {
		return nil
	}
	keys, err := json.Marshal(ranges)
	if err != nil {
		return err
	}
	rows, err := db.QueryContext(ctx, agentStatisticEventsQuery, session.String(), string(keys))
	if err != nil {
		return err
	}
	if totals.phases == nil {
		totals.phases = map[string]protocol.ConversationObservationPhases{}
		totals.replies = map[string]bool{}
		totals.tools = map[string]uint64{}
	} else {
		totals.phases = maps.Clone(totals.phases)
		totals.replies = maps.Clone(totals.replies)
		totals.tools = maps.Clone(totals.tools)
	}
	for rows.Next() {
		work.eventRows++
		var turn string
		var body []byte
		var visible bool
		if err = rows.Scan(&turn, &body, &visible); err != nil {
			rows.Close()
			return err
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
			return err
		}
		if event.Phases != nil {
			totals.phases[turn] = *event.Phases
		}
		if usage := event.Usage; usage != nil {
			totals.usageCount++
			if usage.CacheRead != nil {
				totals.cacheCount++
				totals.cache = addAgentStatistic(totals.cache, *usage.CacheRead)
			}
			if usage.Reasoning != nil {
				totals.reasoningCount++
				totals.reasoning = addAgentStatistic(totals.reasoning, *usage.Reasoning)
			}
		}
		if !visible {
			continue
		}
		if event.Kind == "compaction" {
			totals.compactions++
		}
		if event.HasReply == 1 {
			totals.replies[turn] = true
		}
		for _, tool := range event.Tools {
			totals.tools[tool]++
		}
	}
	return errors.Join(rows.Err(), rows.Close())
}

func (totals *agentStatisticEventTotals) apply(result *protocol.ConversationObservationSummary, turns map[string]agentStatisticsTurn) {
	result.Replies = uint64(len(totals.replies))
	result.Compactions = totals.compactions
	result.Children = 0
	result.Tools = totals.tools
	if result.Tools == nil {
		result.Tools = map[string]uint64{}
	}
	result.CacheRead, result.Reasoning = nil, nil
	if totals.usageCount > 0 && totals.cacheCount == totals.usageCount {
		result.CacheRead = &totals.cache
	}
	if totals.usageCount > 0 && totals.reasoningCount == totals.usageCount {
		result.Reasoning = &totals.reasoning
	}
	result.Phases = protocol.DefaultConversationObservationPhases()
	first := true
	for _, phase := range totals.phases {
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
	for id, turn := range turns {
		if turn.child {
			result.Children++
		}
		if _, exists := totals.phases[id]; turn.own && turn.terminal && !exists {
			result.Phases = protocol.DefaultConversationObservationPhases()
		}
	}
}

func agentStatisticsCallsSummary(calls []agentStatisticCall) protocol.ConversationObservationSummary {
	result := protocol.DefaultConversationObservationSummary()
	result.Calls = uint64(len(calls))
	var latency agentStatisticLatency
	composition := map[protocol.ConversationObservationCategoryKind]protocol.ConversationObservationCategory{}
	for _, call := range calls {
		for _, attempt := range call.request.Attempts {
			latency.observe(attempt)
			if attempt.ElapsedUs != nil {
				result.ModelUs = addAgentStatistic(result.ModelUs, *attempt.ElapsedUs)
			}
			if generation := attempt.Usage.DecodeGeneration(attempt.ElapsedUs, attempt.FirstTokenUs, attempt.ReasoningStreamed); generation != nil {
				result.OutputUs = addAgentStatistic(result.OutputUs, generation.ElapsedUs)
				result.OutputTokens = addAgentStatistic(result.OutputTokens, generation.OutputTokens)
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
	result.Latency = protocol.ConversationObservationLatency{ResponseUs: latency.response.finish(), PairedResponseUs: latency.pairedResponse.finish(), FirstTokenUs: latency.first.finish(), WaitingUs: latency.waiting.finish(), ElapsedUs: latency.elapsed.finish()}
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
	return result
}
