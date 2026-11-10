package engine

import (
	"cmp"
	"context"
	"encoding/json"
	"slices"
	"strings"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	handoff "github.com/sailry/sailry-harness/internal/node/agent/compact"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/observation"
	"github.com/sailry/sailry-harness/internal/node/agent/prompt"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type streamOutput struct {
	sources  canonical.Sources
	record   canonical.Record
	blocks   []canonical.Block
	slots    map[string]int
	indices  map[string]uint64
	calls    map[string]bool
	finished bool
	limited  bool
	usage    canonical.Usage
}

func (r *Runtime) infer(x *execution, binding model.Binding, languageModel fantasy.LanguageModel, state store.AgentState) error {
	parts := []string{prompt.Workflow(binding.Config.Mode, x.inv.Session.Delegation != nil)}
	parts = append(parts, r.delegationContext(x, state), x.skillContext, childBudgetContext(x, state))
	if state.Context.Environment != nil {
		parts = append(parts, *state.Context.Environment)
	}
	if title := prompt.TitleInstruction(state.Context.Title, toolCapable(binding), x.inv.Session.Delegation != nil); title != nil {
		parts = append(parts, *title)
	}
	for _, instruction := range x.inv.Instructions {
		if len(instruction.AnyTools) == 0 {
			parts = append(parts, instruction.Text)
		}
	}
	ownedParts := []string{}
	for _, tool := range x.tools {
		for _, part := range tool.ContextParts {
			if !slices.Contains(ownedParts, part) {
				ownedParts = append(ownedParts, part)
				parts = append(parts, part)
			}
		}
	}
	parts = prompt.Pending(state.Records, ownedParts, parts)
	if contextRecord := prompt.ContextMessage(x.inv.Run.Turn, parts, ownedParts, r.clock.Now()); contextRecord != nil {
		if err := r.save(x.ctx, x.inv.Session.ID, *contextRecord); err != nil {
			return err
		}
		state.Records = append(state.Records, *contextRecord)
	}
	instruction := prompt.Instruction(x.inv.Session.Delegation != nil)
	system := prompt.System(instruction, state.Context, nil)
	tools := r.declarations(x, state)
	schemas := map[string]json.RawMessage{}
	for _, tool := range tools {
		if captured := findTool(x.tools, tool.Name); captured != nil && len(captured.SchemaJSON) > 0 {
			schemas[tool.Name] = captured.SchemaJSON
		}
	}
	projected := x.output.Project(state.Records)
	hydrated, attachmentAssets, err := r.hydrateAttachments(x, projected)
	if err != nil {
		return err
	}
	projected = hydrated
	assets, err := r.modelAssetsWith(x.ctx, projected, attachmentAssets)
	if err != nil {
		return err
	}
	call, err := model.BuildCall(binding, model.Request{System: system, Records: projected, Tools: tools, ToolSchemas: schemas, Assets: assets})
	if err != nil {
		return err
	}
	inventory := &observation.Inventory{System: system}
	for _, part := range prompt.SystemParts(instruction, state.Context, nil)[1:] {
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
	for emptyAttempt := 0; emptyAttempt <= 3; emptyAttempt++ {
		out := streamOutput{record: canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant), r.clock.Now()), slots: map[string]int{}, calls: map[string]bool{}}
		out.record.Provenance.Author = "assistant"
		scope := model.Scope(binding)
		purpose := canonical.PurposeMain
		out.record.Metadata.Execution = &canonical.Execution{Source: &scope, Purpose: &purpose}
		out.record.Metadata.Runtime = &canonical.Runtime{UserVisible: true, AgentVisible: true, Inference: &canonical.Inference{Provider: "sailry:" + binding.Provider.ID.String(), RequestedModel: binding.Config.Model}}
		var streamErr error
		var attempts []model.Attempt
		escaped := false
		err = model.Run(x.ctx, languageModel, call, model.Main, r.clock, func(attempt model.Attempt) error {
			out.usage = attempt.Usage
			if attempt.Failure != nil && attempt.Failure.Retryable() && !escaped && attempt.Index < 5 {
				record := r.observationRecord(x, binding, purpose, attempt)
				limit, delay := uint64(5), uint64(60000)
				code := string(attempt.Failure.Kind)
				message := attempt.Failure.Message
				record.Metadata.Execution.Retry = &protocol.ConversationRecoveryRetry{Attempt: attempt.Index + 1, Limit: &limit, DelayMs: &delay, Status: attempt.Status, Code: &code, Error: &message}
				record.Metadata.Runtime.UserVisible = true
				return r.save(context.WithoutCancel(x.ctx), x.inv.Session.ID, record)
			}
			attempts = append(attempts, attempt)
			return nil
		}, func(part fantasy.StreamPart) bool {
			escaped = true
			if x.ctx.Err() != nil {
				return false
			}
			if e := out.receive(part); e != nil {
				streamErr = e
				return false
			}
			if (part.Type == fantasy.StreamPartTypeTextDelta || part.Type == fantasy.StreamPartTypeReasoningDelta) && part.Delta != "" {
				kind := "text"
				if part.Type == fantasy.StreamPartTypeReasoningDelta {
					kind = "thinking"
				}
				streamErr = r.feed.Delta(x.inv.Session.ID, protocol.ConversationDraft{ID: out.record.ID, Turn: x.inv.Run.Turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: kind, Data: part.Delta}}})
			}
			return streamErr == nil
		})
		if streamErr != nil {
			err = streamErr
		}
		out.order()
		empty := out.empty()
		var billing []canonical.Record
		for _, attempt := range attempts {
			billing = append(billing, r.observationRecord(x, binding, purpose, attempt))
		}
		if x.ctx.Err() != nil || err != nil || !empty || emptyAttempt == 3 || out.limited || successfulTool(state.Records) {
			finish := canonical.Completed
			if x.ctx.Err() != nil {
				finish = canonical.Interrupted
			} else if err != nil {
				finish = canonical.Failed
			}
			message := canonical.NewMessage(canonical.Assistant)
			message.Sources = out.sources
			message.PartCount = uint64(len(out.blocks))
			for _, block := range out.blocks {
				if block.Data != nil {
					message.Content = append(message.Content, block)
				}
			}
			message.Model = &binding.Config.Model
			message.Finish = &finish
			out.record.Metadata.Runtime.OutputLimitReached = out.limited
			if empty && err == nil && x.ctx.Err() == nil && !out.limited {
				if successfulTool(state.Records) {
					out.record.Metadata.Runtime.UserVisible = false
				} else {
					message.Content = []canonical.Block{canonical.Text(0, "The model returned an empty response. Please resend your message to continue.")}
					message.PartCount = 1
					out.record.Metadata.Runtime.Notes = &canonical.OperationNotes{EmptyResponse: true}
				}
			}
			out.record.Data = &message
			if len(billing) > 0 && (len(message.Content) > 0 || out.finished || out.limited) {
				last := billing[len(billing)-1]
				billing = billing[:len(billing)-1]
				message.Usage = last.Message().Usage
				out.record.Metadata.Execution = last.Metadata.Execution
			}
			if len(billing) > 0 {
				if e := r.save(context.WithoutCancel(x.ctx), x.inv.Session.ID, billing...); e != nil {
					return e
				}
			}
			if len(message.Content) > 0 || out.finished || out.limited {
				if e := r.save(context.WithoutCancel(x.ctx), x.inv.Session.ID, out.record); e != nil {
					return e
				}
			}
			if x.ctx.Err() != nil {
				return x.ctx.Err()
			}
			if err != nil {
				failure := model.Classify(err)
				code := string(failure.Kind)
				failed := canonical.NewMessage(canonical.Assistant, canonical.Text(0, failure.Message))
				failed.Error = &canonical.ResponseError{Code: &code, Message: failure.Message}
				failed.Finish = &finish
				record := canonical.NewRecord(x.inv.Run.Turn, "message", failed, r.clock.Now())
				record.Provenance.Author = "assistant"
				runtime := canonical.DefaultRuntime()
				runtime.AgentVisible = false
				record.Metadata.Runtime = &runtime
				return r.save(x.ctx, x.inv.Session.ID, record)
			}
			return nil
		}
		if len(billing) > 0 {
			if e := r.save(x.ctx, x.inv.Session.ID, billing...); e != nil {
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
func toolCapable(binding model.Binding) bool {
	for _, m := range binding.Provider.Models {
		if m.ID == binding.Config.Model {
			return m.Tools
		}
	}
	return false
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
	observed.Categories, observed.CompleteContext = attempt.Categories, attempt.CompleteContext
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
		block canonical.Block
		index uint64
		kind  int
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
		slots = append(slots, slot{block, index, kind})
	}
	slices.SortStableFunc(slots, func(a, b slot) int {
		if n := cmp.Compare(a.index, b.index); n != 0 {
			return n
		}
		return cmp.Compare(a.kind, b.kind)
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
