// Package acpfixture is a scripted Agent Client Protocol agent for tests. A
// test binary serves as the agent: its TestMain calls Serve when Variable is
// set. The agent replies from script.json in its working directory, logs every
// line it receives to requests.log there and records the sessions it opened
// in sessions.log, so a program started again can resume them.
package acpfixture

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"slices"
	"strings"
)

// Variable names the scenario: comma-separated flags.
//
//   - meta: the agent takes system text in vendor.systemPrompt.append
//   - variants, options, unlisted: how sessions list models
//   - signed-out, login: authentication
//   - resume, load, close: the session methods the agent offers
//   - forget: resuming or loading any session fails
//   - compact: sessions advertise a /compact command, which answers without
//     taking a scripted reply
//   - compact-hang: /compact answers only when it is cancelled
const Variable = "SAILRY_ACP_FIXTURE"

// Compact is the prompt text of the native compaction command.
const Compact = "/compact"

// Reply is the scripted answer to one prompt. Prompts are counted across
// every program started in the same directory.
type Reply struct {
	Thoughts   []string       `json:"thoughts"`
	Chunks     []string       `json:"chunks"`
	Stop       string         `json:"stop"`
	Usage      map[string]any `json:"usage"`
	Permission bool           `json:"permission"`
	Hang       bool           `json:"hang"`
	Crash      bool           `json:"crash"`
}

// Serve speaks the protocol on the standard streams until input ends.
func Serve(scenario string) {
	previous, _ := os.ReadFile("requests.log")
	prompts := 0
	for _, line := range strings.Split(string(previous), "\n") {
		if strings.Contains(line, `"method":"session/prompt"`) && !strings.Contains(line, `"text":"`+Compact+`"`) {
			prompts++
		}
	}
	log, err := os.OpenFile("requests.log", os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
	if err != nil {
		os.Exit(2)
	}
	record := func(value any) {
		line, _ := json.Marshal(value)
		log.Write(append(line, '\n'))
	}
	record(map[string]any{"started": os.Args[1:], "pid": os.Getpid(), "key": os.Getenv("FIXTURE_KEY"), "home": os.Getenv("HOME"), "leak": os.Getenv("FIXTURE_LEAK")})
	var script []Reply
	if raw, err := os.ReadFile("script.json"); err == nil {
		json.Unmarshal(raw, &script)
	}
	send := func(value map[string]any) {
		value["jsonrpc"] = "2.0"
		line, _ := json.Marshal(value)
		os.Stdout.Write(append(line, '\n'))
	}
	has := func(flag string) bool { return strings.Contains(","+scenario+",", ","+flag+",") }
	known := func(id string) bool {
		raw, _ := os.ReadFile("sessions.log")
		return id != "" && slices.Contains(strings.Split(string(raw), "\n"), id)
	}
	models := func(result map[string]any) {
		switch {
		case has("variants"):
			result["models"] = map[string]any{"availableModels": []any{
				map[string]any{"modelId": "captured-low", "name": "Captured (Low)"},
				map[string]any{"modelId": "captured-high", "name": "Captured (High)"},
				map[string]any{"modelId": "plain", "name": "Plain"},
			}}
		case has("options"):
			result["configOptions"] = []any{
				map[string]any{"id": "model", "category": "model", "type": "select", "currentValue": "other", "options": []any{
					map[string]any{"value": "captured", "name": "Captured"}, map[string]any{"value": "other", "name": "Other"}}},
				map[string]any{"id": "thought_level", "category": "thought_level", "type": "select", "currentValue": "low", "options": []any{
					map[string]any{"value": "low", "name": "Low"}, map[string]any{"value": "high", "name": "High"}}},
			}
		case !has("unlisted"):
			result["models"] = map[string]any{"availableModels": []any{map[string]any{"modelId": "captured", "name": "Captured"}}}
		}
	}
	commands := func(session string) {
		if has("compact") {
			send(map[string]any{"method": "session/update", "params": map[string]any{"sessionId": session, "update": map[string]any{
				"sessionUpdate": "available_commands_update", "availableCommands": []any{map[string]any{"name": "compact", "description": "Compact"}}}}})
		}
	}
	sessions, authenticated := 0, false
	var pending json.RawMessage
	var waiting *Reply
	session := ""
	answer := func(reply Reply) {
		update := func(kind, text string) {
			send(map[string]any{"method": "session/update", "params": map[string]any{"sessionId": session, "update": map[string]any{
				"sessionUpdate": kind, "content": map[string]any{"type": "text", "text": text}}}})
		}
		for _, text := range reply.Thoughts {
			update("agent_thought_chunk", text)
		}
		for _, text := range reply.Chunks {
			update("agent_message_chunk", text)
		}
		if reply.Crash {
			fmt.Fprintln(os.Stderr, "fixture agent crashed")
			os.Exit(3)
		}
		if reply.Hang {
			return
		}
		stop := reply.Stop
		if stop == "" {
			stop = "end_turn"
		}
		result := map[string]any{"stopReason": stop}
		if reply.Usage != nil {
			result["usage"] = reply.Usage
		}
		send(map[string]any{"id": pending, "result": result})
		pending = nil
	}
	lines := bufio.NewScanner(os.Stdin)
	lines.Buffer(make([]byte, 0, 1024*1024), 16*1024*1024)
	for lines.Scan() {
		log.Write(append(append([]byte(nil), lines.Bytes()...), '\n'))
		var message struct {
			ID     json.RawMessage `json:"id"`
			Method string          `json:"method"`
			Params struct {
				SessionID string `json:"sessionId"`
				Prompt    []struct {
					Text string `json:"text"`
				} `json:"prompt"`
			} `json:"params"`
		}
		if json.Unmarshal(lines.Bytes(), &message) != nil {
			continue
		}
		switch message.Method {
		case "initialize":
			capabilities := map[string]any{"promptCapabilities": map[string]any{"image": true}}
			if has("meta") {
				capabilities["_meta"] = map[string]any{"vendor": map[string]any{"systemPrompt": map[string]any{"append": true}}}
			}
			if has("load") {
				capabilities["loadSession"] = true
			}
			methods := map[string]any{}
			for _, method := range []string{"resume", "close"} {
				if has(method) {
					methods[method] = map[string]any{}
				}
			}
			if len(methods) > 0 {
				capabilities["sessionCapabilities"] = methods
			}
			send(map[string]any{"id": message.ID, "result": map[string]any{"protocolVersion": 1, "agentCapabilities": capabilities, "authMethods": []any{}}})
		case "authenticate":
			authenticated = true
			send(map[string]any{"id": message.ID, "result": map[string]any{}})
		case "session/new":
			if has("signed-out") || has("login") && !authenticated {
				send(map[string]any{"id": message.ID, "error": map[string]any{"code": -32000, "message": "Authentication required"}})
				continue
			}
			sessions++
			session = fmt.Sprintf("session-%d-%d", os.Getpid(), sessions)
			if file, err := os.OpenFile("sessions.log", os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600); err == nil {
				fmt.Fprintln(file, session)
				file.Close()
			}
			result := map[string]any{"sessionId": session}
			models(result)
			send(map[string]any{"id": message.ID, "result": result})
			commands(session)
		case "session/resume", "session/load":
			if has("forget") || !known(message.Params.SessionID) || message.Method == "session/resume" && !has("resume") || message.Method == "session/load" && !has("load") {
				send(map[string]any{"id": message.ID, "error": map[string]any{"code": -32602, "message": "Unknown session"}})
				continue
			}
			session = message.Params.SessionID
			result := map[string]any{}
			if message.Method == "session/load" {
				// A loaded session's history arrives as updates before the answer.
				send(map[string]any{"method": "session/update", "params": map[string]any{"sessionId": session, "update": map[string]any{
					"sessionUpdate": "agent_message_chunk", "content": map[string]any{"type": "text", "text": "earlier reply"}}}})
				models(result)
			}
			send(map[string]any{"id": message.ID, "result": result})
			commands(session)
		case "session/close":
			send(map[string]any{"id": message.ID, "result": map[string]any{}})
		case "session/set_model":
			send(map[string]any{"id": message.ID, "result": map[string]any{}})
		case "session/set_config_option":
			send(map[string]any{"id": message.ID, "result": map[string]any{"configOptions": []any{}}})
		case "session/prompt":
			if len(message.Params.Prompt) == 1 && message.Params.Prompt[0].Text == Compact {
				if !has("compact") {
					send(map[string]any{"id": message.ID, "error": map[string]any{"code": -32602, "message": "Unknown command"}})
					continue
				}
				if has("compact-hang") {
					pending = message.ID
					continue
				}
				send(map[string]any{"id": message.ID, "result": map[string]any{"stopReason": "end_turn"}})
				continue
			}
			reply := Reply{Chunks: []string{"ok"}}
			if prompts < len(script) {
				reply = script[prompts]
			}
			prompts++
			pending, session = message.ID, message.Params.SessionID
			if reply.Permission {
				waiting = &reply
				send(map[string]any{"id": 900, "method": "session/request_permission", "params": map[string]any{"sessionId": session,
					"toolCall": map[string]any{"toolCallId": "builtin"},
					"options":  []any{map[string]any{"optionId": "yes", "name": "Allow", "kind": "allow_once"}, map[string]any{"optionId": "no", "name": "Reject", "kind": "reject_once"}}}})
				continue
			}
			answer(reply)
		case "session/cancel":
			if pending != nil {
				send(map[string]any{"id": pending, "result": map[string]any{"stopReason": "cancelled"}})
				pending = nil
			}
		case "":
			// The answer to the permission request releases the held reply.
			if waiting != nil {
				reply := *waiting
				waiting = nil
				answer(reply)
			}
		default:
			if message.ID != nil {
				send(map[string]any{"id": message.ID, "error": map[string]any{"code": -32601, "message": "Method not found"}})
			}
		}
	}
}
