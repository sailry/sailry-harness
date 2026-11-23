// Package hooks runs captured workspace scripts around Node-owned tool execution.
package hooks

import (
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type Entry struct {
	ID     string
	Phase  protocol.HooksPhase
	Script protocol.HooksScript
}

// entries preserves the protocol's fixed slots followed by additional scripts.
func entries(settings protocol.HooksSettings) []Entry {
	entries := []Entry{}
	for _, slot := range []struct {
		phase  protocol.HooksPhase
		script *protocol.HooksScript
	}{
		{protocol.HooksPhaseBeforeTool, settings.BeforeTool},
		{protocol.HooksPhaseAfterTool, settings.AfterTool},
		{protocol.HooksPhaseTurnEnd, settings.TurnEnd},
	} {
		if slot.script != nil {
			entries = append(entries, Entry{string(slot.phase), slot.phase, *slot.script})
		}
	}
	for _, entry := range settings.Additional {
		entries = append(entries, Entry{entry.ID, entry.Phase, entry.Script})
	}
	return entries
}

func Scripts(settings protocol.HooksSettings, phase protocol.HooksPhase) []Entry {
	selected := []Entry{}
	if settings.Enabled {
		for _, entry := range entries(settings) {
			if entry.Phase == phase && entry.Script.Enabled {
				selected = append(selected, entry)
			}
		}
	}
	return selected
}

// MatchesTool reports whether a hook script applies to the named tool.
// An empty tool filter matches every tool for backward compatibility.
// Filter entries match exact tool names or trailing-wildcard prefixes (e.g. "git_*").
func MatchesTool(script protocol.HooksScript, tool string) bool {
	if len(script.Tools) == 0 {
		return true
	}
	for _, pattern := range script.Tools {
		pattern = strings.TrimSpace(pattern)
		if pattern == "" {
			continue
		}
		if pattern == "*" || pattern == tool {
			return true
		}
		if prefix, ok := strings.CutSuffix(pattern, "*"); ok && strings.HasPrefix(tool, prefix) {
			return true
		}
	}
	return false
}

// ScriptsForTool returns enabled hook scripts for a tool phase that match the given tool name.
func ScriptsForTool(settings protocol.HooksSettings, phase protocol.HooksPhase, tool string) []Entry {
	selected := []Entry{}
	if !settings.Enabled {
		return selected
	}
	for _, entry := range entries(settings) {
		if entry.Phase == phase && entry.Script.Enabled && MatchesTool(entry.Script, tool) {
			selected = append(selected, entry)
		}
	}
	return selected
}

func Validate(settings protocol.HooksSettings) error {
	ids := map[string]bool{"before_tool": true, "after_tool": true, "turn_end": true}
	for _, entry := range settings.Additional {
		if strings.TrimSpace(entry.ID) == "" || len(entry.ID) > 128 || strings.ContainsRune(entry.ID, 0) || ids[entry.ID] {
			return invalid("hook entry identity is invalid")
		}
		ids[entry.ID] = true
		if entry.Phase != protocol.HooksPhaseBeforeTool && entry.Phase != protocol.HooksPhaseAfterTool && entry.Phase != protocol.HooksPhaseTurnEnd {
			return invalid("hook phase is invalid")
		}
	}
	entries := entries(settings)
	for _, entry := range entries {
		script := entry.Script
		if len(script.Name) > 1024 || strings.ContainsRune(script.Name, 0) || strings.TrimSpace(script.Command) == "" || len(script.Command) > 64*1024 || strings.ContainsRune(script.Command, 0) || script.TimeoutMs < 1 || script.TimeoutMs > 60000 {
			return invalid("hook command or timeout is invalid")
		}
		for _, tool := range script.Tools {
			trimmed := strings.TrimSpace(tool)
			if trimmed == "" || len(trimmed) > 128 || strings.ContainsRune(tool, 0) {
				return invalid("hook tool filter is invalid")
			}
		}
	}
	if settings.Enabled && len(entries) == 0 {
		return invalid("enabled hooks require a command")
	}
	return nil
}

func invalid(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}
