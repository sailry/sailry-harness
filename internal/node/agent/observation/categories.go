package observation

import (
	"cmp"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	systemKind    = protocol.ConversationObservationCategoryKindSystem
	toolsKind     = protocol.ConversationObservationCategoryKindTools
	userKind      = protocol.ConversationObservationCategoryKindUser
	skillsKind    = protocol.ConversationObservationCategoryKindSkills
	injected      = protocol.ConversationObservationCategoryKindInjected
	assistantKind = protocol.ConversationObservationCategoryKindAssistant
	resultsKind   = protocol.ConversationObservationCategoryKindResults
)

var (
	systemFields  = [...]string{"instructions", "system", "systemInstruction", "system_instruction"}
	messageFields = [...]string{"messages", "contents", "input"}
	kinds         = [...]protocol.ConversationObservationCategoryKind{systemKind, toolsKind, userKind, skillsKind, injected, assistantKind, resultsKind}
)

type count struct{ items, characters uint64 }

type measurement struct {
	counts    map[protocol.ConversationObservationCategoryKind]count
	found     map[int]bool
	locations []location
}

func estimate(body any, sources []Source, inventory bool, contexts []Context) []protocol.ConversationObservationCategory {
	measured := measurement{counts: map[protocol.ConversationObservationCategoryKind]count{}, found: map[int]bool{}}
	for i := range contexts {
		if located := contexts[i].locate(body); located != nil {
			measured.locations = append(measured.locations, *located)
		}
	}
	object, _ := body.(map[string]any)
	recognized, supported := false, true
	for _, field := range systemFields {
		if value, ok := object[field]; ok {
			measured.add(systemKind, value, fieldPath("$", field), sources)
			switch value.(type) {
			case string, []any, map[string]any:
				recognized = true
			}
		}
	}
	tools, toolsPresent := object["tools"]
	declarations, toolsPath, declared := toolDeclarations(tools)
	for index, tool := range declarations {
		measured.add(toolsKind, tool, indexPath(toolsPath, index), sources)
	}
	for _, field := range messageFields {
		value, present := object[field]
		path := fieldPath("$", field)
		switch value := value.(type) {
		case []any:
			recognized = true
			for index, message := range value {
				item, _ := message.(map[string]any)
				kind := messageKind(item)
				content, key, exists := messageContent(item)
				messagePath := indexPath(path, index)
				if parts, ok := content.([]any); ok {
					for i, part := range parts {
						partKind := kind
						partObject, _ := part.(map[string]any)
						_, response := partObject["functionResponse"]
						if partObject["type"] == "tool_result" || response {
							partKind = resultsKind
						}
						measured.add(partKind, part, indexPath(fieldPath(messagePath, key), i), sources)
					}
				} else if exists {
					measured.add(kind, content, fieldPath(messagePath, key), sources)
				} else {
					measured.add(kind, message, messagePath, sources)
				}
				if kind == assistantKind && exists {
					for _, field := range []string{"tool_calls", "function_call", "reasoning_content"} {
						if sibling, ok := item[field]; ok {
							measured.add(kind, sibling, fieldPath(messagePath, field), sources)
						}
					}
				}
			}
		case string:
			measured.add(userKind, value, path, sources)
			if field == "input" {
				recognized = true
			} else {
				supported = false
			}
		default:
			if present {
				supported = false
			}
		}
	}
	recognized = recognized && supported && (!toolsPresent || tools == nil || declared)
	rows := make([]protocol.ConversationObservationCategory, 0, len(kinds))
	for _, kind := range kinds {
		identified := kind == skillsKind || kind == injected
		known := recognized
		if identified {
			known = known && inventory
			for index, source := range sources {
				if source.Kind == kind && source.Text != "" && !measured.found[index] {
					known = false
				}
			}
			for i := range contexts {
				if !contexts[i].contains(kind) {
					continue
				}
				located := false
				for _, location := range measured.locations {
					located = located || location.context == &contexts[i]
				}
				known = known && located
			}
		}
		row := protocol.ConversationObservationCategory{Kind: kind}
		if known {
			value := measured.counts[kind]
			tokens := (value.characters + 3) / 4
			method := "text_characters_quarter"
			if identified {
				method = "identified_source_characters_quarter"
			}
			row.Items, row.Tokens, row.Method = &value.items, &tokens, &method
		}
		rows = append(rows, row)
	}
	return rows
}

func toolDeclarations(value any) ([]any, string, bool) {
	path := fieldPath("$", "tools")
	if tools, ok := value.([]any); ok {
		return tools, path, true
	}
	object, _ := value.(map[string]any)
	key := "functionDeclarations"
	declarations, exists := object[key]
	if !exists {
		key = "function_declarations"
		declarations = object[key]
	}
	tools, ok := declarations.([]any)
	return tools, fieldPath(path, key), ok
}

func messageKind(message map[string]any) protocol.ConversationObservationCategoryKind {
	switch message["role"] {
	case "system", "developer":
		return systemKind
	case "assistant", "model":
		return assistantKind
	case "tool", "function":
		return resultsKind
	}
	switch message["type"] {
	case "function_call_output", "computer_call_output":
		return resultsKind
	case "function_call", "reasoning", "computer_call":
		return assistantKind
	}
	return userKind
}

func (measured *measurement) increment(kind protocol.ConversationObservationCategoryKind, items, characters uint64) {
	value := measured.counts[kind]
	value.items += items
	value.characters += characters
	measured.counts[kind] = value
}

func (measured *measurement) add(kind protocol.ConversationObservationCategoryKind, value any, path string, sources []Source) {
	measured.increment(kind, 1, 0)
	if kind != systemKind {
		sources = nil
	}
	measured.measure(kind, value, path, sources)
}

func (measured *measurement) measure(kind protocol.ConversationObservationCategoryKind, value any, path string, sources []Source) {
	switch value := value.(type) {
	case string:
		if strings.HasPrefix(value, "data:") {
			return
		}
		if kind == userKind {
			var matched []location
			for _, location := range measured.locations {
				if location.path == path {
					matched = append(matched, location)
				}
			}
			slices.SortStableFunc(matched, func(a, b location) int { return cmp.Compare(a.start, b.start) })
			if len(matched) > 0 {
				end := 0
				for _, location := range matched {
					if location.start < end {
						continue
					}
					measured.increment(kind, 0, uint64(utf8.RuneCountInString(value[end:location.start])))
					for _, span := range location.context.ranges {
						measured.increment(span.kind, 1, uint64(utf8.RuneCountInString(location.context.text[span.start:span.end])))
					}
					end = location.start + len(location.context.text)
				}
				measured.increment(kind, 0, uint64(utf8.RuneCountInString(value[end:])))
				return
			}
		}
		type attributedRange struct {
			sourceRange
			index int
		}
		var ranges []attributedRange
		for index, source := range sources {
			for _, start := range occurrences(value, source.Text) {
				ranges = append(ranges, attributedRange{sourceRange{start, start + len(source.Text), source.Kind}, index})
			}
		}
		slices.SortStableFunc(ranges, func(a, b attributedRange) int {
			if order := cmp.Compare(a.start, b.start); order != 0 {
				return order
			}
			return cmp.Compare(b.end, a.end)
		})
		end, attributed := 0, uint64(0)
		for _, span := range ranges {
			if span.start < end {
				continue
			}
			end = span.end
			measured.found[span.index] = true
			characters := uint64(utf8.RuneCountInString(value[span.start:span.end]))
			attributed += characters
			measured.increment(span.kind, 1, characters)
		}
		measured.increment(kind, 0, uint64(utf8.RuneCountInString(value))-attributed)
	case []any:
		for index, value := range value {
			measured.measure(kind, value, indexPath(path, index), sources)
		}
	case map[string]any:
		for key, value := range value {
			switch key {
			case "data", "image_url", "file_data", "signature", "thoughtSignature", "encrypted_content":
				continue
			}
			measured.measure(kind, value, fieldPath(path, key), sources)
		}
	}
}
