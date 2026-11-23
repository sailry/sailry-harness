package memorynode

import (
	"encoding/json"
	"regexp"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	relevantLimit = 3
	saveCueText   = "Use save_memory only for an explicit memory request or guidance for a separate future session. Never save secrets."
)

var (
	// Matches requests to remember or explicitly recurring guidance (EN + ZH).
	// Go's RE2 does not support negative lookahead `(?!\s+to\b)`, so we verify
	// "remember to" explicitly in isCued below to match policy.js identically.
	cueENPrefixRE = regexp.MustCompile(`(?i)(?:^|[.!?\n])\s*(?:(?:please |(?:can|could) you )?remember\b|from now on\b|(?:in|for) future sessions\b|i (?:usually|generally) prefer\b|our standing (?:rule|convention)\b)`)
	cueENRemember = regexp.MustCompile(`(?i)(?:^|[.!?\n])\s*(?:(?:please |(?:can|could) you )?remember)\b(\s+to\b)?`)
	cueZHRE       = regexp.MustCompile("(?:^|[\u3002\uff01\uff1f\n])\\s*(?:\u8bf7)?(?:\u8bb0\u4f4f|(?:\u4ee5\u540e|\u4eca\u540e)\u90fd|\u6211\u4e60\u60ef|\u6211\u7684\u957f\u671f\u504f\u597d|\u6211\u4eec\u7684\u60ef\u4f8b)")
)

func isCued(message string) bool {
	if message == "" {
		return false
	}
	if cueZHRE.MatchString(message) {
		return true
	}
	for _, loc := range cueENPrefixRE.FindAllStringIndex(message, -1) {
		sub := message[loc[0]:]
		m := cueENRemember.FindStringSubmatchIndex(sub)
		if m != nil && m[0] == 0 {
			// Skip "remember to ..." (negative lookahead in policy.js).
			if m[2] >= 0 && m[3] > m[2] {
				continue
			}
		}
		return true
	}
	return false
}

type contentBlock struct {
	Role  string        `json:"role"`
	Parts []contentPart `json:"parts"`
}

type contentPart struct {
	Text string `json:"text"`
}

func makeContent(text string) contentBlock {
	return contentBlock{Role: "user", Parts: []contentPart{{Text: text}}}
}

func contentCost(text string) int {
	b, _ := json.Marshal(makeContent(text))
	return len(b) + 128
}

func relevantParts(relevant []memoryEntry, limit int) []string {
	var parts []string
	size := 0
	for _, item := range relevant {
		if len(parts) == relevantLimit {
			break
		}
		payload, _ := json.Marshal(map[string]any{
			"id": item.Summary.ID, "title": item.Summary.Title, "kind": item.Summary.Kind,
			"global": item.Summary.Project == nil, "body": item.Body,
		})
		part := "Relevant memory: " + string(payload)
		added := len(part)
		if size+added > limit {
			continue
		}
		parts = append(parts, part)
		size += added
	}
	return parts
}

func buildTurnParts(relevant []memoryEntry, message string, settings memorySettings, write bool) []string {
	save := write && isCued(message)
	limit := settings.ContextBytes / 2
	if save {
		limit -= len(saveCueText)
	}
	parts := relevantParts(relevant, limit)
	if save {
		parts = append(parts, saveCueText)
	}
	return parts
}

type retrievedPayload struct {
	Body          string        `json:"body"`
	BodyTruncated bool          `json:"body_truncated"`
	Summary       memorySummary `json:"summary"`
}

func retrieveEntries(entries []memoryEntry, remaining *int) []contentBlock {
	results := make([]contentBlock, 0, len(entries))
	for _, entry := range entries {
		runes := []rune(entry.Body)
		encode := func(end int) string {
			b, _ := json.Marshal(retrievedPayload{
				Body:          string(runes[:end]),
				BodyTruncated: end < len(runes),
				Summary:       entry.Summary,
			})
			return string(b)
		}
		text := encode(len(runes))
		if contentCost(text) > *remaining {
			lo, hi := 0, len(runes)+1
			for lo < hi {
				mid := (lo + hi) / 2
				if contentCost(encode(mid)) <= *remaining {
					lo = mid + 1
				} else {
					hi = mid
				}
			}
			if lo <= 1 {
				continue
			}
			text = encode(lo - 1)
		}
		size := contentCost(text)
		if size <= *remaining {
			*remaining -= size
			results = append(results, makeContent(text))
		}
	}
	return results
}

type readAuthority struct {
	id       string
	revision int64
}

func extractReads(name string, raw json.RawMessage) []readAuthority {
	var top map[string]any
	if json.Unmarshal(raw, &top) != nil || top == nil {
		return nil
	}
	response := top
	if ctxObj, ok := top["context"].(map[string]any); ok {
		response = ctxObj
	}
	var entries []any
	switch name {
	case "search_memory":
		entries, _ = response["entries"].([]any)
	case "save_memory":
		if response["status"] == "similar" && response["saved"] == false {
			entries, _ = response["existing"].([]any)
		} else if response["status"] == "changed" && response["saved"] == false {
			entries, _ = response["latest"].([]any)
		}
	case "consolidate_memories":
		if response["status"] == "changed" && response["saved"] == false {
			entries, _ = response["latest"].([]any)
		}
	}
	if len(entries) == 0 {
		return nil
	}
	var out []readAuthority
	for _, entry := range entries {
		em, ok := entry.(map[string]any)
		if !ok {
			continue
		}
		parts, _ := em["parts"].([]any)
		for _, p := range parts {
			pm, ok := p.(map[string]any)
			if !ok {
				continue
			}
			text, ok := pm["text"].(string)
			if !ok {
				continue
			}
			var decoded retrievedPayload
			if json.Unmarshal([]byte(text), &decoded) == nil && !decoded.BodyTruncated && decoded.Body != "" && decoded.Summary.ID != "" && decoded.Summary.Revision >= 0 {
				out = append(out, readAuthority{id: decoded.Summary.ID, revision: decoded.Summary.Revision})
			}
		}
	}
	return out
}

func hasReadAuthority(reads []readAuthority, id string, revision int64) error {
	for _, r := range reads {
		if r.id == id && r.revision == revision {
			return nil
		}
	}
	return failFault(protocol.ErrorCodeInvalidRequest, "call search_memory with this ID to read its complete current body, then retry with its revision; titles and truncated bodies are not enough")
}

func isMemoryToolName(name string) (string, bool) {
	switch name {
	case "search_memory", "review_memories", "save_memory", "forget_memory", "consolidate_memories":
		return name, true
	}
	if strings.HasPrefix(name, "plugin_") {
		if idx := strings.IndexByte(name[len("plugin_"):], '_'); idx >= 0 {
			bare := name[len("plugin_")+idx+1:]
			switch bare {
			case "search_memory", "review_memories", "save_memory", "forget_memory", "consolidate_memories":
				return bare, true
			}
		}
	}
	return "", false
}
