package web

import (
	"context"
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
	count := func(path ...string) uint64 { value, _ := jsonUint(jsonAt(usage, path...)); return value }
	pointer := func(value uint64) *uint64 { return &value }
	var result canonical.Usage
	switch api {
	case protocol.ConversationModelApiResponses:
		result.Input, result.Output = pointer(count("input_tokens")), pointer(count("output_tokens"))
		result.CacheRead, result.Reasoning = pointer(count("input_tokens_details", "cached_tokens")), pointer(count("output_tokens_details", "reasoning_tokens"))
	case protocol.ConversationModelApiAnthropic:
		read, write := count("cache_read_input_tokens"), count("cache_creation_input_tokens")
		result.Input, result.Output = pointer(nativeSaturatingAdd(nativeSaturatingAdd(count("input_tokens"), read), write)), pointer(count("output_tokens"))
		result.CacheRead, result.Reasoning = pointer(read), pointer(0)
		if write, ok := jsonUint(usage["cache_creation_input_tokens"]); ok {
			result.CacheWrite = pointer(write)
		}
	case protocol.ConversationModelApiGemini:
		reasoning := count("thoughtsTokenCount")
		result.Input, result.Output = pointer(count("promptTokenCount")), pointer(nativeSaturatingAdd(count("candidatesTokenCount"), reasoning))
		result.CacheRead, result.Reasoning = pointer(count("cachedContentTokenCount")), pointer(reasoning)
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
