package model

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"reflect"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"charm.land/fantasy"
	"github.com/egoist/mygo/netproxy"
	"github.com/openai/openai-go/v3/packages/ssestream"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
)

const UsageMetadataKey = "sailry.usage"
const responseMetadataKey = "sailry.response"

type responseMetadata struct{ Status *uint16 }

func (*responseMetadata) Options() {}

// UsageMetadata preserves reported field presence for one attempt. Scalar
// Fantasy usage is a convenience projection; these pointers are authoritative.
type UsageMetadata struct {
	Reported        bool            `json:"reported"`
	Precise         bool            `json:"precise"`
	Input           *int64          `json:"input"`
	Output          *int64          `json:"output"`
	Total           *int64          `json:"total"`
	Reasoning       *int64          `json:"reasoning"`
	CacheRead       *int64          `json:"cache_read"`
	CacheWrite      *int64          `json:"cache_write"`
	UncachedInput   *int64          `json:"uncached_input,omitempty"`
	CandidateOutput *int64          `json:"candidate_output,omitempty"`
	ToolInput       *int64          `json:"tool_input,omitempty"`
	Raw             json.RawMessage `json:"raw,omitempty"`
	// Context is what the last request occupied when the counts above sum
	// several requests of one reply.
	Context *int64 `json:"context,omitempty"`
}

func (*UsageMetadata) Options() {}
func (metadata UsageMetadata) MarshalJSON() ([]byte, error) {
	type plain UsageMetadata
	return json.Marshal(plain(metadata))
}
func (metadata *UsageMetadata) UnmarshalJSON(data []byte) error {
	type plain UsageMetadata
	return json.Unmarshal(data, (*plain)(metadata))
}

func (metadata UsageMetadata) scalar() fantasy.Usage {
	var result fantasy.Usage
	for _, item := range []struct {
		source *int64
		target *int64
	}{
		{metadata.Input, &result.InputTokens}, {metadata.Output, &result.OutputTokens},
		{metadata.Total, &result.TotalTokens}, {metadata.Reasoning, &result.ReasoningTokens},
		{metadata.CacheRead, &result.CacheReadTokens}, {metadata.CacheWrite, &result.CacheCreationTokens},
	} {
		if item.source != nil {
			*item.target = *item.source
		}
	}
	return result
}

type sdkModel struct {
	config transportConfig
	client *http.Client
	// Prefer HTTP/1.1 only after it completed a stream that this captured
	// model's HTTP/2 route reset.
	preferHTTP1 atomic.Bool
}
type responseFacts struct {
	status   int
	headers  http.Header
	body     []byte
	family   string
	observer *usageObserverBody
}
type factsKey struct{}

// maxFailureBody bounds the diagnostic body retained from a failed response.
const maxFailureBody = 1024 * 1024

type sourceBodyKey struct{}

type preparedRequestObservationKey struct{}

// preparedRequestObservation ties pre-encoded JSON to the SDK request that owns it.
// RoundTrip reuses it only for observation while that request's identity is intact.
type preparedRequestObservation struct {
	raw           []byte
	method        string
	target        url.URL
	body          io.ReadCloser
	contentLength int64
}

func withPreparedRequestObservation(request *http.Request, raw []byte) *http.Request {
	if request == nil || request.URL == nil {
		return request
	}
	prepared := preparedRequestObservation{
		raw:           raw,
		method:        request.Method,
		target:        *request.URL,
		body:          request.Body,
		contentLength: request.ContentLength,
	}
	return request.WithContext(context.WithValue(request.Context(), preparedRequestObservationKey{}, prepared))
}

func preparedRequestObservationBody(request *http.Request) ([]byte, bool) {
	prepared, ok := request.Context().Value(preparedRequestObservationKey{}).(preparedRequestObservation)
	if !ok || request.URL == nil || prepared.method != http.MethodPost || request.Method != prepared.method ||
		*request.URL != prepared.target || request.ContentLength != prepared.contentLength ||
		request.ContentLength != int64(len(prepared.raw)) || request.Body == nil || request.Body == http.NoBody ||
		!sameRequestBody(request.Body, prepared.body) {
		return nil, false
	}
	return prepared.raw, true
}

func sameRequestBody(left, right io.ReadCloser) bool {
	if left == nil || right == nil {
		return left == nil && right == nil
	}
	return reflect.ValueOf(left).Comparable() && reflect.ValueOf(right).Comparable() && left == right
}

// stallTimeout is how long a model response may send nothing, counted from
// dispatch until headers arrive and during each pending body read.
const stallTimeout = 300 * time.Second

type responseTransport struct {
	base  http.RoundTripper
	http1 http.RoundTripper
	stall time.Duration
}

// stallError ends a request whose response stayed silent. It is a network
// timeout, so a step that has shown nothing takes the normal retry path.
type stallError struct{ idle time.Duration }

func (stallError) Error() string   { return "model stream stalled" }
func (stallError) Timeout() bool   { return true }
func (stallError) Temporary() bool { return true }

// stallBody cancels its request when no data arrives within the limit.
type stallBody struct {
	io.ReadCloser
	limit   time.Duration
	timer   *time.Timer
	cancel  context.CancelFunc
	stalled *atomic.Bool
}

func (body *stallBody) Read(buffer []byte) (int, error) {
	body.timer.Reset(body.limit)
	count, err := body.ReadCloser.Read(buffer)
	body.timer.Stop()
	if body.stalled.Load() {
		return 0, stallError{idle: body.limit}
	}
	return count, err
}

func (body *stallBody) Close() error {
	body.timer.Stop()
	body.cancel()
	return body.ReadCloser.Close()
}

func (transport responseTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	if raw, ok := request.Context().Value(sourceBodyKey{}).([]byte); ok {
		if request.Body != nil {
			request.Body.Close()
		}
		request.Body = io.NopCloser(bytes.NewReader(raw))
		request.ContentLength = int64(len(raw))
		request.GetBody = func() (io.ReadCloser, error) { return io.NopCloser(bytes.NewReader(raw)), nil }
	}
	bodySize := request.ContentLength
	if raw, ok := preparedRequestObservationBody(request); ok {
		capturePrepared(request.Context(), raw, true)
		bodySize = int64(len(raw))
	} else if request.GetBody != nil {
		body, err := request.GetBody()
		if err != nil {
			return nil, err
		}
		raw, err := io.ReadAll(body)
		body.Close()
		if err != nil {
			return nil, err
		}
		capturePrepared(request.Context(), raw, true)
		bodySize = int64(len(raw))
	}
	owner, cancel := context.WithCancel(request.Context())
	stalled := &atomic.Bool{}
	timer := time.AfterFunc(transport.stall, func() { stalled.Store(true); cancel() })
	wire := transport.base
	if transport.http1 != nil && bodySize >= largeHTTP1BodyBytes && !largeHTTP1Skipped(owner) {
		wire = transport.http1
		markLargeHTTP1Used(owner)
	}
	captureDispatch(owner)
	response, err := wire.RoundTrip(request.WithContext(owner))
	// Receiving headers ends the dispatch wait. Time spent by the consumer
	// between reads is local work and does not indicate a silent peer.
	headersInTime := timer.Stop()
	if !headersInTime || err != nil || response == nil {
		cancel()
		if response != nil && response.Body != nil {
			response.Body.Close()
		}
		if !headersInTime {
			err = stallError{idle: transport.stall}
		}
		return nil, err
	}
	response.Body = &stallBody{ReadCloser: response.Body, limit: transport.stall, timer: timer, cancel: cancel, stalled: stalled}
	captureHeaders(request.Context(), response.StatusCode)
	if facts, ok := request.Context().Value(factsKey{}).(*responseFacts); ok {
		facts.status, facts.headers = response.StatusCode, response.Header.Clone()
		if response.StatusCode >= 400 {
			// A failure body is a bounded diagnostic; its read error is not the transport's.
			facts.body, _ = io.ReadAll(io.LimitReader(response.Body, maxFailureBody))
			response.Body.Close()
			response.Body = io.NopCloser(bytes.NewReader(facts.body))
		} else if (facts.family == "google" || facts.family == "anthropic") && strings.HasPrefix(strings.ToLower(response.Header.Get("Content-Type")), "text/event-stream") {
			facts.observer = observeUsage(response, facts.family)
			response.Body = facts.observer
		}
	}
	return response, nil
}

// modelTransport is the connection pool shared by model requests. A model
// stream can stay silent while the model thinks, and a NAT or gateway may drop
// an idle connection without notice; HTTP/2 health pings detect a dead
// connection instead of waiting for the stall timeout, and idle connections
// are retired before typical gateway idle windows close them.
var modelTransport = sync.OnceValue(func() *http.Transport {
	transport := netproxy.Transport().Clone()
	transport.IdleConnTimeout = 30 * time.Second
	transport.MaxIdleConnsPerHost = 16
	transport.HTTP2 = &http.HTTP2Config{SendPingTimeout: 15 * time.Second, PingTimeout: 10 * time.Second}
	return transport
})

func newSDK(_ context.Context, config transportConfig) (fantasy.LanguageModel, error) {
	// GenAI fills an empty key from the process environment even with a custom
	// endpoint. Only the captured provider's explicit credential may leave Node.
	if config.Kind == "google" && config.Key == "" {
		return nil, &Failure{Kind: NotConfigured, Message: "Gemini requires an explicitly configured API key"}
	}
	var transport http.RoundTripper = responseTransport{base: modelTransport(), http1: modelHTTP1Transport(), stall: stallTimeout}
	if config.Kind == "anthropic" && config.OAuth {
		transport = claudeTransport{base: transport}
	}
	if config.vertex != nil {
		transport = vertexTransport{base: transport, account: config.vertex}
	}
	return &sdkModel{config: config, client: &http.Client{Transport: transport, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}}, nil
}

func (model *sdkModel) Provider() string {
	if model.config.Kind == "responses" || model.config.Kind == "chat" {
		return "openai"
	}
	return model.config.Kind
}
func (model *sdkModel) Model() string { return model.config.Model }

func (model *sdkModel) Stream(ctx context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	body, err := requestJSON(model.config, call)
	if err != nil {
		return nil, err
	}
	// `/responses/compact` takes a few members of the body as they are; the
	// ordered main-request encoding would be discarded.
	compactWindow := model.config.Kind == "responses" && nativeRequest(call) != nil
	var raw []byte
	if !compactWindow {
		if raw, err = sourceRequestBytes(model.config, call, body); err != nil {
			return nil, err
		}
	}
	return func(yield func(fantasy.StreamPart) bool) {
		owner, cancel := context.WithCancel(ctx)
		defer cancel()
		facts := &responseFacts{family: model.config.Kind}
		defer func() {
			if facts.observer != nil {
				facts.observer.Close()
			}
		}()
		owner = context.WithValue(owner, factsKey{}, facts)
		emitter := &sdkEmitter{ctx: owner, config: model.config, facts: facts, yield: yield}
		switch model.config.Kind {
		case "anthropic":
			model.streamAnthropic(owner, raw, call, emitter)
		case "google":
			model.streamGoogle(context.WithValue(owner, sourceBodyKey{}, raw), body, call, emitter)
		case "responses":
			if compactWindow {
				model.compactResponses(owner, body, call, emitter)
				return
			}
			model.streamResponses(owner, raw, call, emitter)
		default:
			model.streamChat(owner, raw, call, emitter)
		}
	}, nil
}

func (model *sdkModel) Generate(ctx context.Context, call fantasy.Call) (*fantasy.Response, error) {
	stream, err := model.Stream(ctx, call)
	if err != nil {
		return nil, err
	}
	response := &fantasy.Response{}
	for part := range stream {
		switch part.Type {
		case fantasy.StreamPartTypeError:
			return nil, part.Error
		case fantasy.StreamPartTypeTextDelta:
			response.Content = append(response.Content, fantasy.TextContent{Text: part.Delta})
		case fantasy.StreamPartTypeReasoningDelta:
			response.Content = append(response.Content, fantasy.ReasoningContent{Text: part.Delta, ProviderMetadata: part.ProviderMetadata})
		case fantasy.StreamPartTypeToolCall:
			response.Content = append(response.Content, fantasy.ToolCallContent{ToolCallID: part.ID, ToolName: part.ToolCallName, Input: part.ToolCallInput, ProviderMetadata: part.ProviderMetadata})
		case fantasy.StreamPartTypeFinish:
			response.Usage, response.FinishReason, response.ProviderMetadata = part.Usage, part.FinishReason, part.ProviderMetadata
		}
	}
	return response, nil
}

func (*sdkModel) GenerateObject(context.Context, fantasy.ObjectCall) (*fantasy.ObjectResponse, error) {
	return nil, fmt.Errorf("structured generation is not configured")
}
func (*sdkModel) StreamObject(context.Context, fantasy.ObjectCall) (fantasy.ObjectStreamResponse, error) {
	return nil, fmt.Errorf("structured generation is not configured")
}

type sdkEmitter struct {
	ctx          context.Context
	config       transportConfig
	facts        *responseFacts
	usage        UsageMetadata
	yield        func(fantasy.StreamPart) bool
	indices      map[string]uint64
	nextIndex    uint64
	sources      canonical.Sources
	sourcesKnown bool
	response     *responseMetadata
}

func (emitter *sdkEmitter) emit(part fantasy.StreamPart) bool {
	if part.ProviderMetadata == nil {
		part.ProviderMetadata = fantasy.ProviderMetadata{}
	}
	var family string
	switch part.Type {
	case fantasy.StreamPartTypeTextStart, fantasy.StreamPartTypeTextDelta, fantasy.StreamPartTypeTextEnd:
		family = "text"
	case fantasy.StreamPartTypeReasoningStart, fantasy.StreamPartTypeReasoningDelta, fantasy.StreamPartTypeReasoningEnd:
		family = "reasoning"
	case fantasy.StreamPartTypeToolInputStart, fantasy.StreamPartTypeToolInputDelta, fantasy.StreamPartTypeToolInputEnd, fantasy.StreamPartTypeToolCall:
		family = "tool"
	}
	if family != "" {
		if _, explicit := part.ProviderMetadata[indexMetadataKey]; !explicit {
			if emitter.indices == nil {
				emitter.indices = map[string]uint64{}
			}
			key := family + ":" + part.ID
			index, known := emitter.indices[key]
			if !known {
				index = emitter.nextIndex
				emitter.indices[key] = index
				emitter.nextIndex++
			}
			part.ProviderMetadata[indexMetadataKey] = &indexMetadata{Index: index}
		}
	}
	terminal := part.Type == fantasy.StreamPartTypeFinish || part.Type == fantasy.StreamPartTypeError
	if observer := emitter.facts.observer; observer != nil {
		if terminal {
			observer.Close()
		}
		if terminal || emitter.config.Kind == "google" || !emitter.usage.Reported {
			emitter.usage = observer.snapshot()
		}
	}
	part.Usage = emitter.usage.scalar()
	if emitter.facts.status > 0 && emitter.facts.status <= 65535 {
		// The status is fixed once the response starts; its parts share it.
		if status := uint16(emitter.facts.status); emitter.response == nil || *emitter.response.Status != status {
			emitter.response = &responseMetadata{Status: &status}
		}
		part.ProviderMetadata[responseMetadataKey] = emitter.response
	}
	snapshot := emitter.usage
	part.ProviderMetadata[UsageMetadataKey] = &snapshot
	if emitter.sourcesKnown {
		part.ProviderMetadata[sourcesMetadataKey] = &sourcesMetadata{Sources: cloneSources(emitter.sources)}
	}
	return emitter.yield(part)
}

func (emitter *sdkEmitter) fail(cause error) {
	if emitter.ctx.Err() == nil && emitter.facts.status == 0 {
		if failure := certificateVerificationFailure(cause); failure != nil {
			emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: failure})
			return
		}
		// No HTTP response reached this transport, so a completion event was not yet expected.
		if errors.Is(cause, io.EOF) || errors.Is(cause, io.ErrUnexpectedEOF) {
			emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &Failure{Kind: Network, Message: "Model connection ended before response headers"}})
			return
		}
	}
	message := "model stream failed"
	if errors.Is(cause, context.Canceled) || errors.Is(emitter.ctx.Err(), context.Canceled) {
		message = "model request cancelled"
	} else if emitter.facts.status >= 400 {
		message = fmt.Sprintf("model request failed (HTTP %d)", emitter.facts.status)
	}
	headers := map[string]string{}
	for key, values := range emitter.facts.headers {
		if len(values) > 0 {
			headers[key] = values[0]
		}
	}
	if len(emitter.facts.body) == 0 {
		var raw interface{ RawJSON() string }
		if errors.As(cause, &raw) {
			emitter.facts.body = []byte(raw.RawJSON())
		}
	}
	if len(emitter.facts.body) == 0 {
		var streamError *ssestream.StreamError
		if errors.As(cause, &streamError) {
			emitter.facts.body = append([]byte(nil), streamError.Event.Data...)
		}
	}
	failure := &fantasy.ProviderError{Message: message, Cause: cause, StatusCode: emitter.facts.status, ResponseHeaders: headers, ResponseBody: emitter.facts.body}
	lower := strings.ToLower(string(emitter.facts.body))
	failure.ContextTooLargeErr = strings.Contains(lower, "context_length_exceeded") || strings.Contains(lower, "maximum context length")
	emitter.emit(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: withResponseHints(failure, emitter.facts.status, emitter.facts.headers)})
}

func decodeSDK(raw string) (map[string]any, error) {
	value := map[string]any{}
	decoder := json.NewDecoder(strings.NewReader(raw))
	decoder.UseNumber()
	err := decoder.Decode(&value)
	return value, err
}
func wireString(value any) string         { result, _ := value.(string); return result }
func wireObject(value any) map[string]any { result, _ := value.(map[string]any); return result }
func wireArray(value any) []any           { result, _ := value.([]any); return result }
func wireInt(value any) *int64 {
	switch value := value.(type) {
	case json.Number:
		if number, err := value.Int64(); err == nil {
			return &number
		}
	case float64:
		number := int64(value)
		return &number
	}
	return nil
}

func (emitter *sdkEmitter) updateUsage(value map[string]any, family string) {
	if value == nil {
		return
	}
	emitter.usage.Reported = true
	emitter.usage.Precise = true
	merged := map[string]any{}
	if len(emitter.usage.Raw) > 0 {
		decoder := json.NewDecoder(bytes.NewReader(emitter.usage.Raw))
		decoder.UseNumber()
		decoder.Decode(&merged)
	}
	mergeUsageFields(merged, value)
	if iterations := wireArray(merged["iterations"]); family == "anthropic" && len(iterations) > 0 {
		// The cache write breakdown follows the iteration totals below, so
		// that pricing can tell five-minute from one-hour writes.
		creation := map[string]any{}
		for _, key := range []string{"ephemeral_5m_input_tokens", "ephemeral_1h_input_tokens"} {
			var sum int64
			found := false
			for _, iteration := range iterations {
				if count := wireInt(wireObject(wireObject(iteration)["cache_creation"])[key]); count != nil {
					sum, found = sum+*count, true
				}
			}
			if found {
				creation[key] = json.Number(fmt.Sprint(sum))
			}
		}
		if len(creation) > 0 {
			merged["cache_creation"] = creation
		}
	}
	raw, _ := json.Marshal(merged)
	emitter.usage.Raw = raw
	keys := map[string]**int64{}
	switch family {
	case "anthropic":
		keys = map[string]**int64{"input_tokens": &emitter.usage.UncachedInput, "output_tokens": &emitter.usage.Output, "cache_read_input_tokens": &emitter.usage.CacheRead, "cache_creation_input_tokens": &emitter.usage.CacheWrite}
	case "google":
		keys = map[string]**int64{"promptTokenCount": &emitter.usage.UncachedInput, "toolUsePromptTokenCount": &emitter.usage.ToolInput, "candidatesTokenCount": &emitter.usage.CandidateOutput, "totalTokenCount": &emitter.usage.Total, "cachedContentTokenCount": &emitter.usage.CacheRead, "thoughtsTokenCount": &emitter.usage.Reasoning}
	case "chat":
		keys = map[string]**int64{"prompt_tokens": &emitter.usage.Input, "completion_tokens": &emitter.usage.Output, "total_tokens": &emitter.usage.Total}
		details := wireObject(value["prompt_tokens_details"])
		if count := wireInt(details["cached_tokens"]); count != nil {
			emitter.usage.CacheRead = count
		} else if count := wireInt(value["prompt_cache_hit_tokens"]); count != nil {
			emitter.usage.CacheRead = count
		}
		if count := wireInt(details["cache_write_tokens"]); count != nil {
			emitter.usage.CacheWrite = count
		}
		if count := wireInt(wireObject(value["completion_tokens_details"])["reasoning_tokens"]); count != nil {
			emitter.usage.Reasoning = count
		}
	default:
		keys = map[string]**int64{"input_tokens": &emitter.usage.Input, "output_tokens": &emitter.usage.Output, "total_tokens": &emitter.usage.Total}
		details := wireObject(value["input_tokens_details"])
		if count := wireInt(details["cached_tokens"]); count != nil {
			emitter.usage.CacheRead = count
		}
		if count := wireInt(details["cache_write_tokens"]); count != nil {
			emitter.usage.CacheWrite = count
		}
		if count := wireInt(wireObject(value["output_tokens_details"])["reasoning_tokens"]); count != nil {
			emitter.usage.Reasoning = count
		}
	}
	counts := value
	if iterations := wireArray(merged["iterations"]); family == "anthropic" && len(iterations) > 0 {
		// A request that compacts reports zero at the top level and its billed
		// passes, the compaction among them, as iterations.
		counts = map[string]any{}
		for key := range keys {
			var sum int64
			found := false
			for _, iteration := range iterations {
				if count := wireInt(wireObject(iteration)[key]); count != nil {
					sum, found = sum+*count, true
				}
			}
			if found {
				counts[key] = json.Number(fmt.Sprint(sum))
			}
		}
	}
	for key, target := range keys {
		if count := wireInt(counts[key]); count != nil {
			*target = count
		}
	}
	if family == "anthropic" && emitter.usage.UncachedInput != nil {
		input := *emitter.usage.UncachedInput
		if emitter.usage.CacheRead != nil {
			input += *emitter.usage.CacheRead
		}
		if emitter.usage.CacheWrite != nil {
			input += *emitter.usage.CacheWrite
		}
		emitter.usage.Input = &input
		if emitter.usage.Output != nil {
			total := input + *emitter.usage.Output
			emitter.usage.Total = &total
		}
	}
	if family == "google" {
		if emitter.usage.UncachedInput != nil {
			input := *emitter.usage.UncachedInput
			if emitter.usage.ToolInput != nil {
				input += *emitter.usage.ToolInput
			}
			emitter.usage.Input = &input
		}
		if emitter.usage.CandidateOutput != nil {
			output := *emitter.usage.CandidateOutput
			if emitter.usage.Reasoning != nil {
				output += *emitter.usage.Reasoning
			}
			emitter.usage.Output = &output
		}
	}
}

// Usage deltas may report only one field of a nested cache or token breakdown.
func mergeUsageFields(previous, current map[string]any) {
	for key, value := range current {
		old, next := wireObject(previous[key]), wireObject(value)
		if old != nil && next != nil {
			mergeUsageFields(old, next)
			continue
		}
		previous[key] = value
	}
}

func (emitter *sdkEmitter) emitIndexed(part fantasy.StreamPart, index int64) bool {
	if part.ProviderMetadata == nil {
		part.ProviderMetadata = fantasy.ProviderMetadata{}
	}
	if index >= 0 {
		part.ProviderMetadata[indexMetadataKey] = &indexMetadata{Index: uint64(index)}
	}
	return emitter.emit(part)
}
