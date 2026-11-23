package model

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"hash"
	"slices"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// scopeReplay keeps native thinking only with the projected request that made
// it. Unknown older captures lose only their replay blocks, not their history.
// origins addresses messages in the original Prompt, before empty ones are cut.
func scopeReplay(binding Binding, call *fantasy.Call, origins map[int]string) (string, error) {
	if binding.Provider.Api != protocol.ConversationModelApiAnthropic {
		return "", nil
	}
	entry, found := catalogEntry("anthropic", binding.Config.Model)
	if !found || !entry.Rig.BindsContext {
		return "", nil
	}
	config := transportConfig{Kind: "anthropic", Model: binding.Config.Model}
	body, err := requestJSON(config, *call)
	if err != nil {
		return "", err
	}
	raw, err := sourceRequestBytes(config, *call, body)
	if err != nil {
		return "", err
	}
	var wire struct {
		System   []any `json:"system"`
		Tools    []any `json:"tools"`
		Messages []struct {
			Role    string `json:"role"`
			Content []any  `json:"content"`
		} `json:"messages"`
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.UseNumber()
	if err := decoder.Decode(&wire); err != nil {
		return "", err
	}
	replayCacheMarkers(wire.System)
	for _, value := range wire.Tools {
		if tool, ok := value.(map[string]any); ok {
			delete(tool, "cache_control")
		}
	}
	prefix := sha256.New()
	prefix.Write([]byte("sailry-anthropic-replay-v1\x00"))
	flavor := string(binding.Provider.Authentication)
	if binding.Provider.Authentication == protocol.AuthenticationClaudeOAuth {
		flavor += ":" + claudeVersion
	}
	if err := replayFrame(prefix, "transport", flavor); err != nil {
		return "", err
	}
	if err := replayFrame(prefix, "system", wire.System); err != nil {
		return "", err
	}
	if err := replayFrame(prefix, "tools", wire.Tools); err != nil {
		return "", err
	}
	projected := make(fantasy.Prompt, 0, len(call.Prompt))
	position := 0
	for index := 0; index < len(call.Prompt); index++ {
		message := call.Prompt[index]
		if message.Role == fantasy.MessageRoleSystem {
			projected = append(projected, message)
			continue
		}
		if position >= len(wire.Messages) {
			return "", fmt.Errorf("model replay messages do not match the prompt")
		}
		encoded := wire.Messages[position]
		position++
		replayCacheMarkers(encoded.Content)
		if message.Role == fantasy.MessageRoleTool {
			if err := replayFrame(prefix, "message", encoded); err != nil {
				return "", err
			}
			projected = append(projected, message)
			for index+1 < len(call.Prompt) && call.Prompt[index+1].Role == fantasy.MessageRoleTool {
				index++
				projected = append(projected, call.Prompt[index])
			}
			continue
		}
		if message.Role == fantasy.MessageRoleAssistant && origins[index] != hex.EncodeToString(prefix.Sum(nil)) {
			content := slices.DeleteFunc(slices.Clone(message.Content), func(part fantasy.MessagePart) bool {
				return part.GetType() == fantasy.ContentTypeReasoning
			})
			if len(content) != len(message.Content) {
				message.Content = content
				encoded.Content = slices.DeleteFunc(encoded.Content, func(value any) bool {
					block, _ := value.(map[string]any)
					return block["type"] == "thinking" || block["type"] == "redacted_thinking"
				})
				if len(content) == 0 {
					continue
				}
			}
		}
		if err := replayFrame(prefix, "message", encoded); err != nil {
			return "", err
		}
		projected = append(projected, message)
	}
	if position != len(wire.Messages) {
		return "", fmt.Errorf("model replay messages do not match the prompt")
	}
	call.Prompt = projected
	return hex.EncodeToString(prefix.Sum(nil)), nil
}

// Cache annotations are not content. Limit removal to wire block boundaries;
// a tool's input or JSON Schema can legitimately contain a cache_control field.
func replayCacheMarkers(blocks []any) {
	for _, value := range blocks {
		block, ok := value.(map[string]any)
		if !ok {
			continue
		}
		delete(block, "cache_control")
		if block["type"] == "tool_result" {
			if content, ok := block["content"].([]any); ok {
				replayCacheMarkers(content)
			}
		}
	}
}

func replayFrame(prefix hash.Hash, kind string, value any) error {
	raw, err := json.Marshal(value)
	if err != nil {
		return err
	}
	var size [8]byte
	binary.BigEndian.PutUint64(size[:], uint64(len(kind)))
	prefix.Write(size[:])
	prefix.Write([]byte(kind))
	binary.BigEndian.PutUint64(size[:], uint64(len(raw)))
	prefix.Write(size[:])
	prefix.Write(raw)
	return nil
}
