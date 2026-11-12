package model

import (
	"bytes"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
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
// JSON string does not end it.
type acpParser struct {
	buffer string
	inside bool
	called bool
	text   strings.Builder
	calls  []string
}

var errACPCall = &Failure{Kind: InvalidResponse, Message: "The model sent a malformed tool call"}

func (parser *acpParser) feed(chunk string) (text string, calls []acpToolCall, err error) {
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
		if err != nil {
			return out.String(), calls, err
		}
		if !complete {
			break
		}
		parser.buffer, parser.inside, parser.called = rest, false, true
		parser.calls = append(parser.calls, call.ID)
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
	if parser.inside {
		return "", &Failure{Kind: InvalidResponse, Message: "The model's reply ended inside a tool call"}
	}
	var out strings.Builder
	parser.emit(&out, parser.buffer)
	parser.buffer = ""
	return out.String(), nil
}

func acpCallBody(buffer string) (call acpToolCall, rest string, complete bool, err error) {
	from, closed := 0, false
	for {
		at := strings.Index(buffer[from:], acpCallClose)
		if at < 0 {
			break
		}
		closed = true
		body := strings.TrimSpace(buffer[:from+at])
		body = strings.TrimSpace(strings.TrimSuffix(strings.TrimPrefix(strings.TrimPrefix(body, "```json"), "```"), "```"))
		var parsed struct {
			Name      string          `json:"name"`
			Arguments json.RawMessage `json:"arguments"`
		}
		decoder := json.NewDecoder(strings.NewReader(body))
		if decoder.Decode(&parsed) == nil && !decoder.More() {
			if parsed.Name == "" {
				return call, "", false, errACPCall
			}
			arguments := bytes.TrimSpace(parsed.Arguments)
			if len(arguments) == 0 {
				arguments = []byte("{}")
			}
			identifier := make([]byte, 12)
			if _, err := rand.Read(identifier); err != nil {
				return call, "", false, err
			}
			return acpToolCall{ID: "call_" + hex.EncodeToString(identifier), Name: parsed.Name, Arguments: string(arguments)}, buffer[from+at+len(acpCallClose):], true, nil
		}
		from += at + len(acpCallClose)
	}
	// A closing tag after which another call opens, or after which the reply
	// goes on for long, closed a body that is not JSON.
	if closed && (strings.Contains(buffer[from:], acpCallOpen) || len(buffer)-from > 64*1024) {
		return call, "", false, errACPCall
	}
	return call, "", false, nil
}
