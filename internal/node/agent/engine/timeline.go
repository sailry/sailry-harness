package engine

import (
	"slices"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/clock"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A later phase wins an instant several intervals share.
type timelinePhase uint8

const (
	phaseWaiting timelinePhase = iota
	phaseOutput
	phaseTool
	// phaseExcluded is time the turn spends waiting for somebody else: the
	// user's answer or decision, or a subagent.
	phaseExcluded
)

// A tool call owns its interval and the waits it starts. A model request has
// no owner.
type timelineSpan struct {
	from, to uint64
	phase    timelinePhase
	owner    string
}

// timeline collects a turn's execution intervals; its owner reduces them once
// when the turn ends. A turn run outside its owner has none and records
// nothing.
type timeline struct {
	clock  clock.Clock
	origin time.Time
	mu     sync.Mutex
	spans  []timelineSpan
}

func newTimeline(clock clock.Clock) *timeline {
	return &timeline{clock: clock, origin: clock.Now()}
}

func (t *timeline) record(start, end time.Time, phase timelinePhase, owner string) {
	from := timelineMicros(start.Sub(t.origin))
	t.mu.Lock()
	t.spans = append(t.spans, timelineSpan{from: from, to: max(from, timelineMicros(end.Sub(t.origin))), phase: phase, owner: owner})
	t.mu.Unlock()
}

// span opens an interval its returned function closes.
func (t *timeline) span(phase timelinePhase, owner string) func() {
	if t == nil {
		return func() {}
	}
	started := t.clock.Now()
	return func() { t.record(started, t.clock.Now(), phase, owner) }
}

// tool opens a call's interval. A delegating call waits for its subagent.
func (t *timeline) tool(call canonical.ToolRequest, delegated bool) func() {
	if delegated {
		return t.span(phaseExcluded, call.ID)
	}
	return t.span(phaseTool, call.ID)
}

// attempt records one model request. Waiting ends at the first token;
// streamed reasoning, text and tool calls all count as output.
func (t *timeline) attempt(attempt model.Attempt) {
	if t == nil {
		return
	}
	first := attempt.Finished
	if attempt.FirstToken != nil && attempt.FirstToken.Before(first) {
		first = *attempt.FirstToken
	}
	t.record(attempt.Started, first, phaseWaiting, "")
	if first.Before(attempt.Finished) {
		t.record(first, attempt.Finished, phaseOutput, "")
	}
}

func (t *timeline) summary() protocol.ConversationObservationPhases {
	t.mu.Lock()
	defer t.mu.Unlock()
	return reducePhases(timelineMicros(t.clock.Now().Sub(t.origin)), t.spans)
}

func timelineMicros(value time.Duration) uint64 {
	return uint64(max(0, value.Microseconds()))
}

// reducePhases gives every instant of the turn to one phase. An excluded
// interval removes its owner's other intervals for as long as it lasts, and
// an instant left with nothing but excluded intervals is not active time.
func reducePhases(duration uint64, spans []timelineSpan) protocol.ConversationObservationPhases {
	points := []uint64{0, duration}
	for _, span := range spans {
		points = append(points, min(span.from, duration), min(span.to, duration))
	}
	slices.Sort(points)
	points = slices.Compact(points)
	var active, waiting, output, tools, other uint64
	for i := 1; i < len(points); i++ {
		start, end := points[i-1], points[i]
		var phase *timelinePhase
		excluded := false
		for _, span := range spans {
			if span.from >= end || span.to <= start {
				continue
			}
			if span.phase == phaseExcluded {
				excluded = true
				continue
			}
			if slices.ContainsFunc(spans, func(wait timelineSpan) bool {
				return wait.phase == phaseExcluded && wait.owner == span.owner && wait.from < end && wait.to > start
			}) {
				continue
			}
			if phase == nil || span.phase > *phase {
				phase = &span.phase
			}
		}
		if phase == nil && excluded {
			continue
		}
		active += end - start
		switch {
		case phase == nil:
			other += end - start
		case *phase == phaseWaiting:
			waiting += end - start
		case *phase == phaseOutput:
			output += end - start
		default:
			tools += end - start
		}
	}
	return protocol.ConversationObservationPhases{ActiveUs: &active, WaitingUs: &waiting, OutputUs: &output, ToolsUs: &tools, OtherUs: &other}
}

// phasesRecord is the hidden record that carries a finished turn's phases to
// the session's statistics.
func (r *Runtime) phasesRecord(x *execution) canonical.Record {
	record := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant), r.clock.Now())
	record.Provenance.Author = "system"
	phases := x.timeline.summary()
	record.Metadata.Execution = &canonical.Execution{Phases: &phases}
	runtime := canonical.DefaultRuntime()
	runtime.UserVisible = false
	runtime.AgentVisible = false
	record.Metadata.Runtime = &runtime
	return record
}
