package claudecli

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"slices"
	"strings"

	"charm.land/fantasy"
	"github.com/MoeForge/moego/base64"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
)

// Settings are the facts fixed when a child starts.
type Settings struct {
	Model  string
	Effort string
	// Config is the --mcp-config value; empty offers no tool.
	Config string
	// System is the file holding Sailry's system prompt; empty appends none.
	System string
}

// childSettings is the --settings value: no URL-handler registration, and
// reasoning shown as summaries.
const childSettings = `{"disableDeepLinkRegistration":"disable","showThinkingSummaries":true}`

// shared are the arguments both modes take. docs/claude-cli.md records
// where each flag was verified.
func shared(settings Settings) []string {
	arguments := []string{
		"--model", settings.Model, "--tools", "", "--strict-mcp-config",
		"--setting-sources", "", "--settings", childSettings, "--disable-slash-commands",
		"--permission-mode", "dontAsk",
	}
	if settings.Config != "" {
		arguments = append(arguments, "--mcp-config", settings.Config, "--allowedTools", "mcp__"+ServerName)
	}
	if settings.System != "" {
		arguments = append(arguments, "--append-system-prompt-file", settings.System)
	}
	return arguments
}

// Arguments is the -p command line.
func Arguments(settings Settings) []string {
	arguments := append([]string{
		"-p", "--input-format", "stream-json", "--output-format", "stream-json",
		"--include-partial-messages", "--verbose", "--no-session-persistence",
	}, shared(settings)...)
	if settings.Effort != "" {
		arguments = append(arguments, "--effort", settings.Effort, "--thinking-display", "summarized")
	}
	return arguments
}

// Environment is what the route adds to a child's environment. Every entry
// is one a user could set for a headless run; none claims another
// entrypoint. The process layer inherits only the search path, the account's
// directories, locale and proxy settings, so a stray API key, endpoint,
// entrypoint mark or OAuth token of the Node's own environment never reaches
// the child and nothing has to be removed.
func Environment() map[string]string {
	return map[string]string{
		"ENABLE_CLAUDEAI_MCP_SERVERS":          "0",
		"DISABLE_AUTO_COMPACT":                 "1",
		"CLAUDE_CODE_DISABLE_AUTO_MEMORY":      "1",
		"CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS": "1",
		"CLAUDE_CODE_DISABLE_CLAUDE_MDS":       "1",
		"DISABLE_AUTOUPDATER":                  "1",
		"DISABLE_ERROR_REPORTING":              "1",
	}
}

// effortFlag maps the captured effort to a level Claude Code accepts.
func effortFlag(kind string) string {
	switch kind {
	case "low", "medium", "high", "xhigh", "max":
		return kind
	}
	return ""
}

const (
	roleUser      = "user"
	roleAssistant = "assistant"
	roleTool      = "tool"
)

// turnMessage is one prompt message reduced to what the route compares and
// renders.
type turnMessage struct {
	role   string
	digest string
	// calls are an assistant message's tool call identities.
	calls []string
	// results are a tool message's answers by call identity.
	results map[string]Result
	order   []string
	parts   []fantasy.MessagePart
}

// stepRequest is one inference step.
type stepRequest struct {
	key      string
	settings string
	model    string
	effort   string
	system   string
	messages []turnMessage
	tools    []Tool
	// names maps an offered tool name back to its declared one.
	names map[string]string
	// media reports a file among the conversation's own messages; the
	// terminal interface takes text only.
	media bool
}

func prepare(binding model.Binding, call fantasy.Call) (stepRequest, error) {
	request := stepRequest{model: binding.Config.Model, effort: effortFlag(binding.Config.Effort.Kind), names: map[string]string{}}
	for _, declared := range call.Tools {
		tool, ok := declared.(fantasy.FunctionTool)
		if !ok {
			return request, &model.Failure{Kind: model.InvalidRequest, Message: "model tool declaration is unsupported"}
		}
		schema, err := model.ToolSchema(call, tool)
		if err != nil {
			return request, &model.Failure{Kind: model.InvalidRequest, Message: "model tool schema is invalid"}
		}
		request.tools = append(request.tools, Tool{Name: tool.Name, Description: tool.Description, Schema: schema})
		request.names[ToolName(tool.Name)] = tool.Name
	}
	for _, message := range call.Prompt {
		if message.Role == fantasy.MessageRoleSystem {
			for _, part := range message.Content {
				if text, ok := fantasy.AsMessagePart[fantasy.TextPart](part); ok {
					request.system += text.Text
				}
			}
			continue
		}
		request.messages = append(request.messages, reduce(message))
		if message.Role != fantasy.MessageRoleTool {
			for _, part := range message.Content {
				if _, file := fantasy.AsMessagePart[fantasy.FilePart](part); file {
					request.media = true
				}
			}
		}
	}
	if len(request.messages) == 0 {
		return request, &model.Failure{Kind: model.InvalidRequest, Message: "model request has no message"}
	}
	sum := sha256.Sum256([]byte(request.model + "\x00" + request.effort + "\x00" + request.system))
	request.settings = hex.EncodeToString(sum[:])
	// A request without tools (a summary, a title) never shares a child.
	if len(request.tools) > 0 {
		request.key = binding.Session.String()
	}
	return request, nil
}

func reduce(message fantasy.Message) turnMessage {
	reduced := turnMessage{role: string(message.Role), parts: message.Content, results: make(map[string]Result)}
	hash := sha256.New()
	field := func(values ...string) {
		for _, value := range values {
			fmt.Fprintf(hash, "%d:%s", len(value), value)
		}
	}
	field(reduced.role)
	last := ""
	for _, part := range message.Content {
		if text, ok := fantasy.AsMessagePart[fantasy.TextPart](part); ok {
			field("text", text.Text)
		} else if file, ok := fantasy.AsMessagePart[fantasy.FilePart](part); ok {
			sum := sha256.Sum256(file.Data)
			field("file", file.MediaType, hex.EncodeToString(sum[:]))
			// Media after a result belongs to that result.
			if reduced.role == roleTool && last != "" {
				result := reduced.results[last]
				result.Media = append(result.Media, Media{Mime: file.MediaType, Data: file.Data})
				reduced.results[last] = result
			}
		} else if request, ok := fantasy.AsMessagePart[fantasy.ToolCallPart](part); ok {
			field("call", request.ToolCallID, request.ToolName, request.Input)
			reduced.calls = append(reduced.calls, request.ToolCallID)
		} else if response, ok := fantasy.AsMessagePart[fantasy.ToolResultPart](part); ok {
			result := Result{}
			switch output := response.Output.(type) {
			case fantasy.ToolResultOutputContentText:
				result.Text = output.Text
			case fantasy.ToolResultOutputContentError:
				result.Error = true
				if output.Error != nil {
					result.Text = output.Error.Error()
				}
			case fantasy.ToolResultOutputContentMedia:
				result.Text = output.Text
				if data, err := base64.StdEncoding.DecodeString(output.Data); err == nil {
					result.Media = append(result.Media, Media{Mime: output.MediaType, Data: data})
				}
			}
			field("result", response.ToolCallID, result.Text)
			reduced.results[response.ToolCallID] = result
			reduced.order = append(reduced.order, response.ToolCallID)
			last = response.ToolCallID
		}
		// Reasoning is not compared or replayed: its signatures stay in the child.
	}
	reduced.digest = hex.EncodeToString(hash.Sum(nil))
	return reduced
}

// chain digests a message prefix.
func chain(messages []turnMessage) string {
	hash := sha256.New()
	for _, message := range messages {
		hash.Write([]byte(message.digest))
	}
	return hex.EncodeToString(hash.Sum(nil))
}

const (
	replayOpen  = "The conversation so far follows. Earlier tool calls and their results are shown as text; make new calls with your tools.\n\n"
	replayClose = "\n\nContinue the conversation from its end."
)

// content collects stdin content blocks: text runs with media between them.
type content struct {
	blocks []map[string]any
	text   strings.Builder
}

func (value *content) flush() {
	if value.text.Len() > 0 {
		value.blocks = append(value.blocks, map[string]any{"type": "text", "text": value.text.String()})
		value.text.Reset()
	}
}

func (value *content) file(mime string, data []byte) {
	kind := ""
	switch {
	case strings.HasPrefix(mime, "image/"):
		kind = "image"
	case mime == "application/pdf":
		kind = "document"
	default:
		fmt.Fprintf(&value.text, "[attached file (%s) omitted: Claude Code takes images and PDF]", mime)
		return
	}
	value.flush()
	value.blocks = append(value.blocks, map[string]any{"type": kind, "source": map[string]any{"type": "base64", "media_type": mime, "data": base64.StdEncoding.EncodeToString(data)}})
}

// parts writes one message's parts. Calls and results are text: a new child
// cannot be handed another child's calls.
func (value *content) parts(message turnMessage) {
	for _, part := range message.parts {
		if text, ok := fantasy.AsMessagePart[fantasy.TextPart](part); ok {
			value.text.WriteString(text.Text)
		} else if file, ok := fantasy.AsMessagePart[fantasy.FilePart](part); ok {
			if message.role != roleTool {
				value.file(file.MediaType, file.Data)
			}
		} else if request, ok := fantasy.AsMessagePart[fantasy.ToolCallPart](part); ok {
			fmt.Fprintf(&value.text, "\n[tool call %s id=%s args=%s]", toolPrefix+ToolName(request.ToolName), request.ToolCallID, request.Input)
		}
	}
	for _, identity := range message.order {
		result := message.results[identity]
		mark := ""
		if result.Error {
			mark = " error"
		}
		fmt.Fprintf(&value.text, "[tool result id=%s%s]\n%s", identity, mark, result.Text)
		for _, media := range result.Media {
			value.file(media.Mime, media.Data)
		}
		fmt.Fprintf(&value.text, "\n[end of tool result id=%s]", identity)
	}
}

// replay renders a whole conversation as the first message of a new child.
func replay(messages []turnMessage) []map[string]any {
	value := new(content)
	if len(messages) == 1 && messages[0].role == roleUser {
		value.parts(messages[0])
		value.flush()
		return value.blocks
	}
	value.text.WriteString(replayOpen)
	for index, message := range messages {
		if index > 0 {
			value.text.WriteString("\n\n")
		}
		switch message.role {
		case roleAssistant:
			value.text.WriteString("Assistant: ")
		case roleUser:
			value.text.WriteString("Human: ")
		}
		value.parts(message)
	}
	value.text.WriteString(replayClose)
	value.flush()
	return value.blocks
}

// following renders the user messages a living child has not seen.
func following(messages []turnMessage) []map[string]any {
	value := &content{}
	for index, message := range messages {
		if index > 0 {
			value.text.WriteString("\n\n")
		}
		value.parts(message)
	}
	value.flush()
	return value.blocks
}

// guidance appends user input that joined with tool results to the last
// result, marked as the user's, so the child goes on without a new turn.
func guidance(result Result, messages []turnMessage) Result {
	for _, message := range messages {
		result.Text += "\n[end of tool result]\n\n[user message]\n"
		for _, part := range message.parts {
			if text, ok := fantasy.AsMessagePart[fantasy.TextPart](part); ok {
				result.Text += text.Text
			} else if file, ok := fantasy.AsMessagePart[fantasy.FilePart](part); ok && strings.HasPrefix(file.MediaType, "image/") {
				result.Media = append(result.Media, Media{Mime: file.MediaType, Data: file.Data})
			}
		}
	}
	return result
}

// continues reports whether a request extends what the child has taken: the
// same prefix, the reply's own message, then exactly the parked results or
// new user messages.
func continues(taken int, prefix string, pending []string, messages []turnMessage) bool {
	if taken >= len(messages) || chain(messages[:taken]) != prefix {
		return false
	}
	rest := messages[taken:]
	if rest[0].role != roleAssistant || !sameCalls(rest[0].calls, pending) {
		return false
	}
	rest = rest[1:]
	answered := map[string]bool{}
	index := 0
	for ; index < len(rest) && rest[index].role == roleTool; index++ {
		for _, identity := range rest[index].order {
			if !slices.Contains(pending, identity) {
				return false
			}
			answered[identity] = true
		}
	}
	if len(answered) != len(pending) || (len(pending) == 0 && index == len(rest)) {
		return false
	}
	for _, message := range rest[index:] {
		if message.role != roleUser {
			return false
		}
	}
	return true
}

func sameCalls(left, right []string) bool {
	left, right = slices.Clone(left), slices.Clone(right)
	slices.Sort(left)
	slices.Sort(right)
	return slices.Equal(left, right)
}

// validArguments reports whether a call's arguments are a JSON object.
func validArguments(raw json.RawMessage) bool {
	var object map[string]json.RawMessage
	return json.Unmarshal(raw, &object) == nil && object != nil
}
