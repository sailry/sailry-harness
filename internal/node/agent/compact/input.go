package compact

import (
	_ "embed"
	"encoding/json"
	"fmt"
	"math"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/tokens"
)

//go:embed prompt.md
var promptText string

const Request = "Please summarize the conversation history provided in the system prompt."

// Compaction has its own context budget. A model catalog can advertise a
// larger window than a gateway serves, and a handoff does not need to replay
// that entire window to preserve the recent work.
const summaryContextLimit = 128_000

// Output is the output limit a summary request asks for in a context window:
// twice the handoff's cap, since the JSON document is larger than the text
// rendered from it. Small windows reserve at most a quarter for output so the
// transcript has room. The model's own output limit also bounds it.
func Output(contextLimit uint64) uint64 {
	return min(2*min(max(contextLimit*2/100, 4000), 12000), contextLimit/4)
}

// Cap bounds the rendered handoff to half the actual summary output budget.
// Normal windows target 2% of context, between 4,000 and 12,000 tokens; smaller
// windows and model output limits can reduce that target.
func Cap(contextLimit, outputLimit uint64) uint64 {
	return min(Output(contextLimit), outputLimit) / 2
}

// instructions is the prompt with its handoff limits: the soft target is three
// quarters of the cap.
func instructions(limit uint64, messages string) string {
	figure := func(value uint64) string {
		if value < 1000 {
			return fmt.Sprint(value)
		}
		return fmt.Sprintf("%d,%03d", value/1000, value%1000)
	}
	return strings.TrimSpace(strings.NewReplacer("{{ target }}", figure(limit*3/4), "{{ cap }}", figure(limit), "{{ messages }}", messages).Replace(promptText))
}

func Prepare(records []canonical.Record, contextLimit, outputLimit uint64, request string, systemBytes int) (string, error) {
	return prepare(records, contextLimit, outputLimit, request, systemBytes, 0)
}

// PrepareSmaller halves the transcript beyond the fixed instructions and prior
// handoff, using the actual size of the rejected request as its baseline.
func PrepareSmaller(records []canonical.Record, contextLimit, outputLimit uint64, request, rejected string) (string, bool) {
	system, err := prepare(records, contextLimit, outputLimit, request, 0, len(rejected))
	return system, err == nil && len(system) < len(rejected)
}

func prepare(records []canonical.Record, contextLimit, outputLimit uint64, request string, systemBytes, rejectedBytes int) (string, error) {
	limit := Cap(contextLimit, outputLimit)
	previousID := ""
	for _, record := range records {
		if record.Kind == "compaction" {
			previousID = record.ID
		}
	}
	records = Visible(records)
	filtered := records[:0]
	previous := ""
	for _, record := range records {
		if record.ID == previousID {
			previous = strings.TrimRight(transcript([]canonical.Record{record}, -1), "\n")
		} else if !turnContext(record) {
			filtered = append(filtered, record)
		}
	}
	records = filtered
	base := instructions(limit, "")
	requestReserve := max(256, tokens.Text(request)+7)
	reserve := outputLimit + tokens.Text(base) + requestReserve
	contextLimit = min(contextLimit, summaryContextLimit)
	if contextLimit < reserve {
		return "", fmt.Errorf("summary prompt and output exceed the context limit")
	}
	budget := contextLimit - reserve
	if previous != "" {
		retained := tokens.Text(previous) + 2
		if retained > budget {
			return "", fmt.Errorf("previous summary exceeds the summary context limit")
		}
		budget -= retained
	}
	prepared := func(body string) string {
		if previous != "" {
			body = strings.TrimRight(previous+"\n"+body, "\n")
		}
		if body != "" {
			body = "[user]: " + body
		}
		return instructions(limit, body)
	}
	byteBudget := math.MaxInt
	if systemBytes > 0 || rejectedBytes > 0 {
		baseBytes := len(prepared(""))
		if rejectedBytes > 0 {
			systemBytes = baseBytes + max(0, rejectedBytes-baseBytes)/2
		}
		if baseBytes > systemBytes {
			return "", fmt.Errorf("summary instructions and previous summary exceed the request byte limit")
		}
		separator := len("[user]: ")
		if previous != "" {
			separator = 1
		}
		byteBudget = max(0, systemBytes-baseBytes-separator)
	}
	var turns [][]canonical.Record
	for _, record := range records {
		if len(turns) == 0 || kickoff(record) {
			turns = append(turns, nil)
		}
		turns[len(turns)-1] = append(turns[len(turns)-1], record)
	}
	if len(turns) == 0 {
		return prepared(""), nil
	}
	last := len(turns) - 1
	history := make([]string, len(turns))
	for i, turn := range turns {
		detail := 0
		if i == last {
			detail = -1
		}
		history[i] = transcript(turn, detail)
	}
	// Each turn is measured once and again only when it is shortened.
	sizes := make([]uint64, len(history))
	var count uint64
	var bytes int
	for i, part := range history {
		sizes[i] = tokens.Text(part) + 2
		count += sizes[i]
		bytes += len(part)
	}
	fit := func() (string, bool) {
		if count > budget || bytes > byteBudget {
			return "", false
		}
		var nonempty []string
		for _, part := range history {
			if part != "" {
				nonempty = append(nonempty, part)
			}
		}
		body := strings.Join(nonempty, "\n")
		if len(body) > byteBudget || tokens.Text(body) > budget {
			return "", false
		}
		return body, true
	}
	body, ok := fit()
	for detail := 1; !ok && detail <= 3; detail++ {
		for i := 0; i < last; i++ {
			bytes -= len(history[i])
			history[i] = transcript(turns[i], detail)
			bytes += len(history[i])
			count -= sizes[i]
			sizes[i] = tokens.Text(history[i]) + 2
			count += sizes[i]
			body, ok = fit()
			if ok {
				break
			}
		}
	}
	// The older turns are gone. The latest one, alone, is shortened only as far
	// as it has to be: whole, tool responses to their edges, tool responses
	// omitted, then its oldest records.
	if !ok && len(history[last]) <= byteBudget && sizes[last]-2 <= budget {
		body, ok = history[last], true
	}
	for detail := 0; !ok && detail <= 1; detail++ {
		body = transcript(turns[last], detail)
		ok = len(body) <= byteBudget && tokens.Text(body) <= budget
	}
	if !ok {
		var err error
		if body, err = recent(turns[last], budget, byteBudget); err != nil {
			return "", err
		}
	}
	return prepared(body), nil
}

// kickoff reports a message the user wrote, which starts a turn.
func kickoff(record canonical.Record) bool {
	message, _, _ := record.ModelContent()
	if message == nil || message.Role != canonical.User {
		return false
	}
	hasInput, hasTool := false, false
	for _, block := range message.Content {
		hasInput = hasInput || block.Kind == "text" || block.Kind == "image" || block.Kind == "document" || block.Kind == "file"
		hasTool = hasTool || block.Kind == "tool_response" || block.Kind == "tool_request"
	}
	return hasInput && !hasTool
}

// recent is the latest turn when it does not fit with its tool responses
// omitted: the user's message that started it, one line saying how many
// records were left out, and the newest records that fit after it.
func recent(turn []canonical.Record, budget uint64, byteBudget int) (string, error) {
	head, rest := "", turn
	if kickoff(turn[0]) {
		head, rest = transcript(turn[:1], 1), turn[1:]
	}
	var parts []string
	for i := range rest {
		if part := transcript(rest[i:i+1], 1); part != "" {
			parts = append(parts, part)
		}
	}
	// The estimate reserves the marker line; the joined text decides.
	used := tokens.Text(head) + 16
	bytes := len(head)
	first := len(parts)
	for first > 0 {
		part := parts[first-1]
		size := tokens.Text(part) + 1
		if used+size > budget || bytes+len(part) > byteBudget {
			break
		}
		used += size
		bytes += len(part) + 1
		first--
	}
	for ; ; first++ {
		if first >= len(parts) && head == "" && len(parts) > 0 {
			return "", fmt.Errorf("latest record exceeds the summary context limit")
		}
		var lines []string
		if head != "" {
			lines = append(lines, head)
		}
		if first > 0 {
			lines = append(lines, fmt.Sprintf("[%d earlier records of this turn omitted]", first))
		}
		body := strings.Join(append(lines, parts[first:]...), "\n")
		if len(body) <= byteBudget && tokens.Text(body) <= budget {
			return body, nil
		}
		if first >= len(parts) {
			return "", fmt.Errorf("latest user message exceeds the summary context limit")
		}
	}
}
func transcript(records []canonical.Record, detail int) string {
	if detail == 3 {
		return ""
	}
	var parts []string
	for _, record := range records {
		message, background, _ := record.ModelContent()
		if message == nil {
			if detail < 2 && background != "" {
				parts = append(parts, "[runtime]: "+background)
			}
			continue
		}
		if detail == 2 && (message.Role != canonical.User || hasTools(message)) {
			continue
		}
		for _, block := range message.Content {
			var text string
			switch value := block.Data.(type) {
			case *canonical.TextContent:
				text = value.Text
			case *canonical.AssetContent:
				text = "[" + block.Kind + ": " + value.Asset.Mime + "]"
			case *canonical.FileContent:
				text = "[file]"
				if strings.HasPrefix(value.File.URI, "sailry-attachment://") {
					text = "[file: " + value.File.URI + "]"
				}
			case *canonical.ToolRequest:
				if value.Error != nil {
					text = "tool_request: [error]"
				} else {
					text = "tool_request(" + value.Name + "): " + string(value.Arguments)
				}
			case *canonical.ToolResponse:
				if detail == 1 {
					text = "tool_response: [omitted]"
				} else if value.Format == canonical.MCPError {
					text = "tool_response: [error]"
				} else {
					text = "tool_response: " + resultText(value)
					if detail == 0 {
						text = "tool_response: " + edges(strings.TrimPrefix(text, "tool_response: "))
					}
				}
			default:
				continue
			}
			formatted := "[" + string(message.Role) + "]: " + text
			if detail == 1 && block.Kind == "tool_request" {
				formatted = edges(formatted)
			}
			parts = append(parts, formatted)
			if result, ok := block.Data.(*canonical.ToolResponse); ok && result.Format != canonical.MCPError && result.Revision != "" {
				reference, _ := json.Marshal(map[string]any{"output": map[string]string{"call": result.ID, "revision": result.Revision}, "next_offset": nil})
				parts = append(parts, string(reference))
			}
		}
		if message.Error != nil {
			parts = append(parts, "["+string(message.Role)+"]: error: "+message.Error.Message)
		}
		if detail < 2 && background != "" {
			parts = append(parts, "[runtime]: "+background)
		}
	}
	return strings.Join(parts, "\n")
}
func hasTools(message *canonical.Message) bool {
	for _, block := range message.Content {
		if block.Kind == "tool_request" || block.Kind == "tool_response" {
			return true
		}
	}
	return false
}
func resultText(value *canonical.ToolResponse) string {
	if value.Format == canonical.JSON {
		return string(value.Result.Value)
	}
	var result struct {
		Content []struct {
			Type string `json:"type"`
			Text string `json:"text"`
		} `json:"content"`
	}
	if json.Unmarshal(value.Result.Value, &result) != nil {
		return "[non-text content]"
	}
	var texts []string
	for _, part := range result.Content {
		if part.Type == "text" {
			texts = append(texts, part.Text)
		}
	}
	if len(texts) == 0 {
		return "[non-text content]"
	}
	return strings.Join(texts, "\n")
}
func edges(text string) string {
	runes := []rune(text)
	if len(runes) <= 2000 {
		return text
	}
	return string(runes[:1000]) + fmt.Sprintf("\n[%d characters omitted]\n", len(runes)-2000) + string(runes[len(runes)-1000:])
}
