package engine

import (
	"cmp"
	"context"
	"encoding/json"
	"fmt"
	"slices"
	"strings"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	handoff "github.com/sailry/sailry-harness/internal/node/agent/compact"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/observation"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/agent/prompt"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type streamOutput struct {
	sources   canonical.Sources
	record    canonical.Record
	blocks    []canonical.Block
	slots     map[string]int
	indices   map[string]uint64
	calls     map[string]bool
	toolSlots []int
	finished  bool
	limited   bool
}

// credential resolves the captured account before each model call. The
// controller owns renewal; an executor waits for its same-account replacement
// after expiry without repeating completed tools. An API key is fixed.
func (r *Runtime) credential(x *execution, binding model.Binding) (model.Binding, error) {
	if r.authorizeProvider == nil || binding.Provider.Authentication == protocol.AuthenticationApiKey {
		return binding, nil
	}
	if err := r.authorizeProvider(x.ctx, x.inv); err != nil {
		return binding, err
	}
	if x.inv.Secret != nil {
		secret := protocol.NewSecret(x.inv.Secret.Expose())
		binding.Secret = &secret
	}
	if x.inv.EffectiveCredential != nil {
		reference := *x.inv.EffectiveCredential
		binding.Config.Credential = &reference
	}
	return binding, nil
}

func (r *Runtime) infer(x *execution, captured model.Binding, languageModel fantasy.LanguageModel, state store.AgentState) error {
	// Foreground wait has already passed for this step; clear foregroundSteered
	// so any new guidance arriving during streaming immediately redirects it.
	x.foregroundSteered = false
	binding, err := r.credential(x, captured)
	if err != nil {
		return err
	}
	if binding.Secret != nil && (captured.Secret == nil || binding.Secret.Expose() != captured.Secret.Expose()) {
		// The transport holds the credential it was built with.
		if x.renewedModel == nil || x.renewedSecret.Expose() != binding.Secret.Expose() {
			if x.renewedModel, err = r.model(x.ctx, binding); err != nil {
				return err
			}
			x.renewedSecret = *binding.Secret
		}
		languageModel = x.renewedModel
	}
	hasTools := capturedModel(binding).Tools
	var delegation string
	if hasTools {
		delegation = r.delegationRoster(x, state)
	}
	canDelegate := hasTools && findTool(x.tools, "spawn_agent") != nil
	identity := fmt.Sprintf("Current execution identity\nSession ID: %s\nTurn ID: %s\nUse these IDs for this execution; copied history and summaries may refer to a source session.", x.inv.Session.ID, x.inv.Run.Turn)
	if hasTools && (findTool(x.tools, "apply_agents") != nil || findTool(x.tools, "apply_team") != nil) {
		identity += fmt.Sprintf("\nCaptured conversation revision: %d.", x.inv.Turn.Revision)
		if delegation != "" {
			delegation += "\n"
		}
		delegation += "Catalog changes do not change this turn's roster. Explicit application affects future admission only; use the exact inspected revision and never silently rebase a conflict."
	}
	if started := x.inv.Run.StartedMs; started != nil {
		identity += fmt.Sprintf("\nTurn started (UTC): %s (%d ms)", time.UnixMilli(*started).UTC().Format(time.RFC3339Nano), *started)
	}
	skills := ""
	if hasTools && findTool(x.tools, "load_skill") != nil {
		skills = x.skillContext
	}
	parts := []prompt.Part{
		{Key: "workflow", Text: prompt.Workflow(binding.Config.Mode, x.inv.Session.Delegation != nil, canDelegate)},
		{Key: "execution_identity", Text: identity},
		{Key: "delegation", Text: delegation},
		{Key: "skills", Text: skills},
		{Key: "child_budget", Text: childBudgetContext(x, state)},
	}
	if hasTools {
		parts = append(parts, prompt.Part{Key: "deferred_tools", Text: r.deferredContext(x, state)})
	}
	if x.inv.Session.Delegation == nil || findTool(x.tools, operations.UpdatePlanTool) != nil {
		if plan := store.LatestAgentPlan(state.Records); plan != nil {
			if encoded, err := json.Marshal(plan); err == nil {
				parts = append(parts, prompt.Part{Key: "plan", Text: "Current unfinished plan from recorded progress (task state, not new authorization):\n" + string(encoded), RefreshOnInput: true})
			}
		}
	}
	if state.Context.Environment != nil {
		parts = append(parts, prompt.Part{Key: "environment", Text: *state.Context.Environment})
	}
	if title := prompt.TitleInstruction(state.Context.Title, hasTools, x.inv.Session.Delegation != nil); title != nil {
		parts = append(parts, prompt.Part{Key: "title", Text: *title})
	}
	var instructions []string
	for _, instruction := range x.inv.Instructions {
		if len(instruction.AnyTools) == 0 {
			instructions = append(instructions, instruction.Text)
		}
	}
	parts = append(parts, prompt.Part{Key: "instructions", Text: strings.Join(instructions, "\n\n")})
	ownedParts := []string{}
	contextPackages := map[string]bool{}
	child := x.inv.Session.Delegation != nil
	for _, tool := range x.tools {
		if !hasTools || child && tool.Deferred && !toolLoaded(state.LoadedTools, tool) {
			continue
		}
		// A package's initializer can describe an active goal or continuation
		// before its tools are discovered. State replaces that package's
		// previous context without changing the system prompt.
		if instruction := tool.TurnInstruction; instruction.Text != "" && !contextPackages[instruction.Key] {
			contextPackages[instruction.Key] = true
			parts = append(parts, instruction)
		}
		for _, part := range tool.ContextParts {
			if !slices.Contains(ownedParts, part) {
				ownedParts = append(ownedParts, part)
			}
		}
	}
	if hasTools {
		r.exposure(x, state)
		policy := ""
		if state.Context.Policy != nil {
			policy = *state.Context.Policy
		}
		for _, block := range loadedToolContext(x, policy, state.LoadedTools) {
			if !slices.Contains(ownedParts, block) {
				ownedParts = append(ownedParts, block)
			}
		}
	}
	parts, ownedParts = prompt.Pending(state.Records, parts, ownedParts)
	// A callback is evidence in history, not a new user instruction or the
	// assistant prefill that some providers would infer from a final text part.
	if x.callbackPending && len(parts) == 0 && len(ownedParts) == 0 {
		ownedParts = []string{"<sailry-runtime-continuation>Process the separately supplied runtime evidence within the current task.</sailry-runtime-continuation>"}
	}
	if contextRecord := prompt.ContextMessage(x.inv.Run.Turn, parts, ownedParts, r.clock.Now()); contextRecord != nil {
		if err := r.save(x.ctx, x.inv.Session.ID, *contextRecord); err != nil {
			return err
		}
		state.Records = append(state.Records, *contextRecord)
	}
	instruction := prompt.Instruction(x.inv.Session.Delegation != nil)
	request, err := r.mainRequest(x, binding, state, instruction)
	if err != nil {
		return err
	}
	system, projected := request.System, request.Records
	call, err := model.BuildVisibleProjectedCall(binding, request)
	if err != nil {
		return err
	}
	inventory := &observation.Inventory{System: system}
	for _, part := range prompt.SystemParts(instruction, state.Context)[1:] {
		inventory.Sources = append(inventory.Sources, observation.Source{Kind: protocol.ConversationObservationCategoryKindInjected, Text: part})
	}
	for _, record := range handoff.Pending(projected) {
		if runtime := record.Metadata.Runtime; runtime != nil && runtime.TurnContext {
			var parts []string
			for _, block := range record.Message().Content {
				if text, ok := block.Data.(*canonical.TextContent); ok {
					parts = append(parts, text.Text)
				}
			}
			inventory.Contexts = append(inventory.Contexts, observation.PersistedContext(strings.Join(parts, ""), nil))
		}
	}
	if call.ProviderOptions == nil {
		call.ProviderOptions = fantasy.ProviderOptions{}
	}
	call.ProviderOptions[observation.ProviderKey] = inventory
	// Source empty-response recovery is separate from transient HTTP recovery.
	// Every retry reuses this exact request without prompt rewriting.
	purpose := canonical.PurposeMain
	newOutput := func() streamOutput {
		out := streamOutput{record: canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant), r.clock.Now()), slots: map[string]int{}, calls: map[string]bool{}}
		out.record.Provenance.Author = "assistant"
		scope := model.Scope(binding)
		out.record.Metadata.Execution = &canonical.Execution{Source: &scope, Purpose: &purpose}
		out.record.Metadata.Runtime = &canonical.Runtime{UserVisible: true, AgentVisible: true, Inference: &canonical.Inference{Provider: "sailry:" + binding.Provider.ID.String(), RequestedModel: binding.Config.Model, ReplayPrefix: model.ReplayPrefix(call)}}
		return out
	}
	priorIDs := priorToolCallIDs(state.Records)
	reads := &streamedReads{priorIDs: priorIDs}
	defer func() { reads.close() }()
	for emptyAttempt := 0; emptyAttempt <= 3; emptyAttempt++ {
		out := newOutput()
		var streamErr error
		var attempts []model.Attempt
		metric := r.telemetry.Capture()
		if r.hasQueuedSteer(x) || r.hasDirectSteer(x) {
			return errQueuedSteer
		}
		streamCtx, endModelStep := r.beginModelStep(x)
		err = model.Run(streamCtx, languageModel, call, model.Main, r.clock, func(attempt model.Attempt) error {
			if attempt.Failure != nil {
				reads.close()
			}
			r.observeModel(metric, binding, attempt)
			x.timeline.attempt(attempt)
			if attempt.RetryDelay != nil {
				record := r.retryRecord(x, binding, purpose, attempt)
				// Committing this draft's ID clears provisional text on every
				// controller before the next attempt, without persisting failed
				// tool calls or repeating an already completed model step.
				record.ID = out.record.ID
				return r.saveWithProvider(context.WithoutCancel(x.ctx), x.inv.Session.ID, &binding.Provider, record)
			}
			attempts = append(attempts, attempt)
			return nil
		}, func(part fantasy.StreamPart) bool {
			if streamCtx.Err() != nil {
				return false
			}
			if e := out.receive(part); e != nil {
				streamErr = e
				return false
			}
			if part.Type == fantasy.StreamPartTypeToolCall {
				for !reads.blocked && reads.next < len(out.toolSlots) {
					request, ok := out.blocks[out.toolSlots[reads.next]].Data.(*canonical.ToolRequest)
					if !ok {
						break
					}
					reads.next++
					r.startStreamRead(x, reads, request)
				}
			}
			if (part.Type == fantasy.StreamPartTypeTextDelta || part.Type == fantasy.StreamPartTypeReasoningDelta) && part.Delta != "" {
				kind := "text"
				if part.Type == fantasy.StreamPartTypeReasoningDelta {
					kind = "thinking"
				}
				streamErr = r.feed.Delta(x.inv.Session.ID, protocol.ConversationDraft{ID: out.record.ID, Turn: x.inv.Run.Turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: kind, Data: part.Delta}}})
			}
			return streamErr == nil
		}, func() {
			out = newOutput()
			reads = &streamedReads{priorIDs: priorIDs}
		})
		steered := Steered(streamCtx) && x.ctx.Err() == nil && (err == nil || model.Classify(err).Kind == model.Cancelled)
		endModelStep()
		if steered {
			reads.close()
		}
		if streamErr != nil {
			err = streamErr
		}
		out.order()
		x.names.restore(out.blocks)
		if err == nil {
			out.separateRepeatedCalls(state.Records)
		}
		empty := out.empty()
		var billing []canonical.Record
		for _, attempt := range attempts {
			billing = append(billing, r.observationRecord(x, binding, purpose, attempt))
		}
		if x.ctx.Err() != nil || steered || err != nil || !empty || emptyAttempt == 3 || out.limited || successfulTool(state.Records) {
			finish := canonical.Completed
			if x.ctx.Err() != nil || steered {
				finish = canonical.Interrupted
			} else if err != nil {
				finish = canonical.Failed
			}
			message := canonical.NewMessage(canonical.Assistant)
			message.Sources = out.sources
			message.PartCount = uint64(len(out.blocks))
			for _, block := range out.blocks {
				// A response whose stream failed or was steered mid-flight never
				// requested its tools: the source reader emits calls only after
				// a completed stream.
				if block.Data != nil && (err == nil && !steered || block.Kind != "tool_request") {
					message.Content = append(message.Content, block)
				}
			}
			message.Model = &binding.Config.Model
			message.Finish = &finish
			out.record.Metadata.Runtime.OutputLimitReached = out.limited
			if empty && err == nil && x.ctx.Err() == nil && !steered && !out.limited {
				if successfulTool(state.Records) {
					out.record.Metadata.Runtime.UserVisible = false
				} else {
					message.Content = []canonical.Block{canonical.Text(0, "The model returned an empty response. Please resend your message to continue.")}
					message.PartCount = 1
					out.record.Metadata.Runtime.Notes = &canonical.OperationNotes{EmptyResponse: true}
				}
			}
			out.record.Data = &message
			if len(billing) > 0 && (len(message.Content) > 0 || !steered && (out.finished || out.limited)) {
				last := billing[len(billing)-1]
				billing = billing[:len(billing)-1]
				message.Usage = last.Message().Usage
				out.record.Metadata.Execution = last.Metadata.Execution
			}
			if len(billing) > 0 {
				if e := r.saveWithProvider(context.WithoutCancel(x.ctx), x.inv.Session.ID, &binding.Provider, billing...); e != nil {
					return e
				}
			}
			if len(message.Content) > 0 || !steered && (out.finished || out.limited) {
				if e := r.saveWithProvider(context.WithoutCancel(x.ctx), x.inv.Session.ID, &binding.Provider, out.record); e != nil {
					return e
				}
			}
			if x.ctx.Err() != nil {
				return x.ctx.Err()
			}
			if steered {
				return errQueuedSteer
			}
			if err != nil {
				failure := model.Classify(err)
				code := string(failure.Kind)
				failed := canonical.NewMessage(canonical.Assistant)
				failed.Error = &canonical.ResponseError{Code: &code, Message: failure.Message, RetryExhausted: failure.Exhausted, RetryWaitExhausted: failure.WaitExhausted}
				failed.Finish = &finish
				record := canonical.NewRecord(x.inv.Run.Turn, "message", failed, r.clock.Now())
				record.Provenance.Author = "assistant"
				runtime := canonical.DefaultRuntime()
				runtime.AgentVisible = false
				record.Metadata.Runtime = &runtime
				return r.save(x.ctx, x.inv.Session.ID, record)
			}
			if reads.adopt(out.blocks) {
				x.streamReads, reads = reads, nil
			}
			return nil
		}
		if len(billing) > 0 {
			if e := r.saveWithProvider(context.WithoutCancel(x.ctx), x.inv.Session.ID, &binding.Provider, billing...); e != nil {
				return e
			}
		}
	}
	return nil
}

func (out *streamOutput) slot(id, kind string) *canonical.Block {
	key := id + "\x00" + kind
	if index, ok := out.slots[key]; ok {
		return &out.blocks[index]
	}
	index := len(out.blocks)
	out.slots[key] = index
	block := canonical.Block{Index: uint64(index), Kind: kind}
	switch kind {
	case "text":
		block.Data = &canonical.TextContent{}
	case "thinking":
		block.Data = &canonical.Thinking{}
	case "tool_request":
		out.toolSlots = append(out.toolSlots, index)
	}
	out.blocks = append(out.blocks, block)
	return &out.blocks[index]
}

func (out *streamOutput) receive(part fantasy.StreamPart) error {
	if sources, ok := model.StreamSources(part); ok {
		out.sources = sources
	}
	if index, ok := model.PartIndex(part); ok {
		if out.indices == nil {
			out.indices = map[string]uint64{}
		}
		kind := ""
		switch part.Type {
		case fantasy.StreamPartTypeTextStart, fantasy.StreamPartTypeTextDelta, fantasy.StreamPartTypeTextEnd:
			kind = "text"
		case fantasy.StreamPartTypeReasoningStart, fantasy.StreamPartTypeReasoningDelta, fantasy.StreamPartTypeReasoningEnd:
			kind = "thinking"
		case fantasy.StreamPartTypeToolInputStart, fantasy.StreamPartTypeToolInputDelta, fantasy.StreamPartTypeToolInputEnd, fantasy.StreamPartTypeToolCall:
			kind = "tool_request"
		}
		if kind != "" {
			out.indices[part.ID+"\x00"+kind] = index
		}
	}
	switch part.Type {
	case fantasy.StreamPartTypeTextStart:
		out.slot(part.ID, "text")
	case fantasy.StreamPartTypeTextDelta:
		block := out.slot(part.ID, "text")
		block.Data.(*canonical.TextContent).Text += part.Delta
	case fantasy.StreamPartTypeReasoningStart:
		out.slot(part.ID, "thinking")
	case fantasy.StreamPartTypeReasoningDelta:
		block := out.slot(part.ID, "thinking")
		block.Data.(*canonical.Thinking).Text += part.Delta
	case fantasy.StreamPartTypeReasoningEnd:
		return model.ApplyStreamMetadata(out.slot(part.ID, "thinking"), part)
	case fantasy.StreamPartTypeToolInputStart, fantasy.StreamPartTypeToolInputDelta:
		out.slot(part.ID, "tool_request")
	case fantasy.StreamPartTypeToolCall:
		if out.calls[part.ID] {
			return nil
		}
		out.calls[part.ID] = true
		block := out.slot(part.ID, "tool_request")
		block.Data = &canonical.ToolRequest{ID: part.ID, Name: part.ToolCallName, Arguments: json.RawMessage(part.ToolCallInput)}
		return model.ApplyStreamMetadata(block, part)
	case fantasy.StreamPartTypeFinish:
		out.finished = true
		out.limited = part.FinishReason == fantasy.FinishReasonLength
	case fantasy.StreamPartTypeError:
		return part.Error
	}
	return nil
}

func (out *streamOutput) separateRepeatedCalls(records []canonical.Record) {
	if len(out.calls) == 0 {
		return
	}
	seen := map[string]bool{}
	for _, record := range records {
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				if call, ok := block.Data.(*canonical.ToolRequest); ok && out.calls[call.ID] {
					seen[call.ID] = true
				}
			}
		}
	}
	for position, block := range out.blocks {
		if call, ok := block.Data.(*canonical.ToolRequest); ok && seen[call.ID] {
			call.ID = canonical.ToolCallOccurrenceID(out.record.ID, position)
		}
	}
}

func (out *streamOutput) empty() bool {
	if out.limited {
		return false
	}
	for _, block := range out.blocks {
		switch value := block.Data.(type) {
		case *canonical.TextContent:
			if strings.TrimSpace(value.Text) != "" {
				return false
			}
		case *canonical.Thinking:
			if strings.TrimSpace(value.Text) != "" || value.Signature != nil && *value.Signature != "" {
				return false
			}
		case nil:
		default:
			return false
		}
	}
	return true
}
func successfulTool(records []canonical.Record) bool {
	last := lastMessage(records)
	if last == nil {
		return false
	}
	found := false
	for _, block := range last.Content {
		if value, ok := block.Data.(*canonical.ToolResponse); ok {
			found = true
			if value.Status != canonical.ToolSuccess {
				return false
			}
			if value.Format == canonical.MCPError {
				return false
			}
		}
	}
	return found
}

// quietRetryDelay is the longest wait, in milliseconds, of a first retry that
// shows no row.
const quietRetryDelay = 5000

// retryRecord is the observation of an attempt that waits to be sent again,
// which the conversation shows as the request's retry row. It is the attempt's
// only record: its usage and timing are not observed a second time.
//
// The first retry is a quick resend that most transient failures clear on, so
// it is numbered 0, which shows no row, and the rows count the retries after
// it. A first retry that must wait at least quietRetryDelay still shows.
func (r *Runtime) retryRecord(x *execution, binding model.Binding, purpose canonical.Purpose, attempt model.Attempt) canonical.Record {
	record := r.observationRecord(x, binding, purpose, attempt)
	number, limit, delay := attempt.Index, attempt.RetryLimit, uint64(attempt.RetryDelay.Milliseconds())
	if number == 0 && delay >= quietRetryDelay {
		number = 1
	}
	limit = max(limit-min(limit, 1), number)
	code := string(attempt.Failure.Kind)
	message := attempt.Failure.Message
	record.Metadata.Execution.Retry = &protocol.ConversationRecoveryRetry{Attempt: number, Limit: &limit, DelayMs: &delay, Status: attempt.Status, Code: &code, Error: &message}
	record.Metadata.Runtime.UserVisible = true
	return record
}

func (r *Runtime) observationRecord(x *execution, binding model.Binding, purpose canonical.Purpose, attempt model.Attempt) canonical.Record {
	message := canonical.NewMessage(canonical.Assistant)
	message.Model = &binding.Config.Model
	message.Usage = &attempt.Usage
	record := canonical.NewRecord(x.inv.Run.Turn, "message", message, attempt.Finished)
	record.Provenance.Author = "assistant"
	elapsed := uint64(max(0, attempt.Finished.Sub(attempt.Started).Microseconds()))
	timing := canonical.Timing{ElapsedUs: &elapsed}
	if attempt.FirstToken != nil {
		first := uint64(max(0, attempt.FirstToken.Sub(attempt.Started).Microseconds()))
		timing.FirstTokenUs = &first
	}
	observation := protocol.DefaultConversationObservationRequest()
	observation.ID = attempt.RequestID
	observation.Turn = x.inv.Run.Turn
	observation.Model = binding.Config.Model
	observation.TimestampMs = attempt.RequestStarted.UnixMilli()
	observed := protocol.DefaultConversationObservationAttempt()
	observed.Index = attempt.Index + 1
	observed.Transport = "http"
	observed.HeadersUs, observed.ResponseUs = attempt.ResponseUs, attempt.ResponseUs
	observed.RequestBytes, observed.DispatchUs, observed.RequestSentUs = attempt.RequestBytes, attempt.DispatchUs, attempt.RequestSentUs
	observed.Categories, observed.CompleteContext = attempt.Categories, attempt.CompleteContext
	observed.ResponsesBoundary = attempt.ResponsesBoundary
	observed.ReasoningStreamed = attempt.ReasoningStreamed
	observed.ElapsedUs = &elapsed
	observed.FirstTokenUs = timing.FirstTokenUs
	observed.Status = attempt.Status
	observed.Outcome = protocol.ConversationObservationOutcomeSucceeded
	if attempt.Failure != nil {
		observed.Outcome = protocol.ConversationObservationOutcomeFailed
		if attempt.Failure.Kind == "cancelled" {
			observed.Outcome = protocol.ConversationObservationOutcomeCancelled
		}
	}
	if attempt.Usage.Input != nil && attempt.Usage.Output != nil {
		usage := protocol.ConversationUsage{Input: *attempt.Usage.Input, Output: *attempt.Usage.Output}
		if attempt.Usage.CacheRead != nil {
			usage.CachedInput = *attempt.Usage.CacheRead
		}
		if attempt.Usage.Reasoning != nil {
			usage.Reasoning = *attempt.Usage.Reasoning
		}
		observed.Usage = &usage
	}
	observed.CacheReported = attempt.Usage.CacheRead != nil
	observed.ReasoningReported = attempt.Usage.Reasoning != nil
	observation.Attempts = []protocol.ConversationObservationAttempt{observed}
	scope := model.Scope(binding)
	selected := capturedModel(binding)
	if selected.Context > 0 {
		observation.ContextLimit = &selected.Context
	}
	record.Metadata.Execution = &canonical.Execution{Source: &scope, Purpose: &purpose, Timing: &timing, Observation: &observation, ContextTokens: attempt.Usage.Total}
	runtime := canonical.DefaultRuntime()
	runtime.UserVisible = false
	runtime.AgentVisible = false
	record.Metadata.Runtime = &runtime
	return record
}

func (out *streamOutput) order() {
	type slot struct {
		block    canonical.Block
		index    uint64
		kind     int
		position int
	}
	var slots []slot
	for key, position := range out.slots {
		block := out.blocks[position]
		if block.Data == nil {
			continue
		}
		index := uint64(position)
		if source, ok := out.indices[key]; ok {
			index = source
		}
		kind := 0
		switch block.Kind {
		case "thinking":
			kind = 1
		case "redacted_thinking":
			kind = 2
		case "tool_request":
			kind = 3
		}
		slots = append(slots, slot{block, index, kind, position})
	}
	// Arrival order settles ties; map iteration above has none.
	slices.SortFunc(slots, func(a, b slot) int {
		return cmp.Or(cmp.Compare(a.index, b.index), cmp.Compare(a.kind, b.kind), cmp.Compare(a.position, b.position))
	})
	out.blocks = nil
	for _, slot := range slots {
		slot.block.Index = uint64(len(out.blocks))
		out.blocks = append(out.blocks, slot.block)
	}
	for i, block := range out.blocks {
		if redacted, ok := block.Data.(*canonical.RedactedThinking); ok {
			var before *uint64
			if i+1 < len(out.blocks) {
				next := out.blocks[i+1].Index
				before = &next
			}
			redacted.Before = make([]*uint64, len(redacted.Data))
			for j := range redacted.Before {
				redacted.Before[j] = before
			}
		}
	}
}

// mainRequest assembles the system prompt, declared tools and projected
// history of a main model step. Compaction reuses it unchanged so a summary
// request shares the provider prompt cache of the conversation it replaces.
func (r *Runtime) mainRequest(x *execution, binding model.Binding, state store.AgentState, instruction string) (model.Request, error) {
	system := prompt.System(instruction, state.Context)
	tools := r.declarations(x, binding, state)
	schemas := map[string]json.RawMessage{}
	for _, tool := range tools {
		if captured := findTool(x.tools, tool.Name); captured != nil && len(captured.SchemaJSON) > 0 {
			schemas[tool.Name] = captured.SchemaJSON
		}
	}
	projected := model.VisibleAnsweredRecords(state.Records)
	if err := r.hydrateModelToolResults(x.ctx, projected); err != nil {
		return model.Request{}, err
	}
	if !x.sessionRoute {
		projectContextPressure(projected, state.Records, tools)
	}
	projected = x.output.Project(projected)
	hydrated, attachmentAssets, err := r.hydrateAttachments(x, projected)
	if err != nil {
		return model.Request{}, err
	}
	projected = hydrated
	assets, err := r.modelAssetsWith(x.ctx, projected, attachmentAssets)
	if err != nil {
		return model.Request{}, err
	}
	projected, tools, schemas = x.names.request(projected, tools, schemas)
	if err = r.toolMediaAssets(x.ctx, binding, projected, assets); err != nil {
		return model.Request{}, err
	}
	return model.Request{System: system, Records: projected, Tools: tools, ToolSchemas: schemas, Assets: assets, Delegated: x.inv.Session.Delegation != nil}, nil
}
