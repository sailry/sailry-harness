package claudecli

import (
	"encoding/json"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/model"
)

// Refusal texts Claude Code prints. The third-party wordings are the two
// magpie records for personal and workspace plans.
const (
	refusedPersonal  = "Third-party apps now draw from your extra usage"
	refusedWorkspace = "Third-party apps now draw from extra usage"
	synthetic        = "<synthetic>"
)

// eventKind names what one stdout line means to a step.
type eventKind int

const (
	eventIgnored eventKind = iota
	eventStarted
	eventMessage
	eventText
	eventReasoning
	eventReasoningEnd
	eventCall
	eventUsage
	eventMessageEnd
	eventLimit
	eventResult
)

// event is one decoded stdout line.
type event struct {
	kind eventKind
	// index is the content block's position inside its message.
	index int
	// message identifies the API message the event belongs to.
	message string
	text    string
	// call fields: Claude's identity, the offered name and the arguments.
	identity  string
	name      string
	arguments json.RawMessage
	usage     usage
	// stop is the API stop reason of a message.
	stop string
	// result fields.
	failed bool
	status int
	// limit fields.
	rejected bool
	resets   time.Time
}

// usage keeps field presence: an absent count stays unknown.
type usage struct {
	input, output, cacheRead, cacheWrite *int64
}

type wireUsage struct {
	Input      *int64 `json:"input_tokens"`
	Output     *int64 `json:"output_tokens"`
	CacheRead  *int64 `json:"cache_read_input_tokens"`
	CacheWrite *int64 `json:"cache_creation_input_tokens"`
}

func (value wireUsage) usage() usage {
	return usage{input: value.Input, output: value.Output, cacheRead: value.CacheRead, cacheWrite: value.CacheWrite}
}

// merge keeps the latest reported value of each count.
func (value *usage) merge(next usage) {
	for _, field := range []struct{ to, from **int64 }{{&value.input, &next.input}, {&value.output, &next.output}, {&value.cacheRead, &next.cacheRead}, {&value.cacheWrite, &next.cacheWrite}} {
		if *field.from != nil {
			*field.to = *field.from
		}
	}
}

type wireBlock struct {
	Type     string   `json:"type"`
	ID       string          `json:"id"`
	Name     string          `json:"name"`
	Text     string   `json:"text"`
	Thinking string   `json:"thinking"`
	Input    json.RawMessage `json:"input"`
}

type wireLine struct {
	Type      string `json:"type"`
	Subtype   string `json:"subtype"`
	SessionID string `json:"session_id"`
	IsError   bool   `json:"is_error"`
	Result    string `json:"result"`
	Status    int    `json:"api_error_status"`
	Event     struct {
		Type    string `json:"type"`
		Index   int    `json:"index"`
		Message struct {
			ID    string    `json:"id"`
			Usage wireUsage `json:"usage"`
		} `json:"message"`
		ContentBlock wireBlock `json:"content_block"`
		Delta struct {
			Type string `json:"type"`
			Text string `json:"text"`
			Thinking    string `json:"thinking"`
			PartialJSON string `json:"partial_json"`
			StopReason  string `json:"stop_reason"`
		} `json:"delta"`
		Usage wireUsage `json:"usage"`
	} `json:"event"`
	Message struct {
		ID  string      `json:"id"`
		Model      string      `json:"model"`
		StopReason string      `json:"stop_reason"`
		Usage      wireUsage   `json:"usage"`
		Content    []wireBlock `json:"content"`
	} `json:"message"`
	RateLimitInfo struct {
		Status   string `json:"status"`
		ResetsAt int64  `json:"resetsAt"`
	} `json:"rate_limit_info"`
}

type openBlock struct {
	kind      string
	identity  string
	name      string
	arguments strings.Builder
}

// decoder turns stream-json stdout lines into step events. One decoder
// follows one child for its whole life.
type decoder struct {
	message  string
	streamed map[string]bool
	blocks   map[int]*openBlock
}

func newDecoder() *decoder {
	return &decoder{streamed: map[string]bool{}, blocks: map[int]*openBlockn
// line decodes one stdout line. A line this build does not know is ignored:
// Claude Code adds line types between versions.
func (decoder *decoder) line(raw []byte) []event {
	var line wireLine
	if json.Unmarshal(raw, &line) != nil {
		return nil
	}
	switch line.Type {
	case "system":
		if line.Subtype == "init" {
			return []event{{kind: eventStarted, text: line.SessionID}}
		}
	case "stream_event":
		return decoder.stream(line)
	case "assistant":
		return decoder.whole(line)
	case "rate_limit_event":
		limit := event{kind: eventLimit, rejected: line.RateLimitInfo.Status == "rejected"}
		if line.RateLimitInfo.ResetsAt > 0 {
			limit.resets = time.Unix(line.RateLimitInfo.ResetsAt, 0)
		}
		return []event{limit}
	case "result":
		return []event{{kind: eventResult, failed: line.IsError, text: line.Result, status: line.Status}}
	}
	return nil
}

func (decoder *decoder) stream(line wireLine) []event {
	inner := line.Event
	switch inner.Type {
	case "message_start":
		decoder.message = inner.Message.ID
		decoder.streamed[decoder.message] = true
		decoder.blocks = map[int]*openBlock{}
		return []event{{kind: eventMessage, message: decoder.message}, {kind: eventUsage, message: decoder.message, usage: inner.Message.Usage.usage()}}
	case "content_block_start":
		block := &openBlock{kind: inner.ContentBlock.Type, identity: inner.ContentBlock.ID, name: inner.ContentBlock.Name}
		decoder.blocks[inner.Index] = block
		if block.kind == "text" && inner.ContentBlock.Text != "" {
			return []event{{kind: eventText, index: inner.Index, message: decoder.message, text: inner.ContentBlock.Text}}
		}
	case "content_block_delta":
		switch inner.Delta.Type {
		case "text_delta":
			return []event{{kind: eventText, index: inner.Index, message: decoder.message, text: inner.Delta.Text}}
		case "thinking_delta":
			return []event{{kind: eventReasoning, index: inner.Index, message: decoder.message, text: inner.Delta.Thinking}}
		case "input_json_delta":
			if block := decoder.blocks[inner.Index]; block != nil {
				block.arguments.WriteString(inner.Delta.PartialJSON)
			}
		}
	case "content_block_stop":
		block := decoder.blocks[inner.Index]
		delete(decoder.blocks, inner.Index)
		if block == nil {
			return nil
		}
		switch block.kind {
		case "thinking":
			return []event{{kind: eventReasoningEnd, index: inner.Index, message: decoder.message}}
		case "tool_use":
			arguments := block.arguments.String()
			if arguments == "" {
				arguments = "{}"
			}
			return []event{{kind: eventCall, index: inner.Index, message: decoder.message, identity: block.identity, name: block.name, arguments: json.RawMessage(arguments)}}
		}
	case "message_delta":
		return []event{{kind: eventUsage, message: decoder.message, usage: inner.Usage.usage(), stop: inner.Delta.StopReason}}
	case "message_stop":
		return []event{{kind: eventMessageEnd, message: decoder.message}}
	}
	return nil
}

// whole decodes a complete assistant line. A message that was streamed has
// been told already; one that was not (Claude Code asks again without a
// stream after a cut attempt) is told here, a block per line.
func (decoder *decoder) whole(line wireLine) []event {
	message := line.Message
	if message.Model == synthetic || message.ID == "" || decoder.streamed[message.ID] {
		return nil
	}
	events := []event{}
	if decoder.message != message.ID {
		decoder.message = message.ID
		events = append(events, event{kind: eventMessage, message: message.ID}, event{kind: eventUsage, message: message.ID, usage: message.Usage.usage()})
	}
	for index, block := range message.Content {
		switch block.Type {
		case "text":
			events = append(events, event{kind: eventText, index: index, message: message.ID, text: block.Text})
		case "thinking":
			events = append(events, event{kind: eventReasoning, index: index, message: message.ID, text: block.Thinking}, event{kind: eventReasoningEnd, index: index, message: message.ID})
		case "tool_use":
			arguments := block.Input
			if len(arguments) == 0 {
				arguments = json.RawMessage("{}")
			}
			events = append(events, event{kind: eventCall, index: index, message: message.ID, identity: block.ID, name: block.Name, arguments: arguments})
		}
	}
	if message.StopReason != "" {
		events = append(events, event{kind: eventUsage, message: message.ID, usage: message.Usage.usage(), stop: message.StopReason}, event{kind: eventMessageEnd, message: message.ID})
	}
	return events
}

// userLine is one stdin user message.
func userLine(content []map[string]any) ([]byte, error) {
	raw, err := json.Marshal(map[string]any{"type": "user", "message": map[string]any{"role": "user", "content": content}})
	if err != nil {
		return nil, err
	}
	return append(raw, '\n'), nil
}

// failure classifies the text Claude Code ends a failed turn or run with.
// The text is Claude Code's or Anthropic's own and names no credential.
func failure(text string, status int) *model.Failure {
	text = strings.TrimSpace(text)
	lower := strings.ToLower(text)
	switch {
	case strings.Contains(text, refusedPersonal) || strings.Contains(text, refusedWorkspace):
		return &model.Failure{Kind: model.Quota, Message: text}
	case strings.Contains(lower, "not logged in") || strings.Contains(lower, "/login") || status == 401:
		return &model.Failure{Kind: model.Authentication, Message: "Claude Code is not signed in"}
	case strings.Contains(lower, "prompt is too long"):
		return &model.Failure{Kind: model.ContextOverflow, Message: text}
	case status == 429:
		return &model.Failure{Kind: model.RateLimited, Message: text}
	case status >= 500:
		return &model.Failure{Kind: model.Unavailable, Message: text}
	case status >= 400:
		return &model.Failure{Kind: model.InvalidRequest, Message: text}
	}
	if text == "" {
		text = "Claude Code ended the turn with an error"
	}
	return &model.Failure{Kind: model.Other, Message: text}
}
