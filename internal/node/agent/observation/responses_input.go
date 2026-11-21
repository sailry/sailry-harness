package observation

import "github.com/sailry/sailry-harness/internal/protocol"

type responsesKind uint8

const (
	responseUser responsesKind = iota
	responseAssistant
	responseToolCall
	responseToolOutput
	responseDeveloperContext
	responseReasoning
	responseOther
)

var responseInputKinds = [...]protocol.ConversationResponsesInputKind{
	responseUser:             protocol.ConversationResponsesInputUser,
	responseAssistant:        protocol.ConversationResponsesInputAssistant,
	responseToolCall:         protocol.ConversationResponsesInputToolCall,
	responseToolOutput:       protocol.ConversationResponsesInputToolOutput,
	responseDeveloperContext: protocol.ConversationResponsesInputDeveloperContext,
	responseReasoning:        protocol.ConversationResponsesInputReasoning,
	responseOther:            protocol.ConversationResponsesInputOther,
}

type responsesItem struct {
	digest [32]byte
	size   uint32
	kind   responsesKind
}

func responseItemKind(item map[string]any) responsesKind {
	switch item["type"] {
	case "function_call", "custom_tool_call", "computer_call", "local_shell_call", "shell_call", "apply_patch_call",
		"file_search_call", "web_search_call", "code_interpreter_call", "image_generation_call", "mcp_call", "mcp_approval_request":
		return responseToolCall
	case "function_call_output", "custom_tool_call_output", "computer_call_output", "local_shell_call_output", "shell_call_output", "apply_patch_call_output", "mcp_approval_response":
		return responseToolOutput
	case "reasoning":
		return responseReasoning
	case nil, "message":
		switch item["role"] {
		case "user":
			return responseUser
		case "assistant":
			return responseAssistant
		case "developer", "system":
			return responseDeveloperContext
		}
	}
	return responseOther
}

func addResponseSize(size *protocol.ConversationResponsesInputSize, item responsesItem, count int) {
	size.Items += uint64(count)
	size.Bytes += uint64(item.size) * uint64(count)
}

func responseInputChanges(previous, current []responsesItem) ([]protocol.ConversationResponsesInputChange, bool) {
	var totals [len(responseInputKinds)]protocol.ConversationResponsesInputChange
	incomplete := false
	switch {
	case len(previous) == 0:
		for _, item := range current {
			addResponseSize(&totals[item.kind].Added, item, 1)
		}
	case len(current) == 0:
		for _, item := range previous {
			addResponseSize(&totals[item.kind].Removed, item, 1)
		}
	default:
		incomplete = responseTailChanges(previous, current, &totals)
	}
	var changes []protocol.ConversationResponsesInputChange
	for index, total := range totals {
		if total.Added.Items != 0 || total.Removed.Items != 0 || total.Reordered.Items != 0 {
			total.Kind = responseInputKinds[index]
			changes = append(changes, total)
		}
	}
	return changes, incomplete
}

func responseTailChanges(previous, current []responsesItem, totals *[len(responseInputKinds)]protocol.ConversationResponsesInputChange) bool {
	type occurrences struct {
		previous, current int
		first             int
	}
	counts := make(map[responsesItem]occurrences, len(previous))
	// Link occurrences of each previous item in order. Indices in the links are
	// one-based so the zero value marks the end of the list.
	next := make([]int, len(previous))
	for i := len(previous) - 1; i >= 0; i-- {
		item := previous[i]
		entry := counts[item]
		entry.previous++
		next[i], entry.first = entry.first, i+1
		counts[item] = entry
	}
	for _, item := range current {
		entry := counts[item]
		entry.current++
		counts[item] = entry
	}
	incomplete := false
	for item, entry := range counts {
		if entry.current > entry.previous {
			addResponseSize(&totals[item.kind].Added, item, entry.current-entry.previous)
		} else if entry.previous > entry.current {
			addResponseSize(&totals[item.kind].Removed, item, entry.previous-entry.current)
		}
		// Pair equally numerous duplicates in occurrence order. With unequal
		// nonzero counts, assigning the extra occurrence can invent a reorder.
		incomplete = incomplete || entry.previous != 0 && entry.current != 0 && entry.previous != entry.current
	}
	var matched []int
	for _, item := range current {
		entry := counts[item]
		if entry.previous == entry.current {
			position := entry.first - 1
			matched = append(matched, position)
			entry.first = next[position]
			counts[item] = entry
		}
	}
	// A retained item changes relative order if it has a larger predecessor
	// or a smaller successor in the sequence of previous positions.
	reordered := make([]bool, len(matched))
	smallest := len(previous)
	for i := len(matched) - 1; i >= 0; i-- {
		reordered[i] = matched[i] > smallest
		smallest = min(smallest, matched[i])
	}
	largest := -1
	for i, position := range matched {
		if reordered[i] || position < largest {
			item := previous[position]
			addResponseSize(&totals[item.kind].Reordered, item, 1)
		}
		largest = max(largest, position)
	}
	return incomplete
}
