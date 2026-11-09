package model

// Claude cache breakpoints derive from oh-my-pi c29f7c56 through Sailry's
// reviewed provider request adapter. MIT; see LICENSE.omp.

import "strings"

func claudeCache(body map[string]any) {
	budget := 4
	control := func() map[string]any { return map[string]any{"type": "ephemeral", "ttl": "1h"} }
	tools, _ := body["tools"].([]any)
	for _, value := range tools {
		delete(value.(map[string]any), "cache_control")
	}
	for index := len(tools) - 1; index >= 0; index-- {
		tool := tools[index].(map[string]any)
		if tool["defer_loading"] != true {
			tool["cache_control"] = control()
			budget--
			break
		}
	}
	system, _ := body["system"].([]any)
	suffix := len(system)
	for index, value := range system {
		block := value.(map[string]any)
		delete(block, "cache_control")
		text, _ := block["text"].(string)
		if suffix == len(system) && (strings.HasPrefix(text, "<memories>") || strings.HasPrefix(text, "<project-context>")) {
			suffix = index
		}
	}
	anchor := suffix - 1
	if suffix == 0 {
		anchor = len(system) - 1
	}
	if anchor >= 0 {
		system[anchor].(map[string]any)["cache_control"] = control()
		budget--
	}
	messages, _ := body["messages"].([]any)
	for _, value := range messages {
		message := value.(map[string]any)
		blocks, _ := message["content"].([]any)
		for _, value := range blocks {
			delete(value.(map[string]any), "cache_control")
		}
	}
	conversational := func(message map[string]any) bool {
		if message["role"] != "user" {
			return false
		}
		if _, ok := message["content"].(string); ok {
			return true
		}
		blocks, _ := message["content"].([]any)
		for _, value := range blocks {
			block := value.(map[string]any)
			if block["type"] == "text" || block["type"] == "image" {
				return true
			}
		}
		return false
	}
	trailing := []int{}
	mark, turns := -1, 0
	for index := len(messages) - 1; index >= 0; index-- {
		message := messages[index].(map[string]any)
		if message["clear_at"] == "next_user_message" {
			mark, turns = index, 0
			continue
		}
		blocks, _ := message["content"].([]any)
		toolControl := message["role"] == "system" && len(blocks) > 0
		for _, value := range blocks {
			block := value.(map[string]any)
			toolControl = toolControl && (block["type"] == "tool_addition" || block["type"] == "tool_removal")
		}
		if len(trailing) < 2 && !toolControl {
			trailing = append(trailing, index)
		}
		if conversational(message) {
			turns++
		}
	}
	candidates := []int{}
	if len(trailing) > 0 {
		candidates = append(candidates, trailing[0])
	}
	stableEnd := len(messages)
	if mark >= 0 {
		stableEnd = mark
	}
	if turns >= 15 {
		remaining := turns % 15
		for index := stableEnd - 1; index >= 0; index-- {
			if !conversational(messages[index].(map[string]any)) {
				continue
			}
			if remaining > 0 {
				remaining--
				continue
			}
			remaining = 14
			candidates = append(candidates, index)
			if len(candidates) > budget {
				break
			}
		}
	}
	if mark > 0 {
		candidates = append(candidates, mark-1)
	}
	candidates = append(candidates, trailing...)
	for _, index := range candidates {
		if budget == 0 {
			break
		}
		message := messages[index].(map[string]any)
		if text, ok := message["content"].(string); ok {
			message["content"] = []any{map[string]any{"type": "text", "text": text, "cache_control": control()}}
			budget--
			continue
		}
		blocks, _ := message["content"].([]any)
		for index := len(blocks) - 1; index >= 0; index-- {
			block := blocks[index].(map[string]any)
			switch block["type"] {
			case "thinking", "redacted_thinking", "fallback", "tool_addition", "tool_removal":
				continue
			}
			if _, exists := block["cache_control"]; !exists {
				block["cache_control"] = control()
				budget--
			}
			break
		}
	}
}
