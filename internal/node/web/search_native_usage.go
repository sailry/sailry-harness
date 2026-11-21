package web

import (
	"context"
	"encoding/json"
	"math"
	"net/http"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func nativeAnthropicExecute(ctx, caller context.Context, search NativeSearch, client *http.Client, body map[string]any) (protocol.SearchPage, error) {
	var content []any
	for continuation := 0; continuation <= 3; continuation++ {
		value, err := nativeSend(ctx, caller, search, client, body)
		if err != nil {
			return protocol.SearchPage{}, err
		}
		if err := nativeAnthropicCheck(value, search.Now()); err != nil {
			return protocol.SearchPage{}, err
		}
		if jsonString(value["stop_reason"]) == "pause_turn" {
			if continuation == 3 {
				return protocol.SearchPage{}, nativeInvalid("model search continuation limit reached")
			}
			blocks, ok := value["content"].([]any)
			if !ok {
				return protocol.SearchPage{}, nativeInvalid("paused model search has no assistant content")
			}
			content = append(content, blocks...)
			messages, ok := body["messages"].([]any)
			if !ok {
				return protocol.SearchPage{}, nativeInvalid("model search messages are invalid")
			}
			// Continue with the original assistant blocks, including opaque provider
			// data. They are transport state and never become source excerpts.
			body["messages"] = append(messages, map[string]any{"role": "assistant", "content": blocks})
			continue
		}
		if len(content) > 0 {
			blocks, ok := value["content"].([]any)
			if !ok {
				return protocol.SearchPage{}, nativeInvalid("model search response has no content")
			}
			value["content"] = append(content, blocks...)
		}
		return nativeAnthropicParse(value, search.Now())
	}
	return protocol.SearchPage{}, nativeInvalid("model search continuation limit reached")
}

func nativeUsage(api protocol.ConversationModelApi, value map[string]any) *canonical.Usage {
	key := "usage"
	if api == protocol.ConversationModelApiGemini {
		key = "usageMetadata"
	}
	usage, ok := value[key].(map[string]any)
	if !ok {
		return nil
	}
	count := func(path ...string) *uint64 {
		value, ok := jsonUint(jsonAt(usage, path...))
		if !ok {
			return nil
		}
		return &value
	}
	sum := func(base *uint64, parts ...*uint64) *uint64 {
		if base == nil {
			return nil
		}
		total := *base
		for _, part := range parts {
			if part != nil {
				total = nativeSaturatingAdd(total, *part)
			}
		}
		return &total
	}
	var result canonical.Usage
	switch api {
	case protocol.ConversationModelApiResponses:
		result.Input, result.Output = count("input_tokens"), count("output_tokens")
		result.CacheRead = count("input_tokens_details", "cached_tokens")
		result.CacheWrite = count("input_tokens_details", "cache_write_tokens")
		result.Reasoning = count("output_tokens_details", "reasoning_tokens")
	case protocol.ConversationModelApiAnthropic:
		result.CacheRead, result.CacheWrite = count("cache_read_input_tokens"), count("cache_creation_input_tokens")
		result.Input, result.Output = sum(count("input_tokens"), result.CacheRead, result.CacheWrite), count("output_tokens")
		if creation, present := usage["cache_creation"]; present {
			result.Provider, _ = json.Marshal(map[string]any{"cache_creation": creation})
		}
	case protocol.ConversationModelApiGemini:
		result.CacheRead, result.Reasoning = count("cachedContentTokenCount"), count("thoughtsTokenCount")
		result.Input, result.Output = count("promptTokenCount"), sum(count("candidatesTokenCount"), result.Reasoning)
	default:
		return nil
	}
	return &result
}

func nativeSaturatingAdd(first, second uint64) uint64 {
	if second > math.MaxUint64-first {
		return math.MaxUint64
	}
	return first + second
}
