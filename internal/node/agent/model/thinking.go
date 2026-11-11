// Inline thinking filter derived from Goose 14741e9a. Apache-2.0; see LICENSE.goose.
package model

import "strings"

// thinkingTagLimit bounds a tag, complete or still arriving, that the filter
// holds back. A longer run is text.
const thinkingTagLimit = 8 * 1024

// thinkingPart is one run of a Chat Completions answer: visible text, or the
// reasoning a model wrote inline between <think> or <thinking> tags.
type thinkingPart struct {
	thinking bool
	text     string
}

// thinkingFilter separates inline reasoning from streamed text. A tag may
// arrive split across deltas, so an unfinished one is held until it resolves.
type thinkingFilter struct {
	buffer string
	depth  int
}

type thinkingTag int

const (
	thinkingOpen thinkingTag = iota
	thinkingClose
	thinkingEmpty
)

func (filter *thinkingFilter) push(text string) []thinkingPart {
	filter.buffer += text
	var parts []thinkingPart
	cursor, consumed := 0, 0
	for {
		offset := strings.IndexByte(filter.buffer[cursor:], '<')
		if offset < 0 {
			break
		}
		start := cursor + offset
		suffix := filter.buffer[start:]
		if tag, length, ok := parseThinkingTag(suffix); ok {
			// A closing tag outside reasoning is text.
			if length <= thinkingTagLimit && (filter.depth > 0 || tag != thinkingClose) {
				parts = filter.append(parts, filter.buffer[consumed:start])
				switch tag {
				case thinkingOpen:
					filter.depth++
				case thinkingClose:
					filter.depth--
				}
				consumed, cursor = start+length, start+length
				continue
			}
		} else if partialThinkingTag(suffix) && len(suffix) <= thinkingTagLimit {
			parts = filter.append(parts, filter.buffer[consumed:start])
			filter.buffer = filter.buffer[start:]
			return parts
		}
		cursor = start + 1
	}
	parts = filter.append(parts, filter.buffer[consumed:])
	filter.buffer = ""
	return parts
}

// finish returns what the stream left unresolved. Reasoning that never closed
// stays reasoning.
func (filter *thinkingFilter) finish() []thinkingPart {
	parts := filter.append(nil, filter.buffer)
	filter.buffer = ""
	return parts
}

func (filter *thinkingFilter) append(parts []thinkingPart, text string) []thinkingPart {
	if text == "" {
		return parts
	}
	thinking := filter.depth > 0
	if last := len(parts) - 1; last >= 0 && parts[last].thinking == thinking {
		parts[last].text += text
		return parts
	}
	return append(parts, thinkingPart{thinking: thinking, text: text})
}

func asciiLetter(value byte) bool {
	return value >= 'a' && value <= 'z' || value >= 'A' && value <= 'Z'
}

func asciiSpace(value byte) bool {
	return value == ' ' || value == '\t' || value == '\n' || value == '\r' || value == '\f'
}

// thinkingName reads the tag name after "<" or "</" and reports where it ends.
func thinkingName(text string) (name string, start, end int) {
	start = 1
	if strings.HasPrefix(text, "</") {
		start = 2
	}
	end = min(start, len(text))
	for end < len(text) && asciiLetter(text[end]) {
		end++
	}
	return strings.ToLower(text[min(start, len(text)):end]), start, end
}

// parseThinkingTag reads a complete thinking tag at the start of text.
func parseThinkingTag(text string) (thinkingTag, int, bool) {
	name, start, cursor := thinkingName(text)
	if name != "think" && name != "thinking" {
		return 0, 0, false
	}
	if start == 2 {
		for cursor < len(text) && asciiSpace(text[cursor]) {
			cursor++
		}
		return thinkingClose, cursor + 1, cursor < len(text) && text[cursor] == '>'
	}
	if cursor >= len(text) || text[cursor] != '>' && text[cursor] != '/' && !asciiSpace(text[cursor]) {
		return 0, 0, false
	}
	var quote, last byte
	for ; cursor < len(text); cursor++ {
		value := text[cursor]
		switch {
		case quote != 0:
			if value == quote {
				quote = 0
			}
		case value == '\'' || value == '"':
			quote, last = value, value
		case value == '>':
			if last == '/' {
				return thinkingEmpty, cursor + 1, true
			}
			return thinkingOpen, cursor + 1, true
		case !asciiSpace(value):
			last = value
		}
	}
	return 0, 0, false
}

// partialThinkingTag reports whether text, which ends the buffer, may still
// become a thinking tag.
func partialThinkingTag(text string) bool {
	name, start, end := thinkingName(text)
	if end == len(text) {
		return strings.HasPrefix("think", name) || strings.HasPrefix("thinking", name)
	}
	if name != "think" && name != "thinking" {
		return false
	}
	rest := text[end:]
	if start == 2 {
		return strings.TrimLeft(rest, " \t\n\r\f") == ""
	}
	if rest[0] != '/' && !asciiSpace(rest[0]) {
		return false
	}
	var quote byte
	for index := 0; index < len(rest); index++ {
		value := rest[index]
		switch {
		case quote != 0:
			if value == quote {
				quote = 0
			}
		case value == '\'' || value == '"':
			quote = value
		case value == '>':
			return false
		}
	}
	return true
}
