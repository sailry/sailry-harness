package client

import (
	"cmp"
	"net/url"
	"slices"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type transcriptInput struct {
	Turn        protocol.TurnID            `json:"turn"`
	Message     protocol.ConversationInput `json:"message"`
	Attachments []protocol.Attachment      `json:"attachments"`
	Canonical   bool                       `json:"canonical"`
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
	Inputs  []transcriptInput `json:"inputs"`
	Rows    []transcriptRow   `json:"rows"`
	Waiting *int              `json:"waiting,omitempty"`
}

func emptySegment() transcriptSegment {
	return transcriptSegment{Inputs: []transcriptInput{}, Rows: []transcriptRow{}}
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

func projectTranscript(snapshot protocol.ConversationSnapshot, calls []projectedToolCall, guidance map[string]string) ([]protocol.TurnID, map[string]protocol.TurnID, map[string]any) {
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
		segments[run.Turn.String()] = transcriptSegments(snapshot, calls, guidance, run.Turn)
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

func turnInputs(page protocol.ConversationPage, run protocol.ConversationRun) []transcriptInput {
	value := []transcriptInput{}
	if run.Notice {
		return value
	}
	for _, entry := range page.Entries {
		if entry.Turn != run.Turn || entry.Author != "user" {
			continue
		}
		input := transcriptInput{Turn: run.Turn, Message: protocol.DefaultConversationInput(), Attachments: []protocol.Attachment{}, Canonical: true}
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
			case "reference":
				if reference, err := payload[protocol.ConversationReference](part.Data); err == nil {
					input.Message.References = append(input.Message.References, reference)
				}
			}
		}
		value = append(value, input)
	}
	if len(value) == 0 && run.Input != nil {
		value = append(value, transcriptInput{Turn: run.Turn, Message: run.Input.Message, Attachments: run.Input.Attachments})
	}
	return value
}

func transcriptSegments(snapshot protocol.ConversationSnapshot, calls []projectedToolCall, guidance map[string]string, turn protocol.TurnID) []transcriptSegment {
	page := snapshot.Page
	group := []protocol.ConversationRun{}
	sequences := map[protocol.TurnID][]uint64{}
	for _, entry := range page.Entries {
		if entry.Author == "user" {
			sequences[entry.Turn] = append(sequences[entry.Turn], entry.Sequence)
		}
	}
	for _, run := range page.Runs {
		if run.Turn == turn || run.Joined != nil && *run.Joined == turn {
			group = append(group, run)
		}
	}
	slices.SortFunc(group, func(left, right protocol.ConversationRun) int {
		if (left.Turn == turn) != (right.Turn == turn) {
			if left.Turn == turn {
				return -1
			}
			return 1
		}
		leftSequence, rightSequence := sequences[left.Turn], sequences[right.Turn]
		if (len(leftSequence) == 0) != (len(rightSequence) == 0) {
			if len(leftSequence) != 0 {
				return -1
			}
			return 1
		}
		if len(leftSequence) > 0 && len(rightSequence) > 0 {
			return cmp.Compare(leftSequence[0], rightSequence[0])
		}
		return cmp.Compare(left.Sequence, right.Sequence)
	})
	type positionedInput struct {
		position int
		sequence uint64
		input    transcriptInput
	}
	inputs := []positionedInput{}
	for _, run := range group {
		if run.Continuation != nil || run.Notice || guidance[run.Turn.String()] == "pending" {
			continue
		}
		for index, input := range turnInputs(page, run) {
			position := positionedInput{position: 2, input: input}
			if run.Turn == turn {
				position.position = 0
			} else if input.Canonical && index < len(sequences[run.Turn]) {
				position.position, position.sequence = 1, sequences[run.Turn][index]
			}
			inputs = append(inputs, position)
		}
	}
	slices.SortStableFunc(inputs, func(left, right positionedInput) int {
		if order := cmp.Compare(left.position, right.position); order != 0 {
			return order
		}
		return cmp.Compare(left.sequence, right.sequence)
	})
	rows := collectTranscriptRows(snapshot, calls, turn)
	segments := []transcriptSegment{}
	current := emptySegment()
	claimed := []transcriptInput{}
	cursor := 0
	for _, input := range inputs {
		switch input.position {
		case 0:
			current.Inputs = append(current.Inputs, input.input)
		case 2:
			claimed = append(claimed, input.input)
		case 1:
			for cursor < len(rows) && !rows[cursor].Draft && rows[cursor].sequence < input.sequence {
				current.Rows = append(current.Rows, rows[cursor])
				cursor++
			}
			if len(current.Rows) > 0 {
				segments = append(segments, current)
				current = emptySegment()
			}
			current.Inputs = append(current.Inputs, input.input)
		}
	}
	current.Rows = append(current.Rows, rows[cursor:]...)
	current.Inputs = append(current.Inputs, claimed...)
	current.Waiting = waitingChildren(snapshot, calls, guidance, turn, current.Rows)
	return append(segments, current)
}

func collectTranscriptRows(snapshot protocol.ConversationSnapshot, calls []projectedToolCall, turn protocol.TurnID) []transcriptRow {
	selected := map[toolReference]projectedToolCall{}
	for _, call := range calls {
		if call.Turn == turn {
			selected[call.Source] = call
		}
	}
	rows := []transcriptRow{}
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
				if value.Type == "model_retry" && len(rows) > 0 && rows[len(rows)-1].Part.Kind == "resource" {
					before := resourceValue(rows[len(rows)-1].Part.Data)
					if before.Type == "model_retry" && !before.Fallback && !value.Fallback && equalOptionalUint(before.Limit, value.Limit) &&
						equalOptionalString(before.Transport, value.Transport) && value.Attempt > before.Attempt {
						rows[len(rows)-1].Part = part
						continue
					}
				}
			}
			row.Work = transcriptWork(part, call)
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

func equalOptionalUint(left, right *uint64) bool {
	return left == nil && right == nil || left != nil && right != nil && *left == *right
}

func equalOptionalString(left, right *string) bool {
	return left == nil && right == nil || left != nil && right != nil && *left == *right
}

func transcriptWork(part protocol.ConversationPart, call projectedToolCall) bool {
	switch part.Kind {
	case "text", "image", "attachment", "reference":
		return false
	case "tool_call", "tool_result":
		return !terminalInteraction(call.Name)
	case "resource":
		value := resourceValue(part.Data)
		return value.Type != "tool_started" && value.Type != "provider_context"
	}
	return true
}

func waitingChildren(snapshot protocol.ConversationSnapshot, calls []projectedToolCall, guidance map[string]string, turn protocol.TurnID, rows []transcriptRow) *int {
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
	for start := 0; start < len(text); {
		colon := strings.IndexByte(text[start:], ':')
		if colon < 0 {
			break
		}
		colon += start
		start = colon + 1
		const scheme = "sailry-image"
		if colon < len(scheme) || !strings.EqualFold(text[colon-len(scheme):colon], scheme) {
			continue
		}
		end := len(text)
		for offset, char := range text[colon:] {
			if unicode.IsSpace(char) || strings.ContainsRune("()<>[]\"'", char) {
				end = colon + offset
				break
			}
		}
		destination := text[colon+1 : end]
		start = end
		slash := strings.LastIndexByte(destination, '/')
		if slash < 0 {
			continue
		}
		call, err := url.PathUnescape(destination[:slash])
		index := destination[slash+1:]
		sequence, sequenceErr := strconv.ParseUint(index, 10, 64)
		if err != nil || call == "" || len(call) > 512 || !utf8.ValidString(call) || sequenceErr != nil || strconv.FormatUint(sequence, 10) != index {
			continue
		}
		value = append(value, protocol.ConversationImagesReference{Call: call, Index: sequence})
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
