package live

import (
	"cmp"
	"fmt"
	"regexp"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The composer's context meter is the core's: the window's occupancy and the
// session's usage, read from the statistics the Node records for the
// session. Packages keep the composer's indicator slot for rings of their
// own (Context.Indicators).

// projectContextMeter gives a session's composer its context meter. A draft
// has no session and no meter.
func (r *Runtime) projectContextMeter(cv *viewmodel.Conversation, comp *composer.State, config protocol.SessionConfig) {
	comp.Indicator = nil
	session, exists := r.session(cv.ID)
	if !exists {
		return
	}
	history, loaded := r.conversationState(session.ID)
	if !loaded {
		return
	}
	compacting := slices.ContainsFunc(history.Snapshot.Drafts, func(draft protocol.ConversationDraft) bool {
		return draftPhase(draft.Parts) == conversation.Compacting
	}) || slices.ContainsFunc(history.Snapshot.Page.Runs, func(run protocol.ConversationRun) bool {
		return run.Kind == protocol.ConversationRunKindCompaction &&
			(run.Status == protocol.ConversationStatusQueued || run.Status == protocol.ConversationStatusRunning || run.Status == protocol.ConversationStatusStopping)
	})
	limit := uint32(0)
	for _, provider := range r.snapshot().Providers {
		if provider.ID != config.Provider {
			continue
		}
		for _, model := range provider.Models {
			if model.ID == config.Model {
				limit = model.Context
			}
		}
	}
	stats := contextStats(history.Snapshot.Statistics, config.Model, limit, compacting)
	if record := history.Snapshot.Statistics; record != nil && record.LastCompaction != nil && stats.LastCompaction != nil {
		target, revision := *record.LastCompaction, history.Snapshot.Page.Revision
		stats.LastCompaction.Reveal = func() {
			comp.Meter.Open = false
			if r.App.Messages.Reveal == nil {
				return
			}
			r.App.Messages.Reveal(cv.ID, conversation.Match{
				Turn: -1, Target: target.Turn.String(), TurnSequence: target.TurnSequence, Entry: target.ID, Revision: revision,
			}, func(index int, found bool) {
				if found {
					r.App.Messages.List(cv.ID).ScrollTo(index, ui.Center)
				}
			})
		}
	}
	comp.Indicator = func(c *ui.Context) { conversation.ContextMeter(c, &comp.Meter, stats) }
}

// contextStats are a session's statistics as the meter shows them. model and
// limit are the composer's selection: where the session's last request ran
// another model, its occupancy no longer applies and the meter shows the
// selected model's window unmeasured.
func contextStats(s *protocol.ConversationStatistics, model string, limit uint32, compacting bool) *conversation.ContextStats {
	d := &conversation.ContextStats{Model: model, Limit: uint64(limit), Compacting: compacting}
	if s == nil {
		return d
	}
	changed := model != "" && s.ContextModel != nil && model != *s.ContextModel
	if !changed {
		d.Tokens = s.ContextTokens
		if d.Tokens == nil {
			d.Tokens = s.ContextEstimate
		}
		d.Estimated = s.ContextTokens == nil && d.Tokens != nil
		if s.ContextModel != nil {
			d.Model = *s.ContextModel
		}
		if s.ContextLimit != nil {
			d.Limit = uint64(*s.ContextLimit)
		} else if d.Tokens != nil {
			d.Limit = 0
		}
	}
	if b := s.Budget; b != nil && b.Model == d.Model && uint64(b.ContextLimit) == d.Limit {
		d.Threshold, d.Reserve = b.TriggerTokens, uint64(b.OutputReserve)
	}
	o := s.Observations
	d.Compactions = o.Compactions
	d.Steps = contextSteps(o.Recent)
	d.Parts, d.Complete = contextAggregate(d.Steps)
	if r := s.LastCompaction; r != nil && r.ID != "" && r.TurnSequence > 0 {
		d.LastCompaction = &conversation.ContextCompaction{Succeeded: r.Outcome.Kind == "succeeded"}
	}

	turns := s.Turns
	d.Turns = &turns
	if s.Cost != nil && s.Cost.Responses == s.Responses {
		d.Cost = fmt.Sprintf("%d.%02d", s.Cost.UsdMicros/1_000_000, s.Cost.UsdMicros%1_000_000/10_000)
	}
	if u := s.Usage; u != nil {
		d.Input, d.Output = &u.Input, &u.Output
		if o.CacheRead != nil && s.IncompleteTokenResponses == 0 {
			d.Cached = &u.CachedInput
		}
	}
	if len(o.Recent) > 0 {
		attempts := o.Recent[len(o.Recent)-1].Attempts
		if len(attempts) > 0 {
			last := attempts[len(attempts)-1]
			if u := last.Usage; u != nil {
				d.CurrentInput = &u.Input
				if last.CacheReported {
					d.CurrentCached = &u.CachedInput
				}
			}
		}
	}
	d.Reasoning = o.Reasoning
	grouped := map[string]uint64{}
	var calls uint64
	for name, count := range o.Tools {
		grouped[contextToolName(name)] += count
		calls += count
	}
	for name, count := range grouped {
		d.Tools = append(d.Tools, conversation.ContextTool{Name: name, Count: count})
	}
	slices.SortFunc(d.Tools, func(a, b conversation.ContextTool) int {
		return cmp.Or(cmp.Compare(b.Count, a.Count), strings.Compare(a.Name, b.Name))
	})
	d.ToolCalls = &calls
	d.Children = o.Children
	if t := s.Delegated.Tokens; t != nil {
		d.Delegated = t.Input + t.Output
	}

	d.Active = o.Phases.ActiveUs
	for _, phase := range []struct {
		key   string
		value *uint64
		chart int
	}{
		{"meter_wait", o.Phases.WaitingUs, 2}, {"meter_reply", o.Phases.OutputUs, 5},
		{"meter_tools_time", o.Phases.ToolsUs, 3}, {"meter_other", o.Phases.OtherUs, -1},
	} {
		if phase.value != nil {
			d.Phases = append(d.Phases, conversation.ContextPhase{Key: phase.key, Micros: *phase.value, Chart: phase.chart})
		}
	}
	if g := s.Generation; g != nil && g.ElapsedUs > 0 {
		speed := float64(g.OutputTokens) * 1_000_000 / float64(g.ElapsedUs)
		d.Speed = &speed
	}
	d.PairedResponse, d.WaitingAfterResponse = o.Latency.PairedResponseUs, o.Latency.WaitingUs
	d.Response, d.FirstToken, d.Elapsed = o.Latency.ResponseUs, o.Latency.FirstTokenUs, o.Latency.ElapsedUs
	return d
}

// contextCategories are the context's categories in the meter's order, with
// their label keys and chart colors.
var contextCategories = []struct {
	kind  protocol.ConversationObservationCategoryKind
	key   string
	chart int
}{
	{"system", "meter_system", 4}, {"tools", "meter_tools", 1}, {"user", "meter_user", 3}, {"skills", "meter_skills", 5},
	{"injected", "meter_injected", 4}, {"assistant", "meter_assistant", 2}, {"results", "meter_results", 3},
}

// identifiedMethod is the estimate of the source text a category's content
// was identified as, which the details note.
const identifiedMethod = "identified_source_characters_quarter"

// contextSteps keeps each turn's latest request, never a sum of its
// requests, in the turns' order. A request without its turn or its place in
// it is left out rather than shown as an invented step.
func contextSteps(recent []protocol.ConversationObservationRequest) []conversation.ContextStep {
	turns := map[string]int{}
	var steps []conversation.ContextStep
	for _, request := range recent {
		if request.Turn == (protocol.TurnID{}) || request.TurnIndex == nil || *request.TurnIndex < 1 {
			continue
		}
		step := conversation.ContextStep{ID: request.Turn.String(), Step: *request.TurnIndex, Model: request.Model}
		if request.ContextLimit != nil {
			step.Limit = uint64(*request.ContextLimit)
		}
		if len(request.Attempts) > 0 {
			attempt := request.Attempts[len(request.Attempts)-1]
			step.Complete = attempt.CompleteContext
			step.FirstTokenUs = attempt.FirstTokenUs
			if generation := attempt.Usage.DecodeGeneration(attempt.ElapsedUs, attempt.FirstTokenUs, attempt.ReasoningStreamed); generation != nil {
				rate := float64(generation.OutputTokens) * 1_000_000 / float64(generation.ElapsedUs)
				step.Speed = &rate
			}
			whole, known := uint64(0), true
			for _, category := range contextCategories {
				part := conversation.ContextPart{Key: category.key, Chart: category.chart}
				for _, value := range attempt.Categories {
					if value.Kind == category.kind {
						part.Tokens = value.Tokens
						part.Identified = value.Method != nil && *value.Method == identifiedMethod
					}
				}
				if part.Tokens == nil {
					known = false
				} else {
					whole += *part.Tokens
				}
				step.Parts = append(step.Parts, part)
			}
			if u := attempt.Usage; u != nil {
				total := u.Input + u.Output
				step.Tokens = &total
			} else if attempt.CompleteContext && known {
				step.Tokens, step.Estimated = &whole, true
			}
		}
		if at, seen := turns[step.ID]; seen {
			steps[at] = step
			continue
		}
		turns[step.ID] = len(steps)
		steps = append(steps, step)
	}
	slices.SortStableFunc(steps, func(a, b conversation.ContextStep) int { return cmp.Compare(a.Step, b.Step) })
	return steps
}

// contextAggregate sums the steps' categories, a category unknown where a
// step does not know it, and reports whether every step saw its whole
// context.
func contextAggregate(steps []conversation.ContextStep) ([]conversation.ContextPart, bool) {
	if len(steps) == 0 {
		return nil, false
	}
	complete := true
	parts := make([]conversation.ContextPart, len(contextCategories))
	for i, category := range contextCategories {
		parts[i] = conversation.ContextPart{Key: category.key, Chart: category.chart, Identified: true}
		var sum uint64
		known := true
		for _, step := range steps {
			if i >= len(step.Parts) || step.Parts[i].Tokens == nil {
				known = false
				continue
			}
			sum += *step.Parts[i].Tokens
			parts[i].Identified = parts[i].Identified && step.Parts[i].Identified
		}
		if known {
			parts[i].Tokens = &sum
		}
	}
	for _, step := range steps {
		complete = complete && step.Complete
	}
	return parts, complete
}

// Model-facing tool aliases carry package and MCP digests; the meter counts
// a tool by its own name.
var (
	pluginToolAlias = regexp.MustCompile(`^plugin_[0-9a-f]{16}_(.+)$`)
	mcpToolAlias    = regexp.MustCompile(`^mcp_(.+)_[0-9a-f]{24}$`)
)

func contextToolName(name string) string {
	for _, alias := range []*regexp.Regexp{pluginToolAlias, mcpToolAlias} {
		if m := alias.FindStringSubmatch(name); m != nil {
			return m[1]
		}
	}
	return name
}
