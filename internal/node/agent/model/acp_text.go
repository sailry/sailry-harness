package model

import (
	"bytes"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// An agent reached over the Agent Client Protocol has no function-call
// channel a client can fill, so the tools travel as text: the system text
// lists them and the reply names a call between these tags.
const (
	acpCallOpen  = "<tool_call"
	acpCallClose = "</tool_call>"
)

const acpDiscipline = "You are used as a plain language model. Never call built-in tools, never read or change files and never run commands."

const acpToolProtocol = `To call a tool from <available_tools>, write a block of this exact form, with valid JSON and no code fence:
<tool_call>
{"name":"tool_name","arguments":{}}
</tool_call>
For several calls, write the blocks one after another. After the last block stop writing and wait: each result arrives as a <tool_response> block. Never write a <tool_response> block yourself.`

// acpSystem is the system text of one session: the discipline, the tool
// convention with the tool list, then the caller's system text.
func acpSystem(request protocol.PluginModelAdapterWireRequest) (string, error) {
	blocks := []string{acpDiscipline}
	if len(request.Tools) > 0 {
		type tool struct {
			Name        string          `json:"name"`
			Description *string         `json:"description,omitempty"`
			Parameters  json.RawMessage `json:"parameters"`
		}
		tools := make([]tool, 0, len(request.Tools))
		for _, declared := range request.Tools {
			tools = append(tools, tool{declared.Name, declared.Description, declared.Parameters})
		}
		list, err := json.Marshal(tools)
		if err != nil {
			return "", &Failure{Kind: InvalidRequest, Message: "model tool schema is invalid"}
		}
		blocks = append(blocks, acpToolProtocol, "<available_tools>\n"+string(list)+"\n</available_tools>")
	}
	if request.System != nil && *request.System != "" {
		blocks = append(blocks, *request.System)
	}
	return strings.Join(blocks, "\n\n"), nil
}

type acpImage struct{ MediaType, Data string }

// acpMessage renders one message of the history as text. Images it carries
// are appended to images and named in the text by their number.
func acpMessage(message protocol.PluginModelAdapterWireMessage, images *[]acpImage) (role, text string, err error) {
	var lines []string
	image := func(mediaType, data string) {
		*images = append(*images, acpImage{mediaType, data})
		lines = append(lines, fmt.Sprintf("[image %d]", len(*images)))
	}
	parts := func(parts []protocol.PluginModelAdapterWirePart) error {
		for _, part := range parts {
			switch value := part.Data.(type) {
			case *protocol.PluginModelAdapterWirePartTextPayload:
				lines = append(lines, value.Text)
			case *protocol.PluginModelAdapterWirePartToolCallPayload:
				call, err := json.Marshal(map[string]any{"name": value.Name, "arguments": value.Arguments})
				if err != nil {
					return &Failure{Kind: InvalidRequest, Message: "model history has an invalid tool call"}
				}
				lines = append(lines, "<tool_call>\n"+string(call)+"\n</tool_call>")
			case *protocol.PluginModelAdapterWirePartImagePayload:
				inline, ok := value.Data.Data.(*protocol.PluginModelAdapterWireImageInlinePayload)
				if !ok {
					return &Failure{Kind: Unsupported, Message: "the agent accepts only inline images"}
				}
				image(value.MediaType, inline.DataBase64)
			case *protocol.PluginModelAdapterWirePartFilePayload:
				return &Failure{Kind: Unsupported, Message: "the agent does not accept files"}
			}
		}
		return nil
	}
	switch value := message.Data.(type) {
	case *protocol.PluginModelAdapterWireMessageUserPayload:
		role, err = "USER", parts(value.Parts)
	case *protocol.PluginModelAdapterWireMessageAssistantPayload:
		role, err = "ASSISTANT", parts(value.Parts)
	case *protocol.PluginModelAdapterWireMessageToolPayload:
		role = "TOOL"
		for _, part := range value.Parts {
			result, ok := part.Data.(*protocol.PluginModelAdapterWirePartJsonPayload)
			if !ok {
				continue
			}
			// A tool result is an MCP envelope or any other JSON value.
			var envelope struct {
				Content []struct {
					Type, Text, Data, MimeType string
				} `json:"content"`
			}
			if json.Unmarshal(result.Value, &envelope) == nil && len(envelope.Content) > 0 {
				for _, content := range envelope.Content {
					if content.Type == "image" {
						image(content.MimeType, content.Data)
					} else if content.Type == "text" {
						lines = append(lines, content.Text)
					}
				}
				continue
			}
			var plain string
			if json.Unmarshal(result.Value, &plain) == nil {
				lines = append(lines, plain)
			} else {
				lines = append(lines, string(result.Value))
			}
		}
		attributes, _ := json.Marshal(value.CallID)
		name, _ := json.Marshal(value.Name)
		failed := ""
		if value.IsError {
			failed = ` error="true"`
		}
		return role, fmt.Sprintf("<tool_response id=%s name=%s%s>\n%s\n</tool_response>", attributes, name, failed, strings.Join(lines, "\n")), nil
	}
	return role, strings.Join(lines, "\n"), err
}

// acpPrompt renders messages as one prompt. A session that has seen the
// history before them receives them as they are; a new session receives the
// whole history as a transcript whose last user message stands alone.
func acpPrompt(messages []protocol.PluginModelAdapterWireMessage, transcript bool) (string, []acpImage, error) {
	var images []acpImage
	blocks := make([]string, 0, len(messages))
	for index, message := range messages {
		role, text, err := acpMessage(message, &images)
		if err != nil {
			return "", nil, err
		}
		if transcript && len(messages) > 1 && !(index == len(messages)-1 && role == "USER") {
			text = "[" + role + "]\n" + text
		}
		blocks = append(blocks, text)
	}
	return strings.Join(blocks, "\n\n"), images, nil
}

func acpDigest(value any) string {
	raw, _ := json.Marshal(value)
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:])
}

// acpReply identifies an assistant message by its text and call identifiers,
// the two things both the agent's reply and the stored history carry.
func acpReply(text string, calls []string) string {
	// A reply without calls is one value, whether its list is nil or empty.
	if calls == nil {
		calls = []string{}
	}
	return acpDigest([]any{strings.TrimSpace(text), calls})
}

func acpStoredReply(message protocol.PluginModelAdapterWireMessage) (string, bool) {
	assistant, ok := message.Data.(*protocol.PluginModelAdapterWireMessageAssistantPayload)
	if !ok {
		return "", false
	}
	var text strings.Builder
	calls := []string{}
	for _, part := range assistant.Parts {
		switch value := part.Data.(type) {
		case *protocol.PluginModelAdapterWirePartTextPayload:
			text.WriteString(value.Text)
		case *protocol.PluginModelAdapterWirePartToolCallPayload:
			calls = append(calls, value.ID)
		}
	}
	return acpReply(text.String(), calls), true
}

type acpToolCall struct {
	ID, Name, Arguments string
}

// acpParser splits a streamed reply into text and tool calls. Text that may
// still become an opening tag is held back; a call is complete at the first
// closing tag before which its body is valid JSON, so a closing tag inside a
// JSON string does not end it. A plain parser serves a request that offers no
// tools: its reply is text, whatever tags it quotes.
type acpParser struct {
	plain  bool
	buffer string
	inside bool
	called bool
	text   strings.Builder
	calls  []string
	// failed is the call this reply could not read; nothing after it
	// counts. attempt counts the calls read from the reply in progress, and
	// asked is the call a correction asked the agent to write again.
	failed  *acpMalformed
	attempt int
	asked   int
}

var errACPCall = &Failure{Kind: InvalidResponse, Message: "The model sent a malformed tool call"}

// acpMalformed is a call the parser could not read: its position in the
// reply, from one, and what the model is told to correct.
type acpMalformed struct {
	index  int
	reason string
}

func (malformed *acpMalformed) Error() string { return errACPCall.Message + ": " + malformed.reason }

// acpResends bounds the corrections one reply may take.
const acpResends = 2

// acpCorrection asks the agent, in the same session, to write again the
// call it got wrong and every call after it.
func acpCorrection(malformed *acpMalformed) string {
	return fmt.Sprintf("Tool call %d of your last reply could not be read (%s); it and everything after it were discarded. "+
		"Write tool call %d and every call after it again, each as a <tool_call> block with valid JSON: escape quotes, newlines and tabs inside strings. Write nothing else.",
		malformed.index, malformed.reason, malformed.index)
}

func (parser *acpParser) feed(chunk string) (text string, calls []acpToolCall, err error) {
	if parser.plain {
		parser.text.WriteString(chunk)
		return chunk, nil, nil
	}
	if parser.failed != nil {
		return "", nil, nil
	}
	parser.buffer += chunk
	var out strings.Builder
	for {
		if !parser.inside {
			open := strings.Index(parser.buffer, acpCallOpen)
			if open < 0 {
				// Keep a tail that is a prefix of the opening tag.
				keep := 0
				for size := min(len(parser.buffer), len(acpCallOpen)-1); size > 0; size-- {
					if strings.HasSuffix(parser.buffer, acpCallOpen[:size]) {
						keep = size
						break
					}
				}
				parser.emit(&out, parser.buffer[:len(parser.buffer)-keep])
				parser.buffer = parser.buffer[len(parser.buffer)-keep:]
				break
			}
			end := strings.IndexByte(parser.buffer[open:], '>')
			if end < 0 {
				parser.emit(&out, parser.buffer[:open])
				parser.buffer = parser.buffer[open:]
				break
			}
			parser.emit(&out, parser.buffer[:open])
			parser.buffer, parser.inside = parser.buffer[open+end+1:], true
		}
		call, rest, complete, err := acpCallBody(parser.buffer)
		var malformed *acpMalformed
		if errors.As(err, &malformed) {
			// The reply is read no further; a correction asks for this call
			// and those after it.
			malformed.index = parser.attempt + 1
			parser.failed, parser.called, parser.inside, parser.buffer = malformed, true, false, ""
			return out.String(), calls, nil
		}
		if err != nil {
			return out.String(), calls, err
		}
		if !complete {
			break
		}
		parser.buffer, parser.inside, parser.called = rest, false, true
		parser.calls = append(parser.calls, call.ID)
		parser.attempt++
		calls = append(calls, call)
	}
	return out.String(), calls, nil
}

// emit keeps the text before the first call. What a model writes after a
// call is not part of its answer: it was told to stop and wait.
func (parser *acpParser) emit(out *strings.Builder, text string) {
	if parser.called {
		return
	}
	parser.text.WriteString(text)
	out.WriteString(text)
}

// finish returns the text still held back. A call left open is an error.
func (parser *acpParser) finish() (string, error) {
	if parser.failed != nil {
		return "", errACPCall
	}
	if parser.inside {
		return "", &Failure{Kind: InvalidResponse, Message: "The model's reply ended inside a tool call"}
	}
	var out strings.Builder
	parser.emit(&out, parser.buffer)
	parser.buffer = ""
	return out.String(), nil
}

// pending reports the call a finished reply leaves unread: one that failed,
// one the reply ended inside, or, after a correction, none written again.
func (parser *acpParser) pending() *acpMalformed {
	switch {
	case parser.plain:
		return nil
	case parser.failed != nil:
		return parser.failed
	case parser.inside:
		return &acpMalformed{index: parser.attempt + 1, reason: acpUnread(parser.buffer)}
	case parser.asked > 0 && parser.attempt == 0:
		return &acpMalformed{index: parser.asked, reason: "the reply wrote no tool call"}
	}
	return nil
}

// resume reads the agent's answer to a correction: only its calls count,
// after those already received.
func (parser *acpParser) resume(malformed *acpMalformed) {
	parser.asked, parser.failed, parser.inside, parser.buffer, parser.attempt = malformed.index, nil, false, "", 0
	parser.called = true
}

type acpCallJSON struct {
	Name      string          `json:"name"`
	Arguments json.RawMessage `json:"arguments"`
}

func acpDecodeCall(body string) (acpCallJSON, error) {
	var parsed acpCallJSON
	decoder := json.NewDecoder(strings.NewReader(body))
	if err := decoder.Decode(&parsed); err != nil {
		return parsed, err
	}
	if decoder.More() {
		return parsed, errors.New("text follows the JSON object")
	}
	return parsed, nil
}

// acpRepair fixes the slips a model makes in otherwise whole JSON: control
// characters written raw inside strings and a comma before a closing
// bracket. It reports whether it changed anything; the caller uses the
// result only if it parses.
func acpRepair(body string) (string, bool) {
	var out strings.Builder
	inString, escaped, changed := false, false, false
	for index := 0; index < len(body); index++ {
		c := body[index]
		switch {
		case inString && escaped:
			escaped = false
		case inString && c == '\\':
			escaped = true
		case inString && c == '"':
			inString = false
		case inString && c < 0x20:
			switch c {
			case '\n':
				out.WriteString(`\n`)
			case '\r':
				out.WriteString(`\r`)
			case '\t':
				out.WriteString(`\t`)
			default:
				fmt.Fprintf(&out, `\u%04x`, c)
			}
			changed = true
			continue
		case !inString && c == '"':
			inString = true
		case !inString && c == ',':
			if rest := strings.TrimLeft(body[index+1:], " \t\r\n"); rest != "" && (rest[0] == '}' || rest[0] == ']') {
				changed = true
				continue
			}
		}
		out.WriteByte(c)
	}
	return out.String(), changed
}

// acpReadCall decodes the body of a call, without a code fence and repaired
// where that makes it parse.
func acpReadCall(body string) (acpCallJSON, error) {
	body = strings.TrimSpace(body)
	body = strings.TrimSpace(strings.TrimSuffix(strings.TrimPrefix(strings.TrimPrefix(body, "```json"), "```"), "```"))
	parsed, failure := acpDecodeCall(body)
	if failure != nil {
		if repaired, changed := acpRepair(body); changed {
			if fixed, again := acpDecodeCall(repaired); again == nil {
				return fixed, nil
			}
		}
	}
	return parsed, failure
}

// acpUnread says why a reply that ended inside a call left it unread: the
// JSON error of its body where a closing tag ended one, else that it ended.
func acpUnread(buffer string) string {
	if at := strings.LastIndex(buffer, acpCallClose); at >= 0 {
		if _, failure := acpReadCall(buffer[:at]); failure != nil {
			return failure.Error()
		}
	}
	return "the reply ended inside it"
}

func acpCallBody(buffer string) (call acpToolCall, rest string, complete bool, err error) {
	from, closed := 0, false
	for {
		at := strings.Index(buffer[from:], acpCallClose)
		if at < 0 {
			break
		}
		closed = true
		parsed, failure := acpReadCall(buffer[:from+at])
		after := from + at + len(acpCallClose)
		if failure == nil {
			if parsed.Name == "" {
				return call, "", false, &acpMalformed{reason: "it names no tool"}
			}
			arguments := bytes.TrimSpace(parsed.Arguments)
			if len(arguments) == 0 {
				arguments = []byte("{}")
			}
			identifier := make([]byte, 12)
			if _, err := rand.Read(identifier); err != nil {
				return call, "", false, err
			}
			return acpToolCall{ID: "call_" + hex.EncodeToString(identifier), Name: parsed.Name, Arguments: string(arguments)}, buffer[after:], true, nil
		}
		// A closing tag that another call follows closed a body that is not
		// JSON.
		if strings.HasPrefix(strings.TrimSpace(buffer[after:]), acpCallOpen) {
			return call, "", false, &acpMalformed{reason: failure.Error()}
		}
		from = after
	}
	// A body the reply goes on long after did not close as JSON.
	if closed && len(buffer)-from > 64*1024 {
		return call, "", false, &acpMalformed{reason: "its JSON did not end at a closing tag"}
	}
	return call, "", false, nil
}
