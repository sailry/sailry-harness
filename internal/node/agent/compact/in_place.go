package compact

import (
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/tokens"
)

// InPlace returns the instructions for a summary request that carries the
// conversation as its own history instead of a rendered transcript. Sending
// the main request's system prompt, tools and history unchanged lets the
// provider serve that prefix from its prompt cache. The tail is the retained
// history that follows the handoff verbatim; the model is told where it
// begins so it summarizes only what is being replaced.
func InPlace(contextLimit, outputLimit uint64, tail []canonical.Record) string {
	task := "## Task\nStop working on the task. You are writing a handoff for yourself that replaces the conversation above. "
	if role, quote := tailQuote(tail); quote != "" {
		task += "The messages from the " + role + " message that begins \"" + quote + "\" onward are kept verbatim after the handoff, so summarize only the history before that message. "
	} else if len(Visible(tail)) > 0 {
		task += "The most recent messages are kept verbatim after the handoff, so summarize the history before them and do not repeat their details. "
	}
	task += "Do not invent later progress. Produce the handoff only: do not continue the task, answer questions, or call tools."
	text := instructions(Cap(contextLimit, outputLimit), "")
	// The embedded prompt's task section names an inline transcript; the rest
	// of the prompt applies unchanged.
	start, end := strings.Index(text, "## Task"), strings.Index(text, "## What to keep")
	return text[:start] + task + "\n\n" + text[end:]
}

// InPlaceTokens estimates the input an in-place summary request adds to the
// conversation it summarizes.
func InPlaceTokens(instructions string) uint64 { return tokens.Text(instructions) + 16 }

// tailQuote is the opening of the retained tail's first message, on one line,
// so the model can find where the replaced history ends. A tail cut within a
// turn can begin with the model's own text; one that begins with a tool
// exchange has no quotable opening, and the quote is empty.
func tailQuote(tail []canonical.Record) (role, quote string) {
	for _, record := range Visible(tail) {
		if turnContext(record) {
			continue
		}
		message, _, _ := record.ModelContent()
		switch {
		case message == nil:
			continue
		case kickoff(record):
			role = "user"
		case message.Role == canonical.Assistant:
			role = "assistant"
		default:
			return "", ""
		}
		for _, block := range message.Content {
			text, ok := block.Data.(*canonical.TextContent)
			if !ok {
				continue
			}
			line := strings.Join(strings.FieldsFunc(text.Text, unicode.IsSpace), " ")
			line = strings.ReplaceAll(line, "\"", "'")
			if runes := []rune(line); len(runes) > 120 {
				line = string(runes[:120]) + "…"
			}
			if line != "" {
				return role, line
			}
		}
		return "", ""
	}
	return "", ""
}
