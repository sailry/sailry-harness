package live

import (
	"crypto/sha256"
	"encoding/json"
	"math"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Demo mode shows the statistics package over a demo conversation as over a
// session: its figures in the composer's context bar and its context panel
// in the header. The package's view is mounted for each demo conversation
// shown, without a session, under the presentation "demo/<id>", and reads
// the composer state the host gives it (readComposer), which is built from
// the conversation's own usage and transcript. No other package is mounted there: theirs
// would ask the Node. Nothing of the demo is sent to a Node; the package's
// bundle is the one another of its mounts read, and is read from the Node
// only where none has it yet.
const (
	pluginStatisticsPackage = "statistics"
	pluginDemoPresentation  = "demo/"
)

// demoPluginRegistry is the contributions shown over a demo conversation.
func (r *Runtime) demoPluginRegistry(cv *viewmodel.Conversation) *plugins.Registry {
	scope := protocol.DefaultPluginContext()
	scope.Surface.Kind = "composer"
	return r.localRuntime().pluginRegistry(scope, pluginDemoPresentation+cv.ID)
}

// demoConversation makes a demo conversation's figures the statistics
// package's (shell.Hooks.DemoConversation), and notes the model its composer
// is set to for the composer state its mount reads.
func (r *Runtime) demoConversation(_ *ui.Context, cv *viewmodel.Conversation, comp *composer.State) {
	root := r.localRuntime()
	root.demoModels.Store(cv.ID, comp.Model)
	if registry := root.demoPluginRegistry(cv); registry != nil {
		root.projectPluginMetrics(comp, registry)
	}
}

// refreshDemoComposer publishes the composer state a demo conversation's
// mount reads, and reports whether the mount is one. The demo has no model
// context: its provider is no provider of the Node.
func (state *pluginNativeState) refreshDemoComposer() bool {
	id, demo := strings.CutPrefix(state.presentation, pluginDemoPresentation)
	if !demo || state.scope.Session != nil {
		return false
	}
	var projection map[string]any
	var failure error
	root := state.runtime.localRuntime()
	if view := root.App.DemoConversation(id); view == nil {
		failure = pluginNativeFault(protocol.ErrorCodeUnavailable, "composer source is unavailable")
	} else {
		model, _ := root.demoModels.Load(id)
		selected, _ := model.(string)
		projection, failure = demoComposer(view, selected, root.App.NavShown())
	}
	state.composer.mu.Lock()
	publishNativeProjection(&state.composer.view, projection, failure)
	publishNativeProjection(&state.composer.model, nil, pluginNativeFault(protocol.ErrorCodePermissionDenied, "model context is unavailable"))
	state.composer.mu.Unlock()
	return true
}

// revealDemoTurn scrolls a demo conversation's transcript to the turn its
// statistics name. The transcript is the demo's own, whole in memory, so
// nothing is asked of a Node.
func (r *Runtime) revealDemoTurn(id string, target protocol.TurnID) {
	root := r.localRuntime()
	view := root.App.DemoConversation(id)
	if view == nil {
		return
	}
	for index, turn := range view.Turns {
		if demoTurnID(turn.ID) == target {
			root.App.DemoReveal(id, index)
			return
		}
	}
}

// demoComposer is the composer state of a demo conversation in the shape
// projectComposer gives a session's: connected, idle, set to model, with the
// statistics a session of the conversation's usage and transcript records.
func demoComposer(view *viewmodel.Conversation, model string, sidebar bool) (map[string]any, error) {
	recorded := demoStatistics(view, model)
	statistics, err := plugins.ComposerStatistics(recorded)
	if err != nil {
		return nil, err
	}
	var limit *uint32
	if recorded != nil {
		limit = recorded.ContextLimit
	}
	var selected *string
	if model != "" {
		selected = &model
	}
	return map[string]any{
		"command": nil, "mode": protocol.WorkModeCode, "mode_selection": nil, "modes": []any{},
		"session": nil, "connected": true, "connection": map[string]any{"route": "local", "rtt_ms": nil},
		"busy": false, "configuring": false, "readonly": false,
		"model_tools": true, "model": selected, "context_limit": limit, "statistics": statistics,
		"compacting": false, "has_messages": len(view.Turns) > 0, "sidebar": sidebar,
		"options": map[string]json.RawMessage{}, "operations": []any{},
	}, nil
}

// demoStatistics are the statistics a session records, written for a demo
// conversation from what it shows: its usage, its transcript and the model
// its composer is set to. Every value follows from those by fixed rules, so
// the same conversation always reads the same and the parts add up:
//
//   - the tokens split into input and output at demoOutputShare, the cached
//     part of the input, three tenths of the output as reasoning, the cost
//     with every response priced, the speed as the output over its time;
//   - the context used of the window, the model's compaction budget, and one
//     request a replying turn whose context grows to the context used, each
//     with the categories its tokens divide into (demoCategories);
//   - the time to the first token and the request times, and the phases of
//     the conversation's active time, which is at least what its replies took;
//   - the tool calls its transcript shows by name, its subagents with their
//     share of tokens and cost (demoDelegated), and its compactions.
//
// A conversation without usage has none.
func demoStatistics(view *viewmodel.Conversation, model string) *protocol.ConversationStatistics {
	usage := view.Session.Usage
	if usage == nil {
		return nil
	}
	statistics := protocol.DefaultConversationStatistics()
	observed := &statistics.Observations
	count := func(value float64) uint64 { return uint64(math.Max(0, math.Round(value))) }
	turns := count(float64(usage.Turns))
	statistics.Turns, statistics.Responses, observed.Calls = turns, turns, turns
	tokens := count(float64(usage.Tokens))
	output := tokens / demoOutputShare
	input := tokens - output
	cached := min(count(float64(usage.Cached)), input)
	reasoning := output * 3 / 10
	statistics.Usage = &protocol.ConversationUsage{Input: input, Output: output, CachedInput: cached, Reasoning: reasoning}
	observed.CacheRead, observed.Reasoning = &cached, &reasoning
	micros := count(usage.Cost * 1_000_000)
	statistics.Cost = &protocol.UsageCost{UsdMicros: micros, Responses: turns}
	var generating uint64
	if usage.Speed > 0 && output > 0 {
		generating = count(float64(output) * 1_000_000 / usage.Speed)
		statistics.Generation = &protocol.UsageGeneration{ElapsedUs: generating, OutputTokens: output, Responses: turns}
		observed.OutputUs, observed.OutputTokens, observed.OutputSamples = generating, output-reasoning, turns
	}

	// The time to the first token is a few hundred milliseconds, the response
	// headers arrive within two fifths of it, and a request then takes its
	// share of the generation.
	var first, response, elapsed uint64
	if turns > 0 {
		first = (420 + 90*(turns%4)) * 1000
		response = first * 2 / 5
		elapsed = first + generating/turns
		statistics.FirstTokenUs = &first
		observed.Latency = protocol.ConversationObservationLatency{ResponseUs: &response, FirstTokenUs: &first, WaitingUs: &first, ElapsedUs: &elapsed}
	}

	var replies []conversation.Turn
	var taken time.Duration
	for index, turn := range view.Turns {
		if state := turn.Compaction; state != nil {
			taken += state.Elapsed
			if state.State == conversation.Compacted {
				observed.Compactions++
			}
			if state.State == conversation.Compacted || state.State == conversation.CompactionFailed {
				record := protocol.DefaultConversationCompactionRecord()
				record.ID, record.Turn = pluginDemoPresentation+turn.ID, demoTurnID(turn.ID)
				record.Sequence, record.TurnSequence = uint64(index+1), uint64(index+1)
				record.TimestampMs = demoMillis(turn.Sent)
				record.Outcome = protocol.ConversationCompactionOutcome{Kind: "succeeded"}
				if state.State == conversation.CompactionFailed {
					failure := protocol.DefaultConversationCompactionFailure()
					failure.Source, failure.Stage = protocol.ConversationCompactionSourceAutomatic, protocol.ConversationCompactionStageSummary
					failure.Reason = protocol.ConversationCompactionReasonSummaryTooLarge
					if state.Cause != "" {
						failure.Cause = &state.Cause
					}
					record.Outcome = protocol.ConversationCompactionOutcome{Kind: "failed", Data: &failure}
				}
				statistics.LastCompaction = &record
			}
			continue
		}
		taken += turn.Reply.Elapsed
		replies = append(replies, turn)
		if slices.ContainsFunc(turn.Reply.Blocks, func(block conversation.Block) bool { return block.Kind == conversation.TextBlock }) {
			observed.Replies++
		}
	}
	observed.Tools = demoTools(view.Turns)
	var calls uint64
	for _, called := range observed.Tools {
		calls += called
	}

	// Waiting and output are the requests'; a tool call takes 1.6 s; the rest
	// of what the replies took is other work, a twenty-fifth at least.
	waiting, working := first*turns, calls*1_600_000
	other := max(uint64(max(0, taken.Microseconds())), waiting+generating+working) - (waiting + generating + working)
	other = max(other, (waiting+generating+working)/25)
	active := waiting + generating + working + other
	observed.Phases = protocol.ConversationObservationPhases{ActiveUs: &active, WaitingUs: &waiting, OutputUs: &generating, ToolsUs: &working, OtherUs: &other}
	observed.ModelUs = waiting + generating

	if usage.Window > 0 {
		window := uint32(usage.Window)
		used := count(math.Max(0, math.Min(1, usage.Context)) * float64(usage.Window))
		statistics.ContextTokens, statistics.ContextLimit = &used, &window
		if model != "" {
			statistics.ContextModel = &model
			// Compaction at four fifths of the window, of which sixteen in a
			// hundred are kept for the answer.
			reserve := window * 4 / 25
			statistics.Budget = &protocol.ConversationCompactionBudget{
				Model: model, ContextLimit: window, TriggerTokens: uint64(window) * 4 / 5, OutputReserve: reserve,
				SummaryInput: uint64(window - reserve), SummaryOutput: reserve,
			}
		}
		if len(replies) > 0 {
			timestamp := demoMillis(replies[len(replies)-1].Sent)
			statistics.ContextTimestampMs = &timestamp
		}
		var results uint64
		for index, turn := range replies {
			for _, called := range demoTools([]conversation.Turn{turn}) {
				results += called
			}
			// The context grows evenly to what the conversation uses now.
			occupied := used * uint64(index+2) / uint64(len(replies)+1)
			written := min(output/uint64(len(replies)), occupied/4)
			sent := occupied - written
			request := protocol.DefaultConversationObservationRequest()
			request.ID, request.Turn = pluginDemoPresentation+turn.ID, demoTurnID(turn.ID)
			round := uint64(index + 1)
			request.TurnIndex, request.Model, request.ContextLimit, request.TimestampMs = &round, model, &window, demoMillis(turn.Sent)
			attempt := protocol.DefaultConversationObservationAttempt()
			attempt.Index, attempt.Transport, attempt.Outcome = 1, "http", protocol.ConversationObservationOutcomeSucceeded
			status := uint16(200)
			attempt.HeadersUs, attempt.ResponseUs, attempt.FirstTokenUs, attempt.ElapsedUs, attempt.Status = &response, &response, &first, &elapsed, &status
			attempt.Usage = &protocol.ConversationUsage{Input: sent, Output: written, Reasoning: written * 3 / 10}
			if input > 0 {
				attempt.Usage.CachedInput = sent * cached / input
			}
			attempt.Categories, attempt.CompleteContext = demoCategories(occupied, uint64(index), results), true
			attempt.CacheReported, attempt.ReasoningReported = true, true
			request.Attempts = []protocol.ConversationObservationAttempt{attempt}
			observed.Recent = append(observed.Recent, request)
			// The composition adds the requests' categories up, kind by kind.
			for at, category := range attempt.Categories {
				if at == len(observed.Composition) {
					items, tokens := uint64(0), uint64(0)
					observed.Composition = append(observed.Composition, protocol.ConversationObservationCategory{
						Kind: category.Kind, Items: &items, Tokens: &tokens, Method: category.Method})
				}
				*observed.Composition[at].Items += *category.Items
				*observed.Composition[at].Tokens += *category.Tokens
			}
		}
		// A session keeps its sixteen latest requests.
		observed.Recent = observed.Recent[max(0, len(observed.Recent)-16):]
	}
	observed.Children, statistics.Delegated = demoDelegated(view.Turns, statistics)
	return &statistics
}

// demoCategories divides a request's tokens into what a request carries, a
// row a kind as the Node counts them: the system prompt and the tool
// definitions, of a fixed size where the request has room for them, and the
// rest among the user's messages, the skills, the injected context, the
// assistant's earlier replies (none before the first) and the tool results
// (none without a call). The rows add up to tokens.
func demoCategories(tokens, earlier, results uint64) []protocol.ConversationObservationCategory {
	system, definitions := min(tokens*28/100, 6200), min(tokens*34/100, 8400)
	rest := tokens - system - definitions
	weights := []uint64{3, 1, 2, 0, 0}
	if earlier > 0 {
		weights[3] = 8
	}
	if results > 0 {
		weights[4] = 10
	}
	var whole uint64
	for _, weight := range weights {
		whole += weight
	}
	parts := make([]uint64, len(weights))
	for index, weight := range weights {
		parts[index] = rest * weight / whole
	}
	// What the division leaves over is the user's.
	parts[0] = rest - parts[1] - parts[2] - parts[3] - parts[4]
	counted, identified := "text_characters_quarter", "identified_source_characters_quarter"
	var categories []protocol.ConversationObservationCategory
	for _, row := range []struct {
		kind          protocol.ConversationObservationCategoryKind
		items, tokens uint64
		method        *string
	}{
		{protocol.ConversationObservationCategoryKindSystem, 1, system, &counted},
		{protocol.ConversationObservationCategoryKindTools, 24, definitions, &counted},
		{protocol.ConversationObservationCategoryKindUser, earlier + 1, parts[0], &counted},
		{protocol.ConversationObservationCategoryKindSkills, 1, parts[1], &identified},
		{protocol.ConversationObservationCategoryKindInjected, 2, parts[2], &identified},
		{protocol.ConversationObservationCategoryKindAssistant, earlier, parts[3], &counted},
		{protocol.ConversationObservationCategoryKindResults, results, parts[4], &counted},
	} {
		categories = append(categories, protocol.ConversationObservationCategory{Kind: row.kind, Items: &row.items, Tokens: &row.tokens, Method: row.method})
	}
	return categories
}

// demoDelegated counts a transcript's subagents and gives them their usage
// beside the session's own: a quarter of the session's tokens for a subagent
// that ran to an end, half of that for one that is running or was stopped,
// and nothing for one still queued, at the session's own price a token. A
// conversation without a started subagent has delegated nothing.
func demoDelegated(turns []conversation.Turn, own protocol.ConversationStatistics) (uint64, protocol.UsageMetrics) {
	delegated := protocol.DefaultUsageMetrics()
	var children, halves uint64
	for _, turn := range turns {
		for _, block := range turn.Reply.Blocks {
			for _, child := range block.Children {
				children++
				switch child.Status {
				case conversation.ChildQueued:
				case conversation.ChildCompleted, conversation.ChildFailed, conversation.ChildContinued:
					halves += 2
				default:
					halves++
				}
			}
		}
	}
	whole := own.Usage.Input + own.Usage.Output
	tokens := whole / 8 * halves
	if tokens == 0 {
		return children, delegated
	}
	output := tokens / demoOutputShare
	usage := protocol.ConversationUsage{Input: tokens - output, Output: output, Reasoning: output * 3 / 10}
	usage.CachedInput = usage.Input * own.Usage.CachedInput / own.Usage.Input
	delegated.Responses, delegated.Tokens = halves, &usage
	delegated.Cost = &protocol.UsageCost{UsdMicros: own.Cost.UsdMicros * tokens / whole, Responses: halves}
	if generation := own.Generation; generation != nil && generation.OutputTokens > 0 {
		delegated.Generation = &protocol.UsageGeneration{ElapsedUs: generation.ElapsedUs * output / generation.OutputTokens, OutputTokens: output, Responses: halves}
	}
	return children, delegated
}

// demoTurnID is the turn a demo conversation's statistics name for a turn
// of its transcript, the same for the same turn.
func demoTurnID(id string) protocol.TurnID {
	sum := sha256.Sum256([]byte("sailry/demo/turn/" + id))
	return protocol.TurnID(sum[:16])
}

// demoMillis is a transcript time as the statistics carry one; an unset time
// is none.
func demoMillis(at time.Time) int64 {
	if at.IsZero() {
		return 0
	}
	return at.UnixMilli()
}

// demoTools counts a transcript's tool calls by name, as a session's
// observations do. A question or a plan review is an interaction, not a call.
func demoTools(turns []conversation.Turn) map[string]uint64 {
	tools := map[string]uint64{}
	for _, turn := range turns {
		for _, block := range turn.Reply.Blocks {
			for _, call := range block.Calls {
				if call.Name != "" && !call.Terminal {
					tools[call.Name]++
				}
			}
		}
	}
	return tools
}

// demoOutputShare is the part of a sample usage's tokens that is output: one
// in eight, the rest input.
const demoOutputShare = 8
