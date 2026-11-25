package model

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/clock"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Protocol-native compaction is a method of the route the model factory
// constructed: Anthropic Messages summarizes on demand with the `compaction`
// parameter, OpenAI Responses compacts through `/responses/compact`, and other
// transports have none. The engine decides when to compact and falls back to
// its own handoff when the route has no native method or it fails.

// ErrNativeUnsupported reports that the provider rejected the protocol
// feature itself: its endpoint, parameter or beta flag is unknown there.
var ErrNativeUnsupported = errors.New("native compaction is unsupported by this route")

// ErrNativeUnusable reports a completed native request without a usable
// result, such as a refused or empty summary.
var ErrNativeUnusable = errors.New("native compaction returned no usable result")

// anthropicCompactBeta enables on-demand compaction and the signed blocks it
// returns; every request that carries such a block repeats it.
const anthropicCompactBeta = "compact-2026-09-04"

const nativeCompactionKey = "sailry.native_compaction"

// nativeCompaction asks the transport to compact the call's history instead
// of answering it.
type nativeCompaction struct{}

func (*nativeCompaction) Options() {}

// nativeOutput is the opaque result a transport reports on its finish part.
type nativeOutput struct {
	Summary string
	Items   json.RawMessage
	// Ignored reports a completed response with no compaction item at all:
	// the provider or model answered without the feature, as a gateway that
	// drops the beta or a model without compaction does. A blocked summary
	// still returns its item and is not Ignored.
	Ignored bool `json:",omitempty"`
}

func (*nativeOutput) Options() {}

// nativeReplay marks a prompt message whose content is a stored native
// checkpoint; the request encoder writes its items unchanged.
type nativeReplay struct{ Items json.RawMessage }

func (*nativeReplay) Options() {}

func (value nativeCompaction) MarshalJSON() ([]byte, error) {
	type plain nativeCompaction
	return json.Marshal(plain(value))
}
func (value *nativeCompaction) UnmarshalJSON(data []byte) error {
	type plain nativeCompaction
	return json.Unmarshal(data, (*plain)(value))
}
func (value nativeOutput) MarshalJSON() ([]byte, error) {
	type plain nativeOutput
	return json.Marshal(plain(value))
}
func (value *nativeOutput) UnmarshalJSON(data []byte) error {
	type plain nativeOutput
	return json.Unmarshal(data, (*plain)(value))
}
func (value nativeReplay) MarshalJSON() ([]byte, error) {
	type plain nativeReplay
	return json.Marshal(plain(value))
}
func (value *nativeReplay) UnmarshalJSON(data []byte) error {
	type plain nativeReplay
	return json.Unmarshal(data, (*plain)(value))
}

// Compacted is the outcome of a native compaction. Summary is the provider's
// readable text, empty for an opaque window.
type Compacted struct {
	Summary string
	Native  canonical.NativeCompaction
}

// NativeCompactionRoute names the protocol, endpoint and model whose native
// compaction a rejection disables, or "" when the route offers none. Package
// translations and agent programs have no native compaction here.
func NativeCompactionRoute(languageModel fantasy.LanguageModel) string {
	model, ok := languageModel.(*sdkModel)
	if !ok || model.config.Kind != "anthropic" && model.config.Kind != "responses" {
		return ""
	}
	return strings.Join([]string{model.config.Kind, model.config.Endpoint, model.config.Model}, "\x00")
}

// CompactNative sends call, the main request over the history to replace,
// as the route's native compaction with the provider's own summary prompt. It
// follows Run's retry rules and reports each attempt to observe.
func CompactNative(ctx context.Context, languageModel fantasy.LanguageModel, binding Binding, call fantasy.Call, timer clock.Clock, observe func(Attempt) error) (Compacted, error) {
	if NativeCompactionRoute(languageModel) == "" {
		return Compacted{}, ErrNativeUnsupported
	}
	options := fantasy.ProviderOptions{}
	for key, value := range call.ProviderOptions {
		options[key] = value
	}
	options[nativeCompactionKey] = &nativeCompaction{}
	call.ProviderOptions = options
	var output *nativeOutput
	err := Run(ctx, languageModel, call, NativePurpose, timer, observe, func(part fantasy.StreamPart) bool {
		if part.Type == fantasy.StreamPartTypeFinish {
			output, _ = part.ProviderMetadata[nativeCompactionKey].(*nativeOutput)
		}
		return true
	}, nil)
	if err != nil {
		if nativeRejected(err) {
			return Compacted{}, fmt.Errorf("%w: %w", ErrNativeUnsupported, err)
		}
		if failure := Classify(err); failure.nativeFallback() {
			// The quick resend is spent, not the ordinary recovery: Sailry's
			// summary still has its own.
			failure.Exhausted, failure.WaitExhausted = false, false
			return Compacted{}, failure
		}
		return Compacted{}, err
	}
	if output != nil && output.Ignored {
		// Each later attempt would pay for an ordinary reply again.
		return Compacted{}, ErrNativeUnsupported
	}
	if output == nil || len(output.Items) == 0 {
		return Compacted{}, ErrNativeUnusable
	}
	return Compacted{Summary: output.Summary, Native: canonical.NativeCompaction{Source: Scope(binding), Items: output.Items}}, nil
}

// nativeRejected recognizes a provider that does not know the feature: a
// missing endpoint, or a rejected parameter, content type or beta flag.
// Other rejections, such as a history the summarizer cannot take, leave the
// route's support unchanged.
func nativeRejected(err error) bool {
	failure := Classify(err)
	if failure.status == nil {
		return false
	}
	switch *failure.status {
	case 404, 405:
		return true
	case 400, 422:
		message := strings.ToLower(failure.Message)
		for _, marker := range []string{"beta", "unknown", "unrecognized", "extra inputs", "not permitted", "unsupported", "not supported", "does not support", "not available", "invalid field", "unexpected field"} {
			if strings.Contains(message, marker) {
				return true
			}
		}
	}
	return false
}

// nativeMessage projects a stored native checkpoint for the provider,
// protocol and model that produced it. Any other route reads the summary.
func nativeMessage(binding Binding, record canonical.Record) (fantasy.Message, bool) {
	var compaction *canonical.Compaction
	switch value := record.Data.(type) {
	case *canonical.Compaction:
		compaction = value
	case canonical.Compaction:
		compaction = &value
	}
	if record.Kind != "compaction" || compaction == nil || compaction.Native == nil || PluginBound(binding.Provider) {
		return fantasy.Message{}, false
	}
	source := compaction.Native.Source
	if source.ID != binding.Provider.ID || source.API != binding.Provider.Api || source.Model != binding.Config.Model {
		return fantasy.Message{}, false
	}
	// A compaction block opens the history as an assistant message; a
	// Responses window is a list of input items.
	role := fantasy.MessageRoleUser
	if family := source.API.Family(); family == protocol.ConversationModelApiAnthropic || family == protocol.ConversationModelApiAzureAi {
		role = fantasy.MessageRoleAssistant
	}
	return fantasy.Message{Role: role, Content: []fantasy.MessagePart{fantasy.TextPart{ProviderOptions: fantasy.ProviderOptions{nativeCompactionKey: &nativeReplay{Items: compaction.Native.Items}}}}}, true
}

// ReplaysNative reports whether a request for binding sends the checkpoint
// record's native items instead of its summary.
func ReplaysNative(binding Binding, record canonical.Record) bool {
	_, ok := nativeMessage(binding, record)
	return ok
}

func nativeItems(part fantasy.MessagePart) ([]any, bool) {
	replay, ok := part.Options()[nativeCompactionKey].(*nativeReplay)
	if !ok {
		return nil, false
	}
	var items []any
	decoder := json.NewDecoder(strings.NewReader(string(replay.Items)))
	decoder.UseNumber()
	if decoder.Decode(&items) != nil {
		return nil, false
	}
	return items, true
}

func nativeRequest(call fantasy.Call) *nativeCompaction {
	request, _ := call.ProviderOptions[nativeCompactionKey].(*nativeCompaction)
	return request
}
