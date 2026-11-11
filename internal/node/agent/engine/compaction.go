// Adapted from Goose ee8500d5 and 14741e9a (Apache-2.0).
// See ../model/LICENSE.goose.
package engine

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	handoff "github.com/sailry/sailry-harness/internal/node/agent/compact"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/output"
	"github.com/sailry/sailry-harness/internal/node/agent/tokens"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func capturedModel(binding model.Binding) protocol.ConversationModel {
	for _, candidate := range binding.Provider.Models {
		if candidate.ID == binding.Config.Model {
			return candidate
		}
	}
	return protocol.DefaultConversationModel()
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
				return *message.Usage.Total + pendingTokens(records[i+1:])
			}
			// Only the latest observation can describe the current context. An
			// incomplete report or summary usage cannot reuse an older total.
			break
		}
	}
	return pendingTokens(records[start:])
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
func shouldCompact(binding model.Binding, state store.AgentState, percent uint8) bool {
	last := lastMessage(state.Records)
	if last == nil || last.Role == canonical.Assistant {
		return false
	}
	selected := capturedModel(binding)
	limit := uint64(selected.Context)
	budget := store.AgentCompactionBudget(selected, percent)
	if budget == nil || budget.TriggerTokens == 0 || budget.TriggerTokens >= limit {
		return false
	}
	count := contextTokens(state.Records)
	return count > budget.TriggerTokens
}
func (r *Runtime) compact(x *execution, binding model.Binding, state store.AgentState, manual bool) error {
	summaryID := protocol.NewRequestID().String()
	if !manual {
		progress, _ := json.Marshal(map[string]any{"type": "compacting", "started_ms": r.clock.Now().UnixMilli()})
		_ = r.feed.Delta(x.inv.Session.ID, protocol.ConversationDraft{ID: summaryID, Turn: x.inv.Run.Turn, Author: "sailry", Parts: []protocol.ConversationPart{{Kind: "resource", Data: json.RawMessage(progress)}}})
	}
	binding, err := r.credential(x, binding)
	if err != nil {
		return err
	}
	selected := capturedModel(binding)
	summaryBinding := model.Summary(binding)
	output := model.OutputLimit(summaryBinding, true)
	requestText := handoff.Request
	if manual && strings.TrimSpace(x.inv.Message.Text) != "" {
		requestText += "\n\nUser focus for the handoff:\n" + x.inv.Message.Text
	}
	system, err := handoff.Prepare(state.Records, uint64(selected.Context), uint64(output), requestText)
	if err != nil {
		return err
	}
	languageModel, err := r.model(x.ctx, summaryBinding)
	if err != nil {
		return err
	}
	request := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, requestText)), r.clock.Now())
	call, err := model.BuildCall(summaryBinding, model.Request{System: system, Records: []canonical.Record{request}, MaxOutputTokens: &output})
	if err != nil {
		return err
	}
	var text strings.Builder
	limited, textBlock := false, false
	var usage canonical.Usage
	var attempts []model.Attempt
	err = model.Run(x.ctx, languageModel, call, model.SummaryPurpose, r.clock, func(attempt model.Attempt) error {
		usage = attempt.Usage
		attempts = append(attempts, attempt)
		return nil
	}, func(part fantasy.StreamPart) bool {
		switch part.Type {
		case fantasy.StreamPartTypeTextDelta:
			if !textBlock && text.Len() > 0 {
				text.WriteByte('\n')
			}
			text.WriteString(part.Delta)
			textBlock = true
		case fantasy.StreamPartTypeReasoningDelta, fantasy.StreamPartTypeReasoningEnd, fantasy.StreamPartTypeToolCall:
			textBlock = false
		case fantasy.StreamPartTypeFinish:
			switch strings.ToLower(string(part.FinishReason)) {
			case "length", "max_tokens", "max_output_tokens":
				limited = true
			}
		}
		return x.ctx.Err() == nil
	})
	failed := func(failure error) error {
		var records []canonical.Record
		for _, attempt := range attempts {
			records = append(records, r.observationRecord(x, summaryBinding, canonical.PurposeCompaction, attempt))
		}
		if e := r.save(context.WithoutCancel(x.ctx), x.inv.Session.ID, records...); e != nil {
			return e
		}
		return failure
	}
	if err != nil {
		return failed(err)
	}
	if limited {
		return failed(fmt.Errorf("compaction summary reached its output limit"))
	}
	summary, err := handoff.Parse(text.String())
	if err != nil {
		return failed(err)
	}
	rendered := summary.Render()
	// Missing counters describe the bounded JSON document before rendering.
	if usage.Input == nil {
		input := tokens.Chat(system, []canonical.Record{request})
		usage.Input = &input
	}
	if usage.Output == nil {
		output := tokens.Text(summary.Document())
		usage.Output = &output
	}
	if usage.Input != nil && usage.Output != nil {
		total := *usage.Input + *usage.Output
		usage.Total = &total
	}
	var boundary *canonical.Record
	for i := len(state.Records) - 1; i >= 0; i-- {
		if state.Records[i].Message() != nil {
			boundary = &state.Records[i]
			break
		}
	}
	if boundary == nil {
		return failed(fmt.Errorf("conversation has no compaction boundary"))
	}
	message := canonical.NewMessage(canonical.User, canonical.Text(0, rendered))
	message.Model = &binding.Config.Model
	message.Usage = &usage
	// The last summary attempt is billed on the summary; earlier attempts retain
	// their independent observations.
	compacted := canonical.Compaction{Boundary: boundary.ID, Summary: message, Start: state.Records[0].Created, End: boundary.Created}
	for _, file := range summary.Files {
		description := file.Summary
		if file.KeyCode != nil {
			description += "\n" + *file.KeyCode
		}
		compacted.Files = append(compacted.Files, canonical.CompactedFile{Path: file.Path, Summary: &description})
	}
	record := canonical.NewRecord(x.inv.Run.Turn, "compaction", &compacted, r.clock.Now())
	record.ID = summaryID
	record.Provenance.Author = "system"
	runtime := canonical.DefaultRuntime()
	runtime.UserVisible = false
	record.Metadata.Runtime = &runtime
	loaded := retainedTools(state)
	record.Metadata.Context = &canonical.Context{LoadedTools: &loaded}
	continuation := "Your context was compacted. The previous message contains a summary of the conversation so far.\nDo not mention that you read a summary or that conversation summarization occurred.\nContinue calling tools as necessary to complete the task."
	if manual {
		continuation = "Your context was compacted at the user's request. The previous message contains a summary of the conversation so far.\nDo not mention that you read a summary or that conversation summarization occurred.\nJust continue the conversation naturally based on the summarized context."
	}
	var preserved *canonical.Message
	if !manual {
		visible := handoff.Visible(state.Records)
		summaries := map[string]bool{}
		for _, record := range state.Records {
			if record.Kind == "compaction" {
				summaries[record.ID] = true
			}
		}
		for i := len(visible) - 1; i >= 0; i-- {
			if summaries[visible[i].ID] || compactionUsage(visible[i]) || visible[i].Metadata.Runtime != nil && visible[i].Metadata.Runtime.TurnContext {
				continue
			}
			candidate := visible[i].Message()
			if candidate != nil && candidate.Role == canonical.User && !containsTool(candidate) {
				for _, block := range candidate.Content {
					if block.Kind == "text" {
						preserved = candidate
						break
					}
				}
				if preserved != nil {
					if visible[i].ID == boundary.ID {
						continuation = "Your context was compacted. The previous message contains a summary of the conversation so far.\nDo not mention that you read a summary or that conversation summarization occurred.\nJust continue the conversation naturally based on the summarized context."
					}
					break
				}
			}
		}
	}
	continued := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, continuation)), r.clock.Now())
	continued.Provenance.Author = "assistant"
	continued.Metadata.Runtime = &runtime
	var records []canonical.Record
	if len(attempts) > 0 {
		for _, attempt := range attempts[:len(attempts)-1] {
			records = append(records, r.observationRecord(x, binding, canonical.PurposeCompaction, attempt))
		}
		last := r.observationRecord(x, binding, canonical.PurposeCompaction, attempts[len(attempts)-1])
		record.Metadata.Execution = last.Metadata.Execution
	}
	records = append(records, record, continued)
	if preserved != nil {
		var blocks []canonical.Block
		for _, block := range preserved.Content {
			if block.Kind == "text" {
				block.Index = uint64(len(blocks))
				blocks = append(blocks, block)
			}
		}
		carry := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, blocks...), r.clock.Now())
		carry.Provenance.Author = "assistant"
		carry.Metadata.Runtime = &runtime
		records = append(records, carry)
	}
	return r.save(x.ctx, x.inv.Session.ID, records...)
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
