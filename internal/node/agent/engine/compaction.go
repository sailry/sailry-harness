// Adapted from Goose ee8500d5 and 14741e9a (Apache-2.0).
// See ../model/LICENSE.goose.
package engine

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"slices"
	"strings"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	handoff "github.com/sailry/sailry-harness/internal/node/agent/compact"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/output"
	"github.com/sailry/sailry-harness/internal/node/agent/prompt"
	"github.com/sailry/sailry-harness/internal/node/agent/tokens"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func capturedModel(binding model.Binding) protocol.ConversationModel {
	for _, candidate := range binding.Provider.Models {
		if candidate.ID == binding.Config.Model {
			return candidate
		}
	}
	// An unlisted model declares no capabilities; only its window is known.
	selected := protocol.DefaultConversationModel()
	selected.ID, selected.Context = binding.Config.Model, model.ContextLimit(binding)
	return selected
}
func contextTokens(records []canonical.Record) uint64 {
	start := 0
	for i, record := range records {
		if record.Kind == "compaction" {
			start = i
		}
	}
	for i := len(records) - 1; i >= start; i-- {
		message := records[i].MutableMessage()
		if message != nil && message.Usage != nil {
			if !compactionUsage(records[i]) && message.Usage.Total != nil {
				used := *message.Usage.Total
				if message.Usage.Input != nil && message.Usage.Output != nil && message.Usage.Reasoning != nil &&
					!hasSignedReasoning(message) && *message.Usage.Reasoning <= *message.Usage.Output {
					// Reasoning tokens that are not replayed back into the next request do not consume the next prompt window.
					used = *message.Usage.Input + (*message.Usage.Output - *message.Usage.Reasoning)
				}
				return used + pendingTokens(records[i+1:])
			}
			// Only the latest observation can describe the current context. An
			// incomplete report or summary usage cannot reuse an older total.
			break
		}
	}
	// Retained messages can precede the checkpoint in storage. Their old
	// measurements describe the pre-compaction request, but their content
	// still belongs to the new context alongside the latest summary.
	return pendingTokens(records)
}

func hasSignedReasoning(message *canonical.Message) bool {
	for _, block := range message.Content {
		switch block.Kind {
		case "redacted_thinking":
			return true
		case "thinking":
			if thinking, ok := block.Data.(*canonical.Thinking); ok && thinking.Signature != nil && *thinking.Signature != "" {
				return true
			}
			if thinking, ok := block.Data.(canonical.Thinking); ok && thinking.Signature != nil && *thinking.Signature != "" {
				return true
			}
		}
	}
	return false
}
func pendingTokens(records []canonical.Record) uint64 {
	var cache output.Cache
	var count uint64
	for _, record := range cache.Project(handoff.Pending(records)) {
		count += tokens.Chat("", []canonical.Record{record})
	}
	return count
}
func compactionUsage(record canonical.Record) bool {
	return record.Kind == "compaction" || record.Metadata.Execution != nil && record.Metadata.Execution.Purpose != nil && *record.Metadata.Execution.Purpose == canonical.PurposeCompaction
}

// shouldCompact follows the model's window and the compaction policy on every
// route. An agent program's prompt bound limits only a whole-history replay;
// one that exceeds it fails as a context overflow, which compaction recovers.
func shouldCompact(binding model.Binding, state store.AgentState, policy store.CompactionPolicy) bool {
	last := lastMessage(state.Records)
	if last == nil || last.Role == canonical.Assistant {
		return false
	}
	selected := capturedModel(binding)
	limit := uint64(selected.Context)
	budget := store.AgentCompactionBudget(selected, policy)
	if budget == nil || budget.TriggerTokens == 0 || budget.TriggerTokens >= limit {
		return false
	}
	count := contextTokens(state.Records)
	return count >= budget.TriggerTokens && automaticFailures(binding, state.Records) < pauseAfter
}

// pauseAfter is how many automatic compactions in a row may fail before the
// threshold stops starting one: each costs a summary request and its retries.
const pauseAfter = 3

// automaticFailures counts the automatic compactions that failed in a row for
// this provider and model. A compaction that succeeded, manual or automatic,
// starts the count again, and so does a request another provider or model
// answered. The count is read from the records, so it survives a restart.
func automaticFailures(binding model.Binding, records []canonical.Record) int {
	count := 0
	for _, record := range records {
		if record.Kind == "compaction" {
			count = 0
			continue
		}
		execution := record.Metadata.Execution
		if execution != nil && execution.Source != nil && execution.Media == nil && (execution.Purpose == nil || *execution.Purpose != canonical.PurposeSearch) &&
			(execution.Source.ID != binding.Provider.ID || execution.Source.Model != binding.Config.Model) {
			count = 0
			continue
		}
		if maintenance := record.Metadata.Compaction; maintenance != nil && maintenance.Failure != nil && maintenance.Failure.Source == protocol.ConversationCompactionSourceAutomatic {
			count++
		}
	}
	return count
}

// compactionFailure is the conversation's record of a compaction that failed.
// Clients show it as their own localized notice with the cause.
func (r *Runtime) compactionFailure(x *execution, binding model.Binding, earlier []canonical.Record, manual bool, stage protocol.ConversationCompactionStage, reason protocol.ConversationCompactionReason, attempts []model.Attempt, failure error) canonical.Record {
	detail := protocol.DefaultConversationCompactionFailure()
	detail.Source, detail.Stage, detail.Reason = protocol.ConversationCompactionSourceAutomatic, stage, reason
	if manual {
		detail.Source = protocol.ConversationCompactionSourceManual
	}
	if len(attempts) > 0 {
		detail.Status = attempts[len(attempts)-1].Status
	}
	cause := model.Classify(failure).Message
	detail.Cause = &cause
	record := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant), r.clock.Now())
	record.Provenance.Author = "system"
	runtime := canonical.DefaultRuntime()
	runtime.AgentVisible = false
	record.Metadata.Runtime = &runtime
	// The failure names its provider and model, which the pause counts by. The
	// one that reaches the limit says the pause once.
	scope := model.Scope(binding)
	record.Metadata.Execution = &canonical.Execution{Source: &scope}
	detail.Paused = !manual && automaticFailures(binding, earlier) == pauseAfter-1
	record.Metadata.Compaction = &canonical.Maintenance{Failure: &detail}
	return record
}
func (r *Runtime) compact(x *execution, binding model.Binding, route model.Route, state store.AgentState, manual bool) error {
	return r.compactWith(x, binding, route, state, manual, true)
}

// compactionPlan is what one compaction summarizes and how its checkpoint is
// recorded.
type compactionPlan struct {
	selection          handoff.Selection
	manual, keepRecent bool
	// native allows the route's own compaction to be tried first.
	native      bool
	preserved   *canonical.Record
	retainMedia bool
	window      uint32
	// carry is the opaque checkpoint whose readable handoff this compaction
	// writes; the new checkpoint keeps its items for their own route.
	carry *canonical.NativeCompaction
}

func (r *Runtime) compactWith(x *execution, binding model.Binding, route model.Route, state store.AgentState, manual, native bool) error {
	selected := capturedModel(binding)
	// The compacted history reaches the model whole, so its handoff and
	// retained tail are sized for the window a replay can fill.
	window := route.ReplayWindow(selected.Context)
	keepRecent := true
	if last := lastMessage(state.Records); last != nil && last.Error != nil && last.Error.Code != nil && *last.Error.Code == "context_overflow" {
		keepRecent = false
	}
	selection, useful := handoff.Select(state.Records, uint64(window), keepRecent)
	if manual && !useful {
		// An explicit request can summarize a short conversation whose entire
		// history would otherwise fit in the retained tail.
		selection, useful = handoff.Select(state.Records, uint64(window), false)
	}
	if !useful {
		return nil
	}
	var preserved *canonical.Record
	var retainMedia bool
	if !manual {
		if latest, _ := latestCompactionInput(selection.Tail); latest < 0 {
			if latest, processed := latestCompactionInput(selection.Prefix); latest >= 0 {
				preserved = &selection.Prefix[latest]
				retainMedia = keepRecent || !processed
			}
		}
	}
	// The route's own compaction needs the history to still fit its window
	// and a route that is not an agent session.
	return r.summarize(x, binding, route, state, compactionPlan{selection: selection, manual: manual, keepRecent: keepRecent, native: native && keepRecent && !route.Session, preserved: preserved, retainMedia: retainMedia, window: window})
}

// summarize writes the checkpoint for plan: natively where the route can,
// else in place from the prompt cache, else over a rendered transcript.
func (r *Runtime) summarize(x *execution, binding model.Binding, route model.Route, state store.AgentState, plan compactionPlan) error {
	selected := capturedModel(binding)
	selection, manual, keepRecent, window := plan.selection, plan.manual, plan.keepRecent, plan.window
	output := min(model.OutputLimit(binding, true), int64(handoff.Output(uint64(window))))
	cap := handoff.Cap(uint64(window), uint64(output))
	summaryID := protocol.NewRequestID().String()
	hideCompacting := func() {
		if !manual {
			r.feed.Publish(x.inv.Session.ID, protocol.ConversationChange{
				Kind: "hidden",
				Data: protocol.ConversationChangeHiddenPayload{ID: summaryID, Turn: x.inv.Run.Turn},
			})
		}
	}
	if !manual {
		progress, _ := json.Marshal(map[string]any{"type": "compacting", "started_ms": r.clock.Now().UnixMilli()})
		_ = r.feed.Delta(x.inv.Session.ID, protocol.ConversationDraft{ID: summaryID, Turn: x.inv.Run.Turn, Author: "sailry", Parts: []protocol.ConversationPart{{Kind: "resource", Data: json.RawMessage(progress)}}})
	}
	binding, err := r.credential(x, binding)
	if err != nil {
		hideCompacting()
		return err
	}
	summaryBinding := model.Summary(binding)
	if route.Session {
		// A transcript summary runs in an agent session of its own, so it
		// neither replaces nor resumes the session that holds the history.
		summaryBinding.Session = protocol.SessionID{}
	}
	requestText := handoff.Request
	if manual && strings.TrimSpace(x.inv.Message.Text) != "" {
		requestText += "\n\nUser focus for the handoff:\n" + x.inv.Message.Text
	}
	var attempts compactionAttempts
	// A compaction that ends without a summary leaves its billed attempts and
	// one record saying where and why it stopped; a stopped turn is no failure.
	failed := func(stage protocol.ConversationCompactionStage, reason protocol.ConversationCompactionReason, failure error) error {
		hideCompacting()
		var records []canonical.Record
		for _, attempt := range attempts {
			records = append(records, r.observationRecord(x, attempt.binding, canonical.PurposeCompaction, attempt.attempt))
		}
		if x.ctx.Err() == nil {
			records = append(records, r.compactionFailure(x, summaryBinding, state.Records, manual, stage, reason, attempts.requests(), failure))
		}
		if e := r.saveWithProvider(context.WithoutCancel(x.ctx), x.inv.Session.ID, &summaryBinding.Provider, records...); e != nil {
			return e
		}
		return failure
	}
	stream := &summaryStream{}
	// current is the configuration the running attempt uses; its observations
	// record the effort that was actually requested.
	current := summaryBinding
	observe := func(attempt model.Attempt) error {
		x.timeline.attempt(attempt)
		// An attempt that waits to be sent again shows as a main request's does.
		if attempt.RetryDelay != nil {
			return r.saveWithProvider(context.WithoutCancel(x.ctx), x.inv.Session.ID, &current.Provider, r.retryRecord(x, current, canonical.PurposeCompaction, attempt))
		}
		stream.usage = attempt.Usage
		attempts = append(attempts, compactionAttempt{attempt: attempt, binding: current})
		return nil
	}
	collect := func(part fantasy.StreamPart) bool { return stream.receive(part) && x.ctx.Err() == nil }
	save := func(summaryBinding model.Binding, rendered string, files []canonical.CompactedFile, native *canonical.NativeCompaction) error {
		return r.saveCompaction(x, state, plan, summaryID, summaryBinding, rendered, files, native, stream.usage, attempts)
	}

	if plan.native {
		current = binding
		compacted, ok, err := r.nativeCompaction(x, binding, route, state, selection, requestText, observe)
		if err != nil {
			return failed(protocol.ConversationCompactionStageRequest, compactionReason(err), err)
		}
		if ok {
			return save(binding, compacted.Summary, nil, &compacted.Native)
		}
		current = summaryBinding
	}
	languageModel, err := r.model(x.ctx, summaryBinding)
	if err != nil {
		return failed(protocol.ConversationCompactionStageRequest, protocol.ConversationCompactionReasonRequestFailed, err)
	}
	systemBytes := model.SummarySystemBytes(languageModel, requestText)
	// A summary of a history that still fits reuses the main request when the
	// provider served the latest main request from its prompt cache: the
	// cached prefix then costs a fraction of a fresh transcript. An agent
	// session that holds the history receives only the summary instructions
	// and summarizes everything it holds. Without that evidence, an overflow
	// recovery, or a model that would replay a bounded transcript, the
	// rendered handoff is smaller and cheaper.
	if keepRecent && (route.Session && route.Resident || systemBytes == 0 && servedFromCache(binding, state.Records)) {
		current = binding
		summary, ok, err := r.inPlaceSummary(x, binding, state, selection.Tail, requestText, uint64(selected.Context), uint64(output), cap, observe, collect, stream)
		if err != nil {
			return failed(protocol.ConversationCompactionStageRequest, compactionReason(err), err)
		}
		if ok {
			return r.compactSession(x, binding, route, save(binding, summary.Render(), compactedFiles(summary), nil))
		}
		stream.reset()
		current = summaryBinding
	}
	if len(selection.Prefix) > 0 {
		if previous, ok := selection.Prefix[0].Data.(*canonical.Compaction); ok && selection.Prefix[0].Kind == "compaction" && !previous.Readable() {
			// The previous checkpoint is an opaque window only its own route can
			// read. A transcript over it needs its readable handoff first; the
			// attempts this compaction already billed keep their observations.
			var records []canonical.Record
			for _, attempt := range attempts {
				records = append(records, r.observationRecord(x, attempt.binding, canonical.PurposeCompaction, attempt.attempt))
			}
			if err := r.saveWithProvider(context.WithoutCancel(x.ctx), x.inv.Session.ID, &summaryBinding.Provider, records...); err != nil {
				return err
			}
			hideCompacting()
			if _, err := r.readableCheckpoint(x, binding, route, state, true); err != nil {
				return err
			}
			state, err := r.load(x.ctx, x.inv.Session.ID)
			if err != nil {
				return err
			}
			return r.compactWith(x, binding, route, state, manual, false)
		}
	}
	system, err := handoff.Prepare(selection.Prefix, uint64(window), uint64(output), requestText, systemBytes)
	if err != nil {
		return failed(protocol.ConversationCompactionStageBudget, protocol.ConversationCompactionReasonNoFittingPrefix, err)
	}
	request := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, requestText)), r.clock.Now())
	run := func(system string) error {
		stream.reset()
		call, err := model.BuildCall(summaryBinding, model.Request{System: system, Records: []canonical.Record{request}, MaxOutputTokens: &output, Delegated: x.inv.Session.Delegation != nil})
		if err != nil {
			return err
		}
		return model.Run(x.ctx, languageModel, call, model.SummaryPurpose, r.clock, observe, collect, nil)
	}
	err = run(system)
	const maxCompactionShrinks = 4
	for shrink := 0; shrink < maxCompactionShrinks && err != nil && x.ctx.Err() == nil && model.Classify(err).Kind == model.ContextOverflow; shrink++ {
		// The model's actual input limit can be much smaller than the captured
		// window. Shrink repeatedly until it fits or reaches the minimum budget.
		smaller, ok := handoff.PrepareSmaller(selection.Prefix, uint64(window), uint64(output), requestText, system)
		if !ok || smaller == system {
			break
		}
		system = smaller
		err = run(system)
	}
	if err != nil {
		return failed(protocol.ConversationCompactionStageRequest, compactionReason(err), err)
	}
	if stream.limited {
		return failed(protocol.ConversationCompactionStageSummary, protocol.ConversationCompactionReasonTruncatedSummary, fmt.Errorf("compaction summary reached its output limit"))
	}
	summary, err := handoff.Parse(stream.text.String(), cap)
	if err != nil {
		reason := protocol.ConversationCompactionReasonUnexpectedOutput
		if strings.TrimSpace(stream.text.String()) == "" {
			reason = protocol.ConversationCompactionReasonEmptySummary
		}
		return failed(protocol.ConversationCompactionStageSummary, reason, err)
	}
	return r.compactSession(x, binding, route, save(summaryBinding, summary.Render(), compactedFiles(summary), plan.carry))
}

func compactedFiles(summary handoff.Summary) []canonical.CompactedFile {
	var files []canonical.CompactedFile
	for _, file := range summary.Files {
		description := file.Summary
		if file.KeyCode != nil {
			description += "\n" + *file.KeyCode
		}
		files = append(files, canonical.CompactedFile{Path: file.Path, Summary: &description})
	}
	return files
}

// nativeCompaction asks the route to compact the summarized prefix itself.
// ok is false when the route has no native compaction, rejected it or
// returned nothing usable, and Sailry's own handoff should be written
// instead; err ends the compaction as a failure the handoff would meet too.
// A rejection of the feature is remembered for the route and model;
// transient failures follow the retry rules and are not.
func (r *Runtime) nativeCompaction(x *execution, binding model.Binding, route model.Route, state store.AgentState, selection handoff.Selection, requestText string, observe func(model.Attempt) error) (model.Compacted, bool, error) {
	// A user's focus for a manual compaction is part of Sailry's handoff
	// request; a provider's summary prompt would replace or drop it.
	if route.Native == "" || requestText != handoff.Request {
		return model.Compacted{}, false, nil
	}
	key := binding.Provider.ID.String() + "\x00" + route.Native
	if _, rejected := r.nativeRejected.Load(key); rejected {
		return model.Compacted{}, false, nil
	}
	languageModel, err := r.model(x.ctx, binding)
	if err != nil {
		return model.Compacted{}, false, err
	}
	// The request is the main request over the records being replaced, so a
	// provider that cached the conversation reads that prefix from its cache.
	included := map[string]bool{}
	for _, record := range selection.Prefix {
		included[record.ID] = true
	}
	prefix := state
	prefix.Records = nil
	for _, record := range state.Records {
		if included[record.ID] {
			prefix.Records = append(prefix.Records, record)
		}
	}
	request, err := r.mainRequest(x, binding, prefix, prompt.Instruction(x.inv.Session.Delegation != nil))
	if err != nil {
		return model.Compacted{}, false, err
	}
	call, err := model.BuildVisibleProjectedCall(binding, request)
	if err != nil {
		return model.Compacted{}, false, err
	}
	compacted, err := model.CompactNative(x.ctx, languageModel, binding, call, r.clock, observe)
	switch {
	case err == nil:
		nativeOutcome(binding, route, "native", nil)
		return compacted, true, nil
	case errors.Is(err, model.ErrNativeUnsupported):
		r.nativeRejected.Store(key, true)
		nativeOutcome(binding, route, "unsupported", err)
		return model.Compacted{}, false, nil
	case errors.Is(err, model.ErrNativeUnusable):
		nativeOutcome(binding, route, "unusable", err)
		return model.Compacted{}, false, nil
	}
	failure := model.Classify(err)
	switch kind := failure.Kind; {
	case x.ctx.Err() != nil, failure.Exhausted, failure.WaitExhausted, kind == model.Cancelled, kind == model.RateLimited, kind == model.Quota, kind == model.Authentication, kind == model.NotConfigured:
		return model.Compacted{}, false, err
	}
	nativeOutcome(binding, route, "failed", err)
	return model.Compacted{}, false, nil
}

// nativeOutcome records whether a route compacted natively or fell back to
// Sailry's handoff, the only place that choice is observable after the fact.
func nativeOutcome(binding model.Binding, route model.Route, outcome string, err error) {
	attrs := []slog.Attr{slog.String("area", "agent"), slog.String("operation", "native_compaction"), slog.String("outcome", outcome),
		slog.String("provider", binding.Provider.ID.String()), slog.String("method", route.Native), slog.String("model", binding.Config.Model)}
	level := slog.LevelInfo
	if err != nil {
		level = slog.LevelWarn
		attrs = append(attrs, slog.String("exception", err.Error()))
	}
	logging.Emit(level, attrs...)
}

// readableCheckpoint writes Sailry's readable handoff for the latest
// checkpoint when it is an opaque window and the history is next read by
// another route, or by Sailry itself when force is set. The new checkpoint
// has the same boundary and keeps the window for its own route. written is
// false when nothing needed it.
func (r *Runtime) readableCheckpoint(x *execution, binding model.Binding, route model.Route, state store.AgentState, force bool) (bool, error) {
	latest := -1
	for i, record := range state.Records {
		if record.Kind == "compaction" {
			latest = i
		}
	}
	if latest < 0 {
		return false, nil
	}
	checkpoint, ok := state.Records[latest].Data.(*canonical.Compaction)
	if !ok || checkpoint.Native == nil || checkpoint.Readable() || !force && model.ReplaysNative(binding, state.Records[latest]) {
		return false, nil
	}
	// Without the opaque checkpoints, the history is what those checkpoints
	// replaced: the latest readable handoff and everything after it.
	records := slices.DeleteFunc(slices.Clone(state.Records), func(record canonical.Record) bool {
		value, ok := record.Data.(*canonical.Compaction)
		return record.Kind == "compaction" && ok && !value.Readable()
	})
	var previous *canonical.Record
	for i := range records {
		if records[i].Kind == "compaction" {
			previous = &records[i]
		}
	}
	visible := handoff.Visible(records)
	cut := slices.IndexFunc(visible, func(record canonical.Record) bool { return record.ID == checkpoint.Boundary })
	boundary := slices.IndexFunc(records, func(record canonical.Record) bool { return record.ID == checkpoint.Boundary })
	if cut < 0 || boundary < 0 {
		return false, &model.Failure{Kind: model.InvalidRequest, Message: "compacted history has no readable boundary"}
	}
	selection := handoff.Selection{Prefix: slices.Clone(visible[:cut+1]), Tail: slices.Clone(visible[cut+1:]), Boundary: records[boundary]}
	if previous != nil && selection.Prefix[0].ID == previous.ID {
		if message := selection.Prefix[0].Message(); message != nil {
			summary := *previous.Data.(*canonical.Compaction)
			summary.Summary = *message
			selection.Prefix[0].Kind, selection.Prefix[0].Data = "compaction", &summary
		}
	}
	window := route.ReplayWindow(capturedModel(binding).Context)
	return true, r.summarize(x, binding, route, state, compactionPlan{selection: selection, window: window, carry: checkpoint.Native})
}

// compactSession follows a saved handoff on an agent session that holds the
// history and advertises `/compact`: the agent shrinks its own context and
// keeps serving the session, as in T3 Code, instead of a new session that
// replays the handoff. The handoff came first, while the session still held
// everything; the command is a second pass over that context. A command that
// fails leaves the recorded handoff, and the next request replays it.
func (r *Runtime) compactSession(x *execution, binding model.Binding, route model.Route, saved error) error {
	if saved != nil || !route.Session || !route.Resident || !route.Compact {
		return saved
	}
	state, err := r.load(x.ctx, x.inv.Session.ID)
	if err != nil {
		return err
	}
	request, err := r.mainRequest(x, binding, state, prompt.Instruction(x.inv.Session.Delegation != nil))
	if err != nil {
		return err
	}
	call, err := model.BuildVisibleProjectedCall(binding, request)
	if err != nil {
		return err
	}
	languageModel, err := r.model(x.ctx, binding)
	if err != nil {
		return err
	}
	_ = model.CompactSession(x.ctx, languageModel, call)
	return x.ctx.Err()
}

// servedFromCache reports whether the latest main request of this provider
// and model read part of its input from the provider's prompt cache.
func servedFromCache(binding model.Binding, records []canonical.Record) bool {
	for i := len(records) - 1; i >= 0; i-- {
		record := records[i]
		message := record.Message()
		if message == nil || message.Role != canonical.Assistant || message.Usage == nil || compactionUsage(record) {
			continue
		}
		execution := record.Metadata.Execution
		if execution == nil || execution.Source == nil || execution.Purpose == nil || *execution.Purpose != canonical.PurposeMain {
			continue
		}
		if execution.Source.ID != binding.Provider.ID || execution.Source.Model != binding.Config.Model {
			return false
		}
		return message.Usage.CacheRead != nil && *message.Usage.CacheRead > 0
	}
	return false
}

// compactionAttempt is one billed summary request and the configuration it
// ran with.
type compactionAttempt struct {
	attempt model.Attempt
	binding model.Binding
}
type compactionAttempts []compactionAttempt

func (a compactionAttempts) requests() []model.Attempt {
	out := make([]model.Attempt, len(a))
	for i, value := range a {
		out[i] = value.attempt
	}
	return out
}

// summaryStream collects one summary attempt's text and how it ended.
type summaryStream struct {
	text                           strings.Builder
	limited, textBlock, toolCalled bool
	usage                          canonical.Usage
}

func (s *summaryStream) reset() {
	s.text.Reset()
	s.limited, s.textBlock, s.toolCalled = false, false, false
}
func (s *summaryStream) receive(part fantasy.StreamPart) bool {
	switch part.Type {
	case fantasy.StreamPartTypeTextDelta:
		if !s.textBlock && s.text.Len() > 0 {
			s.text.WriteByte('\n')
		}
		s.text.WriteString(part.Delta)
		s.textBlock = true
	case fantasy.StreamPartTypeToolCall:
		s.toolCalled = true
		s.textBlock = false
	case fantasy.StreamPartTypeReasoningDelta, fantasy.StreamPartTypeReasoningEnd:
		s.textBlock = false
	case fantasy.StreamPartTypeFinish:
		switch strings.ToLower(string(part.FinishReason)) {
		case "length", "max_tokens", "max_output_tokens":
			s.limited = true
		}
	}
	return true
}

func compactionReason(err error) protocol.ConversationCompactionReason {
	switch model.Classify(err).Kind {
	case model.RateLimited, model.Quota:
		return protocol.ConversationCompactionReasonRateLimited
	case model.Authentication, model.NotConfigured:
		return protocol.ConversationCompactionReasonAuthorizationFailed
	case model.ContextOverflow:
		return protocol.ConversationCompactionReasonContextOverflow
	case model.InvalidRequest, model.Unsupported:
		return protocol.ConversationCompactionReasonServiceRejected
	case model.InvalidResponse:
		return protocol.ConversationCompactionReasonIncompleteResponse
	}
	return protocol.ConversationCompactionReasonRequestFailed
}

// inPlaceSummary asks for the handoff with the main request's system prompt,
// tools, effort and history unchanged, followed by the summary instructions,
// so the provider can serve the conversation from its prompt cache. Thinking,
// effort and tool choice are part of that cached prefix on some providers,
// which is why the request keeps the turn's own configuration. ok is false
// when this request cannot produce a usable handoff and the rendered
// transcript should be tried instead; err reports a failure that the
// transcript request would meet as well.
func (r *Runtime) inPlaceSummary(x *execution, binding model.Binding, state store.AgentState, tail []canonical.Record, requestText string, contextLimit, outputLimit, cap uint64, observe func(model.Attempt) error, collect func(fantasy.StreamPart) bool, stream *summaryStream) (handoff.Summary, bool, error) {
	instructions := handoff.InPlace(contextLimit, outputLimit, tail)
	if requestText != handoff.Request {
		instructions += strings.TrimPrefix(requestText, handoff.Request)
	}
	// The handoff needs its summary output plus room for the turn's reasoning,
	// not the main step's whole output limit: automatic compaction starts once
	// history leaves less than that limit free. A thinking budget must stay
	// below the output limit, so a budget effort keeps the main limit, as does
	// a request whose cached thinking parameters follow its limit.
	maxOutput := model.InPlaceOutputLimit(binding, int64(2*outputLimit))
	if main := model.OutputLimit(binding, false); main > 0 && binding.Config.Effort.Kind == "budget" {
		maxOutput = main
	}
	if contextTokens(state.Records)+handoff.InPlaceTokens(instructions)+uint64(maxOutput) > contextLimit {
		return handoff.Summary{}, false, nil
	}
	request, err := r.mainRequest(x, binding, state, prompt.Instruction(x.inv.Session.Delegation != nil))
	if err != nil {
		return handoff.Summary{}, false, err
	}
	request.MaxOutputTokens = &maxOutput
	ask := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, instructions)), r.clock.Now())
	request.Records = append(request.Records, ask)
	call, err := model.BuildVisibleProjectedCall(binding, request)
	if err != nil {
		return handoff.Summary{}, false, err
	}
	languageModel, err := r.model(x.ctx, binding)
	if err != nil {
		return handoff.Summary{}, false, err
	}
	stream.reset()
	err = model.Run(x.ctx, languageModel, call, model.SummaryPurpose, r.clock, observe, collect, nil)
	if err != nil {
		// A request that spent its retries would spend them again on the transcript.
		failure := model.Classify(err)
		switch kind := failure.Kind; {
		case x.ctx.Err() != nil, failure.Exhausted, failure.WaitExhausted, kind == model.Cancelled, kind == model.RateLimited, kind == model.Quota, kind == model.Authentication, kind == model.NotConfigured:
			return handoff.Summary{}, false, err
		}
		return handoff.Summary{}, false, nil
	}
	if stream.toolCalled || stream.limited {
		return handoff.Summary{}, false, nil
	}
	summary, err := handoff.Parse(stream.text.String(), cap)
	if err != nil {
		return handoff.Summary{}, false, nil
	}
	return summary, true, nil
}

// saveCompaction records a handoff as the conversation's checkpoint. rendered
// is empty for an opaque native window, which has no readable text yet.
func (r *Runtime) saveCompaction(x *execution, state store.AgentState, plan compactionPlan, summaryID string, summaryBinding model.Binding, rendered string, files []canonical.CompactedFile, native *canonical.NativeCompaction, usage canonical.Usage, attempts compactionAttempts) error {
	selection, manual, preserved, retainMedia := plan.selection, plan.manual, plan.preserved, plan.retainMedia
	boundary := selection.Boundary
	message := canonical.NewMessage(canonical.User)
	if strings.TrimSpace(rendered) != "" {
		message = canonical.NewMessage(canonical.User, canonical.Text(0, rendered))
	}
	message.Model = &summaryBinding.Config.Model
	message.Usage = &usage
	// The last summary attempt is billed on the summary; earlier attempts retain
	// their independent observations.
	compacted := canonical.Compaction{Boundary: boundary.ID, Summary: message, Start: state.Records[0].Created, End: boundary.Created, Files: files, Native: native}
	record := canonical.NewRecord(x.inv.Run.Turn, "compaction", &compacted, r.clock.Now())
	record.ID = summaryID
	record.Provenance.Author = "system"
	runtime := canonical.DefaultRuntime()
	runtime.UserVisible = false
	record.Metadata.Runtime = &runtime
	tailState := state
	tailState.Records = selection.Tail
	loaded := retainedTools(tailState)
	record.Metadata.Context = &canonical.Context{LoadedTools: &loaded}
	var records []canonical.Record
	if len(attempts) > 0 {
		for _, attempt := range attempts[:len(attempts)-1] {
			records = append(records, r.observationRecord(x, attempt.binding, canonical.PurposeCompaction, attempt.attempt))
		}
		final := attempts[len(attempts)-1]
		last := r.observationRecord(x, final.binding, canonical.PurposeCompaction, final.attempt)
		record.Metadata.Execution = last.Metadata.Execution
	}
	records = append(records, record)
	if len(selection.Tail) == 0 {
		continuation := "Context was compacted. The preceding summary covers the earlier conversation. Do not mention the summary or compaction. Continue calling tools as necessary to complete the task."
		if manual {
			continuation = "Context was compacted at the user's request. The preceding summary covers the earlier conversation. Do not mention the summary or compaction. Continue from the summarized context."
		}
		continuation = "<sailry-runtime-continuation>" + continuation + "</sailry-runtime-continuation>"
		continued := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, continuation)), r.clock.Now())
		continued.Provenance.Author = "assistant"
		notice := runtime
		notice.TurnContext = true
		continued.Metadata.Runtime = &notice
		records = append(records, continued)
	}
	if preserved != nil {
		var blocks []canonical.Block
		for _, block := range preserved.Message().Content {
			switch block.Kind {
			case "text":
			case "image", "document", "file":
				if !retainMedia {
					continue
				}
			default:
				continue
			}
			block.Index = uint64(len(blocks))
			blocks = append(blocks, block)
		}
		if len(blocks) > 0 {
			// A split can cover the current user input. Keep its exact content
			// and original attachment binding alongside the retained exchanges.
			carry := canonical.NewRecord(preserved.Turn, "message", canonical.NewMessage(canonical.User, blocks...), r.clock.Now())
			carry.Provenance.Author = "assistant"
			carry.Metadata.Runtime = &runtime
			records = append(records, carry)
		}
	}
	return r.saveWithProvider(x.ctx, x.inv.Session.ID, &summaryBinding.Provider, records...)
}

func latestCompactionInput(records []canonical.Record) (index int, processed bool) {
	for i := len(records) - 1; i >= 0; i-- {
		record := records[i]
		if compactionUsage(record) || record.Metadata.Runtime != nil && record.Metadata.Runtime.TurnContext {
			continue
		}
		message := record.Message()
		if message == nil {
			continue
		}
		if message.Role == canonical.Assistant && message.Error == nil {
			processed = true
		}
		if message.Role != canonical.User || containsTool(message) {
			continue
		}
		for _, block := range message.Content {
			switch block.Kind {
			case "text", "image", "document", "file":
				return i, processed
			}
		}
	}
	return -1, processed
}

func containsTool(message *canonical.Message) bool {
	for _, block := range message.Content {
		if block.Kind == "tool_request" || block.Kind == "tool_response" {
			return true
		}
	}
	return false
}
func retainedTools(state store.AgentState) []string {
	present := map[string]bool{}
	searches := map[string]bool{}
	for _, record := range handoff.Visible(state.Records) {
		if m := record.Message(); m != nil {
			for _, block := range m.Content {
				switch value := block.Data.(type) {
				case *canonical.ToolRequest:
					if value.Error == nil {
						present[value.Name] = true
						if value.Name == "tool_search" {
							searches[value.ID] = true
						}
					}
				case *canonical.ToolResponse:
					if value.Format == canonical.MCPError || !searches[value.ID] {
						continue
					}
					var discovered struct {
						Tools      []struct{ Name string } `json:"tools"`
						Structured json.RawMessage         `json:"structuredContent"`
					}
					if json.Unmarshal(value.Result.Value, &discovered) != nil {
						continue
					}
					if value.Format == canonical.MCP && len(discovered.Structured) > 0 {
						_ = json.Unmarshal(discovered.Structured, &discovered)
					}
					for _, tool := range discovered.Tools {
						present[tool.Name] = true
					}
				}
			}
		}
	}
	out := []string{}
	for _, name := range state.LoadedTools {
		if present[name] {
			out = append(out, name)
		}
	}
	return out
}
