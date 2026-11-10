package model

import (
	"encoding/json"
	"fmt"
	"math"
	"slices"
	"strings"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const pluginMetadataKey = "sailry.plugin.output"

type pluginOutputMetadata struct {
	Replay    *string
	ToolError *canonical.ToolError
	Output    bool
}

func (*pluginOutputMetadata) Options() {}

func (value pluginOutputMetadata) MarshalJSON() ([]byte, error) {
	type plain pluginOutputMetadata
	return json.Marshal(plain(value))
}

func (value *pluginOutputMetadata) UnmarshalJSON(raw []byte) error {
	type plain pluginOutputMetadata
	return json.Unmarshal(raw, (*plain)(value))
}

func ApplyPluginMetadata(block *canonical.Block, part fantasy.StreamPart) error {
	metadata, ok := part.ProviderMetadata[pluginMetadataKey].(*pluginOutputMetadata)
	if !ok || metadata == nil {
		return nil
	}
	if metadata.Replay != nil {
		if block.Kind != "thinking" {
			return &Failure{Kind: InvalidResponse, Message: "model reasoning metadata has another content kind"}
		}
		thinking, ok := block.Data.(*canonical.Thinking)
		if !ok {
			return &Failure{Kind: InvalidResponse, Message: "model reasoning metadata is invalid"}
		}
		replay := *metadata.Replay
		thinking.Signature = &replay
	}
	if metadata.ToolError != nil {
		call, ok := block.Data.(*canonical.ToolRequest)
		if !ok {
			return &Failure{Kind: InvalidResponse, Message: "model tool metadata has another content kind"}
		}
		value := *metadata.ToolError
		call.Error = &value
	}
	return nil
}

func PluginOutput(part fantasy.StreamPart) bool {
	metadata, ok := part.ProviderMetadata[pluginMetadataKey].(*pluginOutputMetadata)
	return ok && metadata != nil && metadata.Output
}

type pluginCall struct {
	index     uint64
	id        string
	name      string
	arguments strings.Builder
}

type pluginAccumulator struct {
	calls     map[uint64]*pluginCall
	usage     UsageMetadata
	reason    protocol.PluginModelAdapterWireFinish
	finished  bool
	index     uint64
	content   string
	signature strings.Builder
	secrets   []protocol.Secret
}

func (accumulator *pluginAccumulator) part(kind string) uint64 {
	if accumulator.content != kind {
		accumulator.index++
		accumulator.content = kind
	}
	return accumulator.index
}

func pluginIndexed(kind fantasy.StreamPartType, index uint64) fantasy.StreamPart {
	return fantasy.StreamPart{Type: kind, ID: fmt.Sprintf("plugin-%d", index), ProviderMetadata: fantasy.ProviderMetadata{indexMetadataKey: &indexMetadata{Index: index}}}
}

func (accumulator *pluginAccumulator) flush(parts *[]fantasy.StreamPart) {
	if accumulator.signature.Len() != 0 {
		replay := accumulator.signature.String()
		accumulator.signature.Reset()
		part := pluginIndexed(fantasy.StreamPartTypeReasoningEnd, accumulator.index)
		part.ProviderMetadata[pluginMetadataKey] = &pluginOutputMetadata{Replay: &replay, Output: true}
		*parts = append(*parts, part)
	}
}

func (accumulator *pluginAccumulator) push(deltas []protocol.PluginModelAdapterWireDelta) ([]fantasy.StreamPart, error) {
	parts := []fantasy.StreamPart{}
	for _, delta := range deltas {
		switch value := delta.Data.(type) {
		case *protocol.PluginModelAdapterWireDeltaTextPayload:
			accumulator.flush(&parts)
			if value.Text != "" {
				part := pluginIndexed(fantasy.StreamPartTypeTextDelta, accumulator.part("text"))
				part.Delta = value.Text
				parts = append(parts, part)
			}
		case *protocol.PluginModelAdapterWireDeltaReasoningPayload:
			index := accumulator.part("reasoning")
			if value.Replay != nil {
				accumulator.signature.WriteString(*value.Replay)
			}
			if value.Text != nil || value.Replay != nil && *value.Replay != "" {
				part := pluginIndexed(fantasy.StreamPartTypeReasoningDelta, index)
				if value.Text != nil {
					part.Delta = *value.Text
				}
				part.ProviderMetadata[pluginMetadataKey] = &pluginOutputMetadata{Output: value.Text != nil && *value.Text != "" || value.Replay != nil && *value.Replay != ""}
				parts = append(parts, part)
			}
		case *protocol.PluginModelAdapterWireDeltaToolCallPayload:
			accumulator.flush(&parts)
			if accumulator.calls == nil {
				accumulator.calls = map[uint64]*pluginCall{}
			}
			call := accumulator.calls[value.Index]
			if call == nil {
				accumulator.index++
				accumulator.content = ""
				call = &pluginCall{index: accumulator.index}
				accumulator.calls[value.Index] = call
			}
			if value.ID != nil {
				call.id = *value.ID
			}
			if value.Name != nil {
				call.name += *value.Name
			}
			if value.ArgumentsDelta != nil {
				call.arguments.WriteString(*value.ArgumentsDelta)
			}
			part := pluginIndexed(fantasy.StreamPartTypeKeepalive, call.index)
			part.ToolCallName = call.name
			part.ProviderMetadata[pluginMetadataKey] = &pluginOutputMetadata{Output: true}
			if value.ArgumentsDelta != nil {
				part.Delta = *value.ArgumentsDelta
			}
			parts = append(parts, part)
		case *protocol.PluginModelAdapterWireDeltaUsagePayload:
			accumulator.usage.Reported, accumulator.usage.Precise = true, true
			for _, field := range []struct {
				from *uint64
				to   **int64
			}{{value.InputTokens, &accumulator.usage.Input}, {value.OutputTokens, &accumulator.usage.Output}, {value.CachedInputTokens, &accumulator.usage.CacheRead}, {value.ReasoningTokens, &accumulator.usage.Reasoning}} {
				if field.from != nil {
					if *field.from > math.MaxInt64 {
						return nil, &Failure{Kind: InvalidResponse, Message: "model usage exceeds the supported counter range"}
					}
					number := int64(*field.from)
					*field.to = &number
				}
			}
		case *protocol.PluginModelAdapterWireDeltaFinishPayload:
			accumulator.flush(&parts)
			accumulator.reason, accumulator.finished = value.Reason, true
		case *protocol.PluginModelAdapterWireDeltaErrorPayload:
			return nil, pluginClassified(value.Class, value.Message, value.RetryAfterMs, accumulator.secrets)
		}
	}
	return parts, nil
}

func (accumulator *pluginAccumulator) finish() ([]fantasy.StreamPart, error) {
	if !accumulator.finished {
		return nil, &Failure{Kind: InvalidResponse, Message: "model stream ended before completion"}
	}
	parts := []fantasy.StreamPart{}
	keys := []uint64{}
	for key := range accumulator.calls {
		keys = append(keys, key)
	}
	slices.Sort(keys)
	for _, key := range keys {
		call := accumulator.calls[key]
		if call.id == "" {
			return nil, &Failure{Kind: InvalidResponse, Message: "model tool call identifier is missing"}
		}
		part := pluginIndexed(fantasy.StreamPartTypeToolCall, call.index)
		part.ID, part.ToolCallName, part.ToolCallInput = call.id, call.name, call.arguments.String()
		var arguments map[string]json.RawMessage
		if json.Unmarshal([]byte(part.ToolCallInput), &arguments) != nil || arguments == nil {
			part.ToolCallInput = "null"
			part.ProviderMetadata[pluginMetadataKey] = &pluginOutputMetadata{ToolError: &canonical.ToolError{Code: -32602, Message: "tool arguments must be a JSON object"}}
		}
		parts = append(parts, part)
	}
	if accumulator.usage.Input != nil && accumulator.usage.Output != nil {
		if *accumulator.usage.Input > math.MaxInt64-*accumulator.usage.Output {
			return nil, &Failure{Kind: InvalidResponse, Message: "model usage exceeds the supported counter range"}
		}
		total := *accumulator.usage.Input + *accumulator.usage.Output
		accumulator.usage.Total = &total
	}
	accumulator.usage.Reported, accumulator.usage.Precise = true, true
	reason := fantasy.FinishReasonOther
	switch accumulator.reason {
	case protocol.PluginModelAdapterWireFinishStop:
		reason = fantasy.FinishReasonStop
	case protocol.PluginModelAdapterWireFinishToolCalls:
		reason = fantasy.FinishReasonToolCalls
	case protocol.PluginModelAdapterWireFinishLength:
		reason = fantasy.FinishReasonLength
	case protocol.PluginModelAdapterWireFinishContentFilter:
		reason = fantasy.FinishReasonContentFilter
	}
	usage := accumulator.usage
	parts = append(parts, fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: reason, Usage: usage.scalar(),
		ProviderMetadata: fantasy.ProviderMetadata{UsageMetadataKey: &usage}})
	return parts, nil
}

func pluginClassified(class protocol.PluginModelAdapterWireClass, message string, retryMilliseconds *uint64, secrets []protocol.Secret) *Failure {
	kind := Unavailable
	switch class {
	case protocol.PluginModelAdapterWireClassAuthentication, protocol.PluginModelAdapterWireClassPermission:
		kind = Authentication
	case protocol.PluginModelAdapterWireClassRateLimited:
		kind = RateLimited
	case protocol.PluginModelAdapterWireClassQuota:
		kind = Quota
	case protocol.PluginModelAdapterWireClassContextOverflow:
		kind = ContextOverflow
	case protocol.PluginModelAdapterWireClassInvalidRequest:
		kind = InvalidRequest
	case protocol.PluginModelAdapterWireClassUnsupported:
		kind = Unsupported
	}
	failure := &Failure{Kind: kind, Message: safeDiagnostic(message, secrets)}
	if retryMilliseconds != nil && *retryMilliseconds <= uint64(math.MaxInt64/int64(time.Millisecond)) {
		delay := time.Duration(*retryMilliseconds) * time.Millisecond
		failure.RetryAfter = &delay
	}
	return failure
}
