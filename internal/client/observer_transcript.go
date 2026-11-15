package client

import (
	"cmp"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// TranscriptInput is one visible user input at its canonical history position.
// Joined guidance keeps its own identity while belonging to the recipient's turn.
type TranscriptInput struct {
	Turn        protocol.TurnID            `json:"turn"`
	Message     protocol.ConversationInput `json:"message"`
	Attachments []protocol.Attachment      `json:"attachments"`
	Canonical   bool                       `json:"canonical"`
	Sequence    uint64                     `json:"sequence"`
	TimestampMs int64                      `json:"timestamp_ms"`
	// ImageIDs preserves the canonical part kind for native attachment views.
	ImageIDs []protocol.AttachmentID `json:"-"`
}

type transcriptRow struct {
	Source       toolReference             `json:"source"`
	Draft        bool                      `json:"draft"`
	End          int                       `json:"end"`
	Part         protocol.ConversationPart `json:"part"`
	TextOffset   int                       `json:"text_offset"`
	TextLast     bool                      `json:"text_last"`
	Continuation bool                      `json:"continuation"`
	Work         bool                      `json:"work"`
	sequence     uint64
}

type transcriptSegment struct {
	Inputs  []TranscriptInput `json:"inputs"`
	Rows    []transcriptRow   `json:"rows"`
	Waiting *int              `json:"waiting,omitempty"`
}

func emptySegment() transcriptSegment {
	return transcriptSegment{Inputs: []TranscriptInput{}, Rows: []transcriptRow{}}
}

func projectGuidance(page protocol.ConversationPage) map[string]string {
	value := map[string]string{}
	canonical := map[protocol.TurnID]bool{}
	for _, entry := range page.Entries {
		if entry.Author == "user" {
			canonical[entry.Turn] = true
		}
	}
	queued := map[protocol.TurnID]protocol.ConversationPending{}
	for _, input := range page.Queue.Items {
		queued[input.Turn] = input
	}
	for _, run := range page.Runs {
		input, exists := queued[run.Turn]
		recipient := run.Guidance
		if recipient == nil && exists {
			recipient = input.Guidance
		}
		if recipient == nil {
			continue
		}
		state := "next_turn"
		if canonical[run.Turn] && run.Joined != nil && *run.Joined == *recipient {
			state = "delivered"
		} else if exists {
			if input.Guidance != nil {
				state = "pending"
			}
		} else if run.Joined != nil && *run.Joined == *recipient || run.Status == "queued" {
			state = "pending"
		}
		value[run.Turn.String()] = state
	}
	for _, input := range page.Queue.Items {
		if input.Guidance != nil && value[input.Turn.String()] == "" {
			value[input.Turn.String()] = "pending"
		}
	}
	return value
}

func projectTranscript(snapshot protocol.ConversationSnapshot, calls []ToolCall, guidance map[string]string) ([]protocol.TurnID, map[string]protocol.TurnID, map[string]any) {
	page := snapshot.Page
	runs := slices.Clone(page.Runs)
	slices.SortFunc(runs, func(left, right protocol.ConversationRun) int {
		startLeft, startRight := left.StartedMs, right.StartedMs
		if startLeft == nil {
			startLeft = left.FinishedMs
		}
		if startRight == nil {
			startRight = right.FinishedMs
		}
		if startLeft != nil && startRight == nil {
			return -1
		}
		if startLeft == nil && startRight != nil {
			return 1
		}
		if startLeft != nil && startRight != nil {
			if order := cmp.Compare(*startLeft, *startRight); order != 0 {
				return order
			}
		}
		return cmp.Compare(left.Sequence, right.Sequence)
	})
	known := map[protocol.TurnID]bool{}
	for _, run := range page.Runs {
		known[run.Turn] = true
	}
	timeline := []protocol.TurnID{}
	owners := map[string]protocol.TurnID{}
	segments := map[string]any{}
	inputs := TranscriptInputs(page)
	for _, run := range runs {
		owner := run.Turn
		if run.Joined != nil && known[*run.Joined] {
			owner = *run.Joined
		}
		owners[run.Turn.String()] = owner
		if owner != run.Turn || guidance[run.Turn.String()] == "pending" || !visibleTranscriptRun(snapshot, run) {
			continue
		}
		timeline = append(timeline, run.Turn)
		segments[run.Turn.String()] = transcriptSegments(snapshot, calls, guidance, run.Turn, inputs[run.Turn])
	}
	return timeline, owners, segments
}

func visibleTranscriptRun(snapshot protocol.ConversationSnapshot, run protocol.ConversationRun) bool {
	if run.Continuation == nil || run.Status != "continued" {
		return true
	}
	parts := []protocol.ConversationPart{}
	for _, entry := range snapshot.Page.Entries {
		if entry.Turn == run.Turn && entry.Author != "user" {
			parts = append(parts, entry.Parts...)
		}
	}
	for _, draft := range snapshot.Drafts {
		if draft.Turn == run.Turn {
			parts = append(parts, draft.Parts...)
		}
	}
	for _, part := range parts {
		if part.Kind == "text" || part.Kind == "thinking" {
			if text, err := payload[string](part.Data); err == nil && strings.TrimSpace(text) != "" {
				return true
			}
		} else if part.Kind == "resource" {
			if resourceValue(part.Data).Type == "compaction_failure" {
				return true
			}
		} else {
			return true
		}
	}
	return false
}

func turnInputs(entries []protocol.ConversationEntry, run protocol.ConversationRun) []TranscriptInput {
	value := []TranscriptInput{}
	if run.Notice {
		return value
	}
	for _, entry := range entries {
		input := TranscriptInput{Turn: run.Turn, Message: protocol.DefaultConversationInput(), Attachments: []protocol.Attachment{}, Canonical: true, Sequence: entry.Sequence, TimestampMs: entry.TimestampMs}
		for _, part := range entry.Parts {
			switch part.Kind {
			case "text":
				if text, err := payload[string](part.Data); err == nil {
					input.Message.Text += text
				}
			case "attachment":
				if attachment, err := payload[protocol.Attachment](part.Data); err == nil {
					input.Message.Attachments = append(input.Message.Attachments, attachment.ID)
					input.Attachments = append(input.Attachments, attachment)
				}
			case "image":
				if image, err := payload[protocol.ConversationImage](part.Data); err == nil {
					input.Message.Attachments = append(input.Message.Attachments, image.Attachment.ID)
					input.Attachments = append(input.Attachments, image.Attachment)
					input.ImageIDs = append(input.ImageIDs, image.Attachment.ID)
				}
			case "reference":
				if reference, err := payload[protocol.ConversationReference](part.Data); err == nil {
					input.Message.References = append(input.Message.References, reference)
				}
			}
		}
		value = append(value, input)
	}
	if len(value) == 0 && run.Input != nil {
		input := TranscriptInput{Turn: run.Turn, Message: run.Input.Message, Attachments: run.Input.Attachments, Sequence: run.Sequence}
		if run.StartedMs != nil {
			input.TimestampMs = *run.StartedMs
		}
		value = append(value, input)
	}
	return value
}

// TranscriptInputs groups inputs by their executing turn, in presentation order.
// Desktop and Mobile share these boundaries; admission order cannot place
// guidance that was consumed later ahead of the assistant's earlier output.
func TranscriptInputs(page protocol.ConversationPage) map[protocol.TurnID][]TranscriptInput {
	guidance := projectGuidance(page)
	entries := map[protocol.TurnID][]protocol.ConversationEntry{}
	for _, entry := range page.Entries {
		if entry.Author == "user" {
			entries[entry.Turn] = append(entries[entry.Turn], entry)
		}
	}
	known := map[protocol.TurnID]bool{}
	for _, run := range page.Runs {
		known[run.Turn] = true
	}
	inputs := map[protocol.TurnID][]TranscriptInput{}
	for _, run := range page.Runs {
		if run.Continuation != nil || run.Notice || guidance[run.Turn.String()] == "pending" {
			continue
		}
		owner := run.Turn
		if run.Joined != nil && known[*run.Joined] {
			owner = *run.Joined
		}
		inputs[owner] = append(inputs[owner], turnInputs(entries[run.Turn], run)...)
	}
	for owner, group := range inputs {
		position := func(input TranscriptInput) int {
			if input.Turn == owner {
				return 0
			}
			if input.Canonical {
				return 1
			}
			return 2
		}
		slices.SortStableFunc(group, func(left, right TranscriptInput) int {
			if order := cmp.Compare(position(left), position(right)); order != 0 {
				return order
			}
			return cmp.Compare(left.Sequence, right.Sequence)
		})
		inputs[owner] = group
	}
	return inputs
}

func transcriptSegments(snapshot protocol.ConversationSnapshot, calls []ToolCall, guidance map[string]string, turn protocol.TurnID, inputs []TranscriptInput) []transcriptSegment {
	rows := collectTranscriptRows(snapshot, calls, turn)
	segments := []transcriptSegment{}
	current := emptySegment()
	claimed := []TranscriptInput{}
	cursor := 0
	for _, input := range inputs {
		switch {
		case input.Turn == turn:
			current.Inputs = append(current.Inputs, input)
		case !input.Canonical:
			claimed = append(claimed, input)
		default:
			for cursor < len(rows) && !rows[cursor].Draft && rows[cursor].sequence < input.Sequence {
				current.Rows = append(current.Rows, rows[cursor])
				cursor++
			}
			if len(current.Rows) > 0 {
				segments = append(segments, current)
				current = emptySegment()
			}
			current.Inputs = append(current.Inputs, input)
		}
	}
	current.Rows = append(current.Rows, rows[cursor:]...)
	current.Inputs = append(current.Inputs, claimed...)
	current.Waiting = waitingChildren(snapshot, calls, guidance, turn, current.Rows)
	return append(segments, current)
}

func collectTranscriptRows(snapshot protocol.ConversationSnapshot, calls []ToolCall, turn protocol.TurnID) []transcriptRow {
	selected := map[toolReference]ToolCall{}
	for _, call := range calls {
		if call.Turn == turn {
			selected[call.Source] = call
		}
	}
	rows := []transcriptRow{}
	retries := map[string]int{}
	appendParts := func(id string, parts []protocol.ConversationPart, sequence uint64, draft bool) {
		offset, lastText := 0, -1
		for index, part := range parts {
			if part.Kind == "text" {
				lastText = index
			}
		}
		for index := 0; index < len(parts); {
			start := index
			part := parts[index]
			index++
			row := transcriptRow{
				Source: toolReference{Entry: id, Index: uint64(start)}, Draft: draft, End: index, Part: part, TextOffset: offset,
				Continuation: start > 0 && parts[start-1].Kind == "image" && part.Kind == "image", sequence: sequence,
			}
			if part.Kind == "text" || part.Kind == "thinking" {
				text, _ := payload[string](part.Data)
				for index < len(parts) && parts[index].Kind == part.Kind {
					next, _ := payload[string](parts[index].Data)
					text += next
					index++
				}
				row.End, row.TextLast = index, lastText >= start && lastText < index
				row.Part.Data = text
				if part.Kind == "text" {
					offset += utf8.RuneCountInString(text)
					if strings.TrimSpace(text) == "" {
						continue
					}
				}
			}
			call, matched := selected[row.Source]
			if part.Kind == "tool_result" {
				if result, err := payload[protocol.ConversationPartToolResultPayload](part.Data); err == nil {
					for _, image := range result.Images {
						imageRow := row
						imageRow.Part = protocol.ConversationPart{Kind: "image", Data: image}
						imageRow.TextOffset, imageRow.TextLast, imageRow.Continuation, imageRow.Work = offset, false, false, false
						rows = append(rows, imageRow)
					}
				}
				if !matched {
					continue
				}
			}
			if part.Kind == "tool_call" || part.Kind == "tool_result" {
				if !matched || toolHidden(call) {
					continue
				}
				if call.Name == "compact_context" && call.Summary != nil && call.Summary.SummaryID != nil {
					hasSummary := false
					for _, entry := range snapshot.Page.Entries {
						if entry.ID == *call.Summary.SummaryID {
							hasSummary = slices.ContainsFunc(entry.Parts, func(part protocol.ConversationPart) bool { return part.Kind == "compaction" })
						}
					}
					if hasSummary {
						continue
					}
				}
			}
			if part.Kind == "resource" {
				value := resourceValue(part.Data)
				if value.Type != "model_retry" && value.Type != "compaction_failure" && value.Type != "compacting" {
					continue
				}
				if value.Type == "model_retry" && value.ID != "" && !value.Fallback {
					if position, exists := retries[value.ID]; exists {
						rows[position].Part = part
						continue
					}
					retries[value.ID] = len(rows)
				}
			}
			row.Work = workPart(part)
			rows = append(rows, row)
		}
	}
	for _, entry := range snapshot.Page.Entries {
		if entry.Turn == turn && entry.Author != "user" {
			appendParts(entry.ID, entry.Parts, entry.Sequence, false)
		}
	}
	for _, draft := range snapshot.Drafts {
		if draft.Turn == turn && draft.Author != "user" {
			appendParts(draft.ID, draft.Parts, 0, true)
		}
	}
	return rows
}

func waitingChildren(snapshot protocol.ConversationSnapshot, calls []ToolCall, guidance map[string]string, turn protocol.TurnID, rows []transcriptRow) *int {
	page := snapshot.Page
	running := slices.ContainsFunc(page.Runs, func(run protocol.ConversationRun) bool {
		return run.Turn == turn && run.Kind == "task" && run.Status == "running"
	})
	if !running || slices.ContainsFunc(snapshot.Drafts, func(draft protocol.ConversationDraft) bool { return draft.Turn == turn && draft.Author != "user" }) ||
		slices.ContainsFunc(page.Approvals, func(item protocol.ConversationApproval) bool { return item.Turn == turn && item.State == "pending" }) ||
		slices.ContainsFunc(page.Questions, func(item protocol.ConversationQuestion) bool {
			return item.Turn == turn && item.State.Kind == "pending"
		}) {
		return nil
	}
	for _, run := range page.Runs {
		recipient := run.Guidance
		if recipient == nil {
			for _, item := range page.Queue.Items {
				if item.Turn == run.Turn {
					recipient = item.Guidance
					break
				}
			}
		}
		if recipient != nil && *recipient == turn && guidance[run.Turn.String()] == "pending" {
			return nil
		}
	}
	for _, item := range page.Queue.Items {
		if item.Guidance != nil && *item.Guidance == turn && guidance[item.Turn.String()] == "pending" {
			return nil
		}
	}
	settled := false
	for index := len(rows) - 1; index >= 0; index-- {
		row := rows[index]
		if row.Draft {
			return nil
		}
		switch row.Part.Kind {
		case "tool_call":
			found := false
			for _, call := range calls {
				if call.Turn == turn && call.Source == row.Source {
					if call.State == "waiting" || call.State == "running" {
						return nil
					}
					found = true
					break
				}
			}
			if !found {
				return nil
			}
			settled = true
		case "tool_result":
		case "text", "thinking", "image":
			settled = true
			index = -1
		default:
			return nil
		}
	}
	if !settled {
		return nil
	}
	matching := []protocol.ConversationChild{}
	for _, child := range page.Children {
		if child.Origin.Session == page.Session && child.Origin.Turn == turn {
			matching = append(matching, child)
		}
	}
	count := 0
	for _, child := range LatestConversationChildren(matching) {
		if child.Run.Status != "queued" && child.Run.Status != "running" && child.Run.Status != "stopping" {
			continue
		}
		background := false
		for _, entry := range page.Entries {
			if entry.Turn == child.Origin.Turn && entry.ID == child.Origin.Entry && child.Origin.Index < uint64(len(entry.Parts)) {
				part := entry.Parts[child.Origin.Index]
				if part.Kind == "tool_call" {
					if call, err := payload[protocol.ConversationPartToolCallPayload](part.Data); err == nil && call.Summary != nil {
						background = call.Summary.Delegation.Background
					}
				}
			}
		}
		if !background {
			count++
		}
	}
	if count == 0 {
		return nil
	}
	return &count
}

type referencedImage struct {
	Call  string                     `json:"call"`
	Index uint64                     `json:"index"`
	Image protocol.ConversationImage `json:"image"`
}

func referencedImages(snapshot protocol.ConversationSnapshot) []referencedImage {
	texts := []string{}
	for _, entry := range snapshot.Page.Entries {
		if entry.Author != "user" {
			for _, part := range entry.Parts {
				if part.Kind == "text" {
					text, _ := payload[string](part.Data)
					texts = append(texts, text)
				}
			}
		}
	}
	for _, draft := range snapshot.Drafts {
		if draft.Author != "user" {
			for _, part := range draft.Parts {
				if part.Kind == "text" {
					text, _ := payload[string](part.Data)
					texts = append(texts, text)
				}
			}
		}
	}
	value := []referencedImage{}
	seen := map[protocol.ConversationImagesReference]bool{}
	for _, text := range texts {
		for _, reference := range imageReferences(text) {
			if seen[reference] {
				continue
			}
			image := resolveReferencedImage(snapshot.Page, reference)
			if image != nil {
				seen[reference] = true
				value = append(value, referencedImage{Call: reference.Call, Index: reference.Index, Image: *image})
			}
		}
	}
	return value
}

func imageReferences(text string) []protocol.ConversationImagesReference {
	value := []protocol.ConversationImagesReference{}
	for _, span := range exportImageSpans(text) {
		value = append(value, protocol.ConversationImagesReference{Call: span.key.call, Index: span.key.index})
	}
	return value
}

func resolveReferencedImage(page protocol.ConversationPage, reference protocol.ConversationImagesReference) *protocol.ConversationImage {
	for entry := len(page.Entries) - 1; entry >= 0; entry-- {
		for part := len(page.Entries[entry].Parts) - 1; part >= 0; part-- {
			value := page.Entries[entry].Parts[part]
			if value.Kind != "tool_result" {
				continue
			}
			result, err := payload[protocol.ConversationPartToolResultPayload](value.Data)
			if err != nil || result.ID == nil || *result.ID != reference.Call {
				continue
			}
			for _, image := range result.Images {
				if image.Index == reference.Index {
					return &image
				}
			}
		}
	}
	return nil
}
