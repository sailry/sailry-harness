package model

import (
	"encoding/json"
	"testing"
)

// Updates whose content is a list, such as a tool call the agent's hook
// refused, are accepted and ignored; message and thought chunks still reach
// the prompt in flight.
func TestACPUpdatesOfEveryContentShape(t *testing.T) {
	var got []string
	agent := &acpAgent{sinks: map[string]func(bool, string){"s": func(reasoning bool, text string) {
		if reasoning {
			text = "thought:" + text
		}
		got = append(got, text)
	}}, commands: map[string]map[string]bool{}}
	for _, update := range []string{
		`{"sessionId":"s","update":{"sessionUpdate":"tool_call","toolCallId":"t","title":"ls","status":"pending","content":[{"type":"content","content":{"type":"text","text":"x"}}]}}`,
		`{"sessionId":"s","update":{"sessionUpdate":"tool_call_update","toolCallId":"t","status":"failed","content":[{"type":"content","content":{"type":"text","text":"Built-in tools are disabled."}}]}}`,
		`{"sessionId":"s","update":{"sessionUpdate":"plan","entries":[{"content":"step","priority":"high","status":"pending"}]}}`,
		`{"sessionId":"s","update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"why"}}}`,
		`{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"ok"}}}`,
	} {
		if _, failure := agent.handle(t.Context(), "session/update", json.RawMessage(update)); failure != nil {
			t.Fatalf("%s: %v", update, failure)
		}
	}
	if len(got) != 2 || got[0] != "thought:why" || got[1] != "ok" {
		t.Fatalf("delivered %q", got)
	}
}
