package model

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"maps"
	"net/http"
	"slices"
	"strings"
	"unicode/utf8"

	"charm.land/fantasy"
	"github.com/egoist/mygo/netproxy"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const pluginRequestBytes = 32 * 1024 * 1024
const pluginResponseBytes = 64 * 1024 * 1024
const pluginDiscoveryBytes = 8 * 1024 * 1024

type PluginCapture struct {
	Declaration protocol.PluginModelAdapterDeclaration
	Bundle      plugins.ScriptBundle
	Headers     http.Header
	Access      *protocol.Secret
	Key         *protocol.Secret
	Secrets     []protocol.Secret
	Client      *http.Client
	// Settings are the package's current public setting values, which its
	// prepare and discovery exports receive.
	Settings map[string]json.RawMessage
}

type pluginModel struct {
	binding Binding
	capture PluginCapture
	client  *http.Client
}

func PluginBound(provider protocol.ConversationProvider) bool {
	return provider.Binding != nil && *provider.Binding != nil && (**provider.Binding).Adapter != nil
}

func NewPlugin(_ context.Context, binding Binding, capture PluginCapture) (fantasy.LanguageModel, error) {
	if !PluginBound(binding.Provider) || (**binding.Provider.Binding).Package == nil || (**binding.Provider.Binding).Adapter.Adapter != capture.Declaration.ID {
		return nil, &Failure{Kind: NotConfigured, Message: "captured model adapter is unavailable"}
	}
	raw, err := json.Marshal(capture.Declaration)
	if err != nil {
		return nil, &Failure{Kind: InvalidRequest, Message: "invalid model adapter declaration"}
	}
	if json.Unmarshal(raw, &capture.Declaration) != nil {
		return nil, &Failure{Kind: InvalidRequest, Message: "invalid model adapter declaration"}
	}
	capture.Bundle.Files = maps.Clone(capture.Bundle.Files)
	capture.Headers = capture.Headers.Clone()
	if capture.Headers == nil {
		capture.Headers = http.Header{}
	}
	capture.Secrets = slices.Clone(capture.Secrets)
	capture.Settings = maps.Clone(capture.Settings)
	if capture.Access != nil {
		access := protocol.NewSecret(capture.Access.Expose())
		capture.Access = &access
		capture.Secrets = append(capture.Secrets, access)
	}
	if capture.Key == nil && binding.Provider.Authentication == protocol.AuthenticationApiKey {
		capture.Key = binding.Secret
	}
	if capture.Key != nil {
		key := protocol.NewSecret(capture.Key.Expose())
		capture.Key = &key
		capture.Secrets = append(capture.Secrets, key)
		header, template := "authorization", "Bearer {key}"
		if injection := (**binding.Provider.Binding).KeyInjection; injection != nil {
			header, template = injection.Header, injection.Template
		}
		if !validPluginHeader(header, template) {
			return nil, &Failure{Kind: InvalidRequest, Message: "invalid model credential header"}
		}
		capture.Headers.Set(header, strings.ReplaceAll(template, "{key}", key.Expose()))
	}
	client := capture.Client
	if client == nil {
		client = &http.Client{Transport: netproxy.Transport()}
	}
	copy := *client
	copy.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	return &pluginModel{binding: binding, capture: capture, client: &copy}, nil
}

func (model *pluginModel) Provider() string { return "sailry-plugin" }
func (model *pluginModel) Model() string    { return model.binding.Config.Model }

func (model *pluginModel) Stream(ctx context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	input, err := model.request(call)
	if err != nil {
		return nil, err
	}
	return func(yield func(fantasy.StreamPart) bool) {
		owner, cancel := context.WithCancel(ctx)
		defer cancel()
		// A consumer that stopped the iteration must not be yielded to again.
		stopped := false
		emit := func(part fantasy.StreamPart) bool {
			stopped = !yield(part)
			return !stopped
		}
		if err := model.stream(owner, input, emit); stopped {
			return
		} else if err != nil && ctx.Err() == nil {
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: Classify(err, model.capture.Secrets...)})
		} else if ctx.Err() != nil {
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &Failure{Kind: Cancelled, Message: "model request was cancelled"}})
		}
	}, nil
}

func (model *pluginModel) stream(ctx context.Context, input protocol.PluginModelAdapterWireRequest, yield func(fantasy.StreamPart) bool) error {
	worker, err := plugins.NewTranslation(ctx, model.capture.Bundle)
	if err != nil {
		return pluginTranslationFailure(err)
	}
	defer worker.Close()
	input.Settings = model.capture.Settings
	raw, err := worker.Call("prepare", input, pluginRequestBytes)
	if err != nil {
		return pluginTranslationFailure(err)
	}
	// A translation refuses a request it must not send: nothing leaves the
	// Node and the turn fails with the translation's own class and message.
	var refused struct {
		Refusal *protocol.PluginModelAdapterWireDeltaErrorPayload `json:"refusal"`
	}
	if json.Unmarshal(raw, &refused) == nil && refused.Refusal != nil {
		return pluginClassified(refused.Refusal.Class, refused.Refusal.Message, model.capture.Secrets)
	}
	var prepared protocol.PluginModelAdapterWirePrepared
	if json.Unmarshal(raw, &prepared) != nil {
		return &Failure{Kind: InvalidRequest, Message: "model translation returned an invalid request"}
	}
	framing := model.capture.Declaration.Framing
	if prepared.Framing != nil {
		framing = *prepared.Framing
	}
	if prepared.Body != nil {
		capturePrepared(ctx, []byte(*prepared.Body))
	}
	response, err := model.send(ctx, prepared, pluginRequestBytes)
	if err != nil {
		return err
	}
	defer response.Body.Close()
	captureHeaders(ctx, response.StatusCode)
	headers, err := model.publicHeaders(response.Header)
	if err != nil {
		return err
	}
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return model.classify(ctx, response, headers, worker)
	}
	decoder := pluginDecoder{framing: framing, limit: pluginResponseBytes, first: true}
	accumulator := pluginAccumulator{secrets: model.capture.Secrets}
	state := json.RawMessage("null")
	chunk := make([]byte, 32*1024)
	status := uint16(response.StatusCode)
	for {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		count, readErr := response.Body.Read(chunk)
		done := errors.Is(readErr, io.EOF)
		if readErr != nil && !done {
			return &Failure{Kind: Network, Message: "model connection failed"}
		}
		events, err := decoder.push(chunk[:count], done)
		if err != nil {
			return err
		}
		for _, event := range events {
			if err := model.public(event.Data); err != nil {
				return err
			}
		}
		if len(events) == 0 && !done {
			continue
		}
		raw, err := worker.Call("decode", protocol.PluginModelAdapterWireDecode{State: state, Events: events, Done: done, Status: &status, Headers: &headers}, pluginResponseBytes)
		if err != nil {
			return pluginTranslationFailure(err)
		}
		var decoded protocol.PluginModelAdapterWireDecoded
		if json.Unmarshal(raw, &decoded) != nil {
			return &Failure{Kind: InvalidRequest, Message: "model translation returned an invalid response"}
		}
		state = decoded.State
		parts, err := accumulator.push(decoded.Deltas)
		if err != nil {
			return err
		}
		for _, part := range parts {
			if !yield(part) {
				return nil
			}
		}
		if done || accumulator.finished {
			parts, err := accumulator.finish()
			if err != nil {
				return err
			}
			for _, part := range parts {
				if !yield(part) {
					return nil
				}
			}
			return nil
		}
	}
}

func (model *pluginModel) Generate(ctx context.Context, call fantasy.Call) (*fantasy.Response, error) {
	return model.generate(ctx, model.Stream, call)
}

// generate collects one stream of adapter parts into a response.
func (*pluginModel) generate(ctx context.Context, open func(context.Context, fantasy.Call) (fantasy.StreamResponse, error), call fantasy.Call) (*fantasy.Response, error) {
	stream, err := open(ctx, call)
	if err != nil {
		return nil, err
	}
	response := &fantasy.Response{}
	slots := map[string]int{}
	for part := range stream {
		switch part.Type {
		case fantasy.StreamPartTypeError:
			return nil, part.Error
		case fantasy.StreamPartTypeTextDelta:
			key := "text:" + part.ID
			index, exists := slots[key]
			if !exists {
				index, slots[key] = len(response.Content), len(response.Content)
				response.Content = append(response.Content, fantasy.TextContent{})
			}
			value := response.Content[index].(fantasy.TextContent)
			value.Text += part.Delta
			value.ProviderMetadata = part.ProviderMetadata
			response.Content[index] = value
		case fantasy.StreamPartTypeReasoningDelta, fantasy.StreamPartTypeReasoningEnd:
			key := "reasoning:" + part.ID
			index, exists := slots[key]
			if !exists {
				index, slots[key] = len(response.Content), len(response.Content)
				response.Content = append(response.Content, fantasy.ReasoningContent{})
			}
			value := response.Content[index].(fantasy.ReasoningContent)
			value.Text += part.Delta
			if value.ProviderMetadata == nil {
				value.ProviderMetadata = fantasy.ProviderMetadata{}
			}
			maps.Copy(value.ProviderMetadata, part.ProviderMetadata)
			response.Content[index] = value
		case fantasy.StreamPartTypeToolCall:
			response.Content = append(response.Content, fantasy.ToolCallContent{ToolCallID: part.ID, ToolName: part.ToolCallName, Input: part.ToolCallInput, ProviderMetadata: part.ProviderMetadata})
		case fantasy.StreamPartTypeFinish:
			response.Usage, response.FinishReason, response.ProviderMetadata = part.Usage, part.FinishReason, part.ProviderMetadata
		}
	}
	return response, nil
}

func (*pluginModel) GenerateObject(context.Context, fantasy.ObjectCall) (*fantasy.ObjectResponse, error) {
	return nil, &Failure{Kind: Unsupported, Message: "structured generation is not configured"}
}

func (*pluginModel) StreamObject(context.Context, fantasy.ObjectCall) (fantasy.ObjectStreamResponse, error) {
	return nil, &Failure{Kind: Unsupported, Message: "structured generation is not configured"}
}

func (model *pluginModel) classify(ctx context.Context, response *http.Response, headers map[string][]string, worker *plugins.Translation) error {
	body, err := io.ReadAll(io.LimitReader(response.Body, pluginResponseBytes+1))
	if err != nil {
		return &Failure{Kind: Network, Message: "model connection failed"}
	}
	if len(body) > pluginResponseBytes {
		return pluginResourceFailure()
	}
	if !utf8.Valid(body) {
		return &Failure{Kind: InvalidRequest, Message: "model error response is not UTF-8"}
	}
	if err := model.public(string(body)); err != nil {
		return err
	}
	failure := &Failure{Kind: pluginHTTPClass(response.StatusCode), Message: fmt.Sprintf("model endpoint returned HTTP %d", response.StatusCode)}
	status := uint16(response.StatusCode)
	failure.status = &status
	classifies, err := worker.HasExport("classify")
	if err != nil {
		return pluginTranslationFailure(err)
	}
	if classifies {
		raw, err := worker.Call("classify", map[string]any{"status": response.StatusCode, "headers": headers, "body": string(body)}, pluginResponseBytes)
		if err != nil {
			return pluginTranslationFailure(err)
		}
		var classified protocol.PluginModelAdapterWireDeltaErrorPayload
		if json.Unmarshal(raw, &classified) != nil {
			return &Failure{Kind: InvalidRequest, Message: "model translation returned an invalid error"}
		}
		classifiedFailure := pluginClassified(classified.Class, classified.Message, model.capture.Secrets)
		classifiedFailure.status = &status
		failure = classifiedFailure
	}
	return failure
}

func pluginTranslationFailure(err error) error {
	if errors.Is(err, context.Canceled) {
		return &Failure{Kind: Cancelled, Message: "model translation was cancelled"}
	}
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		kind := InvalidRequest
		if fault.Code == protocol.ErrorCodeCancelled {
			kind = Cancelled
		} else if fault.Code == protocol.ErrorCodeUnavailable {
			kind = Unavailable
		} else if fault.Code == protocol.ErrorCodeInternal {
			kind = Other
		}
		return &Failure{Kind: kind, Message: "model translation failed"}
	}
	return &Failure{Kind: InvalidRequest, Message: "model translation failed"}
}

func pluginResourceFailure() *Failure {
	return &Failure{Kind: Other, Message: "model adapter resource budget exceeded"}
}
