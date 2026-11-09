package observation

import (
	"cmp"
	"slices"
	"strconv"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type sourceRange struct {
	start, end int
	kind       protocol.ConversationObservationCategoryKind
}

// Context keeps exact byte ranges outside the adapted request text.
type Context struct {
	text      string
	ranges    []sourceRange
	persisted bool
}

func NewContext(parts []Source) *Context {
	if len(parts) == 0 {
		return nil
	}
	context := &Context{}
	context.push("<turn-context>\n", injected)
	for i, part := range parts {
		if i != 0 {
			context.push("\n\n", injected)
		}
		context.push(part.Text, part.Kind)
	}
	context.push("\n</turn-context>", injected)
	return context
}

func PersistedContext(text string, skills []string) Context {
	var ranges []sourceRange
	for _, source := range skills {
		for _, start := range occurrences(text, source) {
			ranges = append(ranges, sourceRange{start: start, end: start + len(source)})
		}
	}
	sortRanges(ranges)
	context := Context{persisted: true}
	end := 0
	for _, span := range ranges {
		if span.start < end {
			continue
		}
		context.push(text[end:span.start], injected)
		context.push(text[span.start:span.end], skillsKind)
		end = span.end
	}
	context.push(text[end:], injected)
	return context
}

func (context *Context) Text() string { return context.text }

func (context *Context) push(text string, kind protocol.ConversationObservationCategoryKind) {
	if text == "" {
		return
	}
	start := len(context.text)
	context.text += text
	if last := len(context.ranges) - 1; last >= 0 && context.ranges[last].kind == kind {
		context.ranges[last].end = len(context.text)
	} else {
		context.ranges = append(context.ranges, sourceRange{start: start, end: len(context.text), kind: kind})
	}
}

func (context *Context) contains(kind protocol.ConversationObservationCategoryKind) bool {
	for _, span := range context.ranges {
		if span.kind == kind {
			return true
		}
	}
	return false
}

type location struct {
	path    string
	start   int
	context *Context
}

func (context *Context) locate(body any) *location {
	object, _ := body.(map[string]any)
	var messages []any
	collection := ""
	for _, field := range messageFields {
		if items, ok := object[field].([]any); ok {
			if collection != "" {
				return nil
			}
			messages, collection = items, field
		}
	}
	if collection == "" {
		return nil
	}
	if context.persisted {
		var found *location
		for index, message := range messages {
			item, _ := message.(map[string]any)
			if item["role"] != "user" {
				continue
			}
			content, key, _ := messageContent(item)
			path := fieldPath(indexPath(fieldPath("$", collection), index), key)
			var texts []textNode
			switch content := content.(type) {
			case string:
				texts = append(texts, textNode{content, path})
			case []any:
				for i, part := range content {
					if text, ok := partText(part); ok {
						texts = append(texts, textNode{text, fieldPath(indexPath(path, i), "text")})
					}
				}
			}
			for _, text := range texts {
				for _, start := range occurrences(text.value, context.text) {
					end := start + len(context.text)
					if (start == 0 || text.value[start-1] == '\n') && (end == len(text.value) || text.value[end] == '\n') {
						if found != nil {
							return nil
						}
						found = &location{path: text.path, start: start, context: context}
					}
				}
			}
		}
		return found
	}
	if len(messages) == 0 {
		return nil
	}
	last, _ := messages[len(messages)-1].(map[string]any)
	if last["role"] != "user" {
		return nil
	}
	content, key, ok := messageContent(last)
	if !ok {
		return nil
	}
	path := fieldPath(indexPath(fieldPath("$", collection), len(messages)-1), key)
	var text string
	switch content := content.(type) {
	case string:
		text = content
	case []any:
		if len(content) == 0 {
			return nil
		}
		text, ok = partText(content[len(content)-1])
		if !ok {
			return nil
		}
		path = fieldPath(indexPath(path, len(content)-1), "text")
	default:
		return nil
	}
	prefix, ok := strings.CutSuffix(text, context.text)
	if !ok || prefix != "" && !strings.HasSuffix(prefix, "\n") {
		return nil
	}
	return &location{path: path, start: len(prefix), context: context}
}

type textNode struct{ value, path string }

func partText(value any) (string, bool) {
	part, _ := value.(map[string]any)
	typeValue, exists := part["type"]
	if name, ok := typeValue.(string); exists && ok && name != "text" && name != "input_text" {
		return "", false
	}
	text, ok := part["text"].(string)
	return text, ok
}

func messageContent(message map[string]any) (any, string, bool) {
	if value, ok := message["content"]; ok {
		return value, "content", true
	}
	value, ok := message["parts"]
	return value, "parts", ok
}

func occurrences(text, source string) []int {
	if source == "" {
		return nil
	}
	var starts []int
	for offset := 0; offset <= len(text)-len(source); {
		index := strings.Index(text[offset:], source)
		if index < 0 {
			break
		}
		starts = append(starts, offset+index)
		offset += index + len(source)
	}
	return starts
}

func sortRanges(ranges []sourceRange) {
	slices.SortStableFunc(ranges, func(a, b sourceRange) int {
		if order := cmp.Compare(a.start, b.start); order != 0 {
			return order
		}
		return cmp.Compare(b.end, a.end)
	})
}

// Paths identify decoded nodes even when separate user nodes contain equal text.
func fieldPath(path, key string) string       { return path + "[" + strconv.Quote(key) + "]" }
func indexPath(path string, index int) string { return path + "[" + strconv.Itoa(index) + "]" }
