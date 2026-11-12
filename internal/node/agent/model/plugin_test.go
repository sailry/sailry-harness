package model

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"reflect"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginRoundTripper func(*http.Request) (*http.Response, error)

func (transport pluginRoundTripper) RoundTrip(request *http.Request) (*http.Response, error) {
	return transport(request)
}

func pluginFixture(t *testing.T, source string, transport pluginRoundTripper) (*pluginModel, Binding, PluginCapture) {
	t.Helper()
	capabilities := protocol.PluginModelAdapterCapabilities{Vision: true, Files: true, Tools: true, Reasoning: true}
	selection := protocol.DefaultConversationPresetsBinding()
	selection.Package = &protocol.PluginReference{Name: "fixture", Digest: strings.Repeat("a", 64)}
	selection.Adapter = &protocol.PluginModelAdapterBinding{Adapter: "fixture", Capabilities: capabilities}
	captured := &selection
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Enabled = protocol.NewProviderID(), true
	provider.Api, provider.Authentication = protocol.ConversationModelApiPlugin, protocol.AuthenticationApiKey
	provider.Endpoint, provider.Binding = "https://example.test/v1", &captured
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = provider.ID, "captured"
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
	key := protocol.NewSecret("private-token")
	binding, err := Capture(provider, config, &key, protocol.NewSessionID())
	if err != nil {
		t.Fatal(err)
	}
	declaration := protocol.DefaultPluginModelAdapterDeclaration()
	declaration.ID, declaration.Framing, declaration.Capabilities = "fixture", protocol.PluginModelAdapterFramingSse, capabilities
	declaration.Script.Entry = "dev.sailry.platform/adapter.js"
	capture := PluginCapture{Declaration: declaration, Bundle: plugins.ScriptBundle{Entry: declaration.Script.Entry, Files: map[string]string{declaration.Script.Entry: source}}}
	if transport != nil {
		capture.Client = &http.Client{Transport: transport}
	}
	model, err := NewPlugin(t.Context(), binding, capture)
	if err != nil {
		t.Fatal(err)
	}
	return model.(*pluginModel), binding, capture
}

func pluginDeltas(t *testing.T, source string) []protocol.PluginModelAdapterWireDelta {
	t.Helper()
	var deltas []protocol.PluginModelAdapterWireDelta
	if err := json.Unmarshal([]byte(source), &deltas); err != nil {
		t.Fatal(err)
	}
	return deltas
}

func TestPluginModelWireParity(t *testing.T) {
	t.Run("retains neutral messages and exact schemas", func(t *testing.T) {
		model, binding, _ := pluginFixture(t, "", nil)
		maximum := int64(128)
		signature := "signature"
		user := canonical.NewMessage(canonical.User, canonical.Text(0, "task"), canonical.Block{Index: 1, Kind: "image",
			Data: &canonical.AssetContent{Asset: canonical.AssetRef{Digest: "image", Mime: "image/png"}}})
		assistant := canonical.NewMessage(canonical.Assistant,
			canonical.Block{Index: 0, Kind: "thinking", Data: &canonical.Thinking{Text: "reason", Signature: &signature}},
			canonical.Block{Index: 1, Kind: "tool_request", Data: &canonical.ToolRequest{ID: "call-1", Name: "lookup", Arguments: json.RawMessage(`{"path":"file"}`)}})
		result := canonical.NewMessage(canonical.User, canonical.Block{Index: 0, Kind: "tool_response", Data: &canonical.ToolResponse{
			ID: "call-1", Name: "untrusted-name", Status: canonical.ToolSuccess, Format: canonical.MCP,
			Result: canonical.Payload{Value: json.RawMessage(`{"content":[{"type":"text","text":"result"}],"structuredContent":{"bytes":"unchanged"}}`)}, Revision: strings.Repeat("a", 64),
		}})
		source := Scope(binding)
		turn := protocol.NewTurnID()
		created := time.Unix(100, 0)
		records := []canonical.Record{
			canonical.NewRecord(turn, "message", user, created),
			canonical.NewRecord(turn, "message", assistant, created),
			canonical.NewRecord(turn, "message", result, created),
		}
		records[1].Metadata.Execution = &canonical.Execution{Source: &source}
		schema := json.RawMessage(`{"type":"object","properties":{"path":{"type":"string"}},"additionalProperties":false}`)
		request := Request{System: "instructions", Records: records, Assets: map[string][]byte{"image": []byte("image")}, MaxOutputTokens: &maximum,
			Tools: []Tool{{Name: "lookup", Description: "Read", InputSchema: map[string]any{"type": "object"}}}, ToolSchemas: map[string]json.RawMessage{"lookup": schema}}
		binding.Config.Effort = protocol.Effort{Kind: "high"}
		(**binding.Provider.Binding).Policies = []protocol.ConversationPresetsRequestPolicy{{Models: []string{"captured"}, Body: map[string]json.RawMessage{
			"top_p": json.RawMessage(`0.9`), "custom": json.RawMessage(`"frozen"`), "thinking_effort": json.RawMessage(`"high"`),
			"budget_tokens": json.RawMessage(`4096`), "preserve_thinking_context": json.RawMessage(`true`), "stop": json.RawMessage(`["END"]`),
		}}}
		for _, dialect := range []protocol.PluginModelAdapterDialect{protocol.PluginModelAdapterDialectGeneric, protocol.PluginModelAdapterDialectOpenAi, protocol.PluginModelAdapterDialectGemini} {
			model.capture.Declaration.SchemaDialect = dialect
			call, err := BuildCall(binding, request)
			if err != nil {
				t.Fatal(err)
			}
			wire, err := model.request(call)
			if err != nil {
				t.Fatal(err)
			}
			if wire.Model != "captured" || wire.System == nil || *wire.System != "instructions" || len(wire.Messages) != 3 ||
				wire.TopP == nil || *wire.TopP != 0.9 || !reflect.DeepEqual(wire.Stop, []string{"END"}) || len(wire.Options) != 1 ||
				string(wire.Options["custom"]) != `"frozen"` || !bytes.Equal(wire.Tools[0].Parameters, schema) {
				t.Fatalf("neutral request changed for %s: %+v", dialect, wire)
			}
			reasoning := wire.Messages[1].Data.(*protocol.PluginModelAdapterWireMessageAssistantPayload).Parts[0].Data.(*protocol.PluginModelAdapterWirePartReasoningPayload)
			toolResult := wire.Messages[2].Data.(*protocol.PluginModelAdapterWireMessageToolPayload)
			var actualResult, expectedResult any
			if json.Unmarshal(toolResult.Parts[0].Data.(*protocol.PluginModelAdapterWirePartJsonPayload).Value, &actualResult) != nil ||
				json.Unmarshal(result.Content[0].Data.(*canonical.ToolResponse).Result.Value, &expectedResult) != nil {
				t.Fatal("neutral MCP result is invalid JSON")
			}
			if reasoning.Replay == nil || *reasoning.Replay != signature || toolResult.Name != "lookup" || toolResult.IsError ||
				!reflect.DeepEqual(actualResult, expectedResult) {
				t.Fatal("reasoning replay, tool identity, or full MCP result changed")
			}
		}
	})
	t.Run("rejects undeclared capabilities", func(t *testing.T) {
		model, binding, _ := pluginFixture(t, "", nil)
		model.capture.Declaration.Capabilities = protocol.PluginModelAdapterCapabilities{}
		for _, body := range []string{
			`[{"role":"user","parts":[{"kind":"image","media_type":"image/png","data":{"kind":"inline","data_base64":"aW1hZ2U="}}]}]`,
			`[{"role":"assistant","parts":[{"kind":"reasoning","text":"reason","replay":"signature"}]}]`,
			`[{"role":"user","parts":[{"kind":"file","media_type":"application/pdf","name":null,"data_base64":"cGRm"}]}]`,
			`[{"role":"assistant","parts":[{"kind":"tool_call","id":"call","name":"lookup","arguments":{}}]}]`,
		} {
			wire := protocol.DefaultPluginModelAdapterWireRequest()
			wire.Version, wire.Model, wire.ToolChoice = "v1", binding.Config.Model, protocol.PluginModelAdapterWireChoice{Kind: "auto"}
			if err := json.Unmarshal([]byte(body), &wire.Messages); err != nil {
				t.Fatal(err)
			}
			if err := validatePluginCapabilities(wire, model.capture.Declaration.Capabilities); Classify(err) == nil || Classify(err).Kind != InvalidRequest {
				t.Fatalf("unsupported content accepted: %s, %v", body, err)
			}
		}
		wire := protocol.DefaultPluginModelAdapterWireRequest()
		wire.Reasoning.Effort = protocol.Effort{Kind: "high"}
		if validatePluginCapabilities(wire, model.capture.Declaration.Capabilities) == nil {
			t.Fatal("unsupported reasoning effort accepted")
		}
	})
	t.Run("joins calls and preserves unknown usage", func(t *testing.T) {
		accumulator := &pluginAccumulator{}
		parts, err := accumulator.push(pluginDeltas(t, `[
{"kind":"text","text":"Hello"},
{"kind":"tool_call","index":0,"id":"call","name":"look","arguments_delta":"{\"path\":"},
{"kind":"tool_call","index":0,"name":"up","arguments_delta":"\"file\"}"},
{"kind":"usage","output_tokens":3},
{"kind":"finish","reason":"tool_calls"}]`))
		if err != nil || len(parts) != 3 || !PluginOutput(parts[1]) || !PluginOutput(parts[2]) {
			t.Fatalf("partial call output = %+v, %v", parts, err)
		}
		final, err := accumulator.finish()
		if err != nil || len(final) != 2 {
			t.Fatalf("final call = %+v, %v", final, err)
		}
		call := final[0]
		usage := final[1].ProviderMetadata[UsageMetadataKey].(*UsageMetadata)
		if call.ID != "call" || call.ToolCallName != "lookup" || call.ToolCallInput != `{"path":"file"}` ||
			usage.Input != nil || usage.Total != nil || usage.Output == nil || *usage.Output != 3 {
			t.Fatalf("call or optional usage changed: %+v, %+v", call, usage)
		}
	})
	t.Run("joins replay at the reasoning boundary", func(t *testing.T) {
		accumulator := &pluginAccumulator{}
		parts, err := accumulator.push(pluginDeltas(t, `[{"kind":"reasoning","text":"Plan","replay":"first-"},{"kind":"reasoning","replay":"second"}]`))
		if err != nil || len(parts) != 2 || !PluginOutput(parts[1]) {
			t.Fatalf("raw replay output = %+v, %v", parts, err)
		}
		completed, err := accumulator.push(pluginDeltas(t, `[{"kind":"text","text":"Answer"}]`))
		if err != nil || len(completed) != 2 || completed[0].ID != parts[0].ID {
			t.Fatalf("reasoning boundary = %+v, %v", completed, err)
		}
		block := canonical.Block{Kind: "thinking", Data: &canonical.Thinking{Text: "Plan"}}
		if err := ApplyStreamMetadata(&block, completed[0]); err != nil {
			t.Fatal(err)
		}
		if replay := block.Data.(*canonical.Thinking).Signature; replay == nil || *replay != "first-second" {
			t.Fatalf("joined replay = %v", replay)
		}
	})
	t.Run("retains optional reasoning counts and malformed arguments", func(t *testing.T) {
		for _, tokens := range []string{"null", "0", "2"} {
			accumulator := &pluginAccumulator{}
			_, err := accumulator.push(pluginDeltas(t, `[{"kind":"usage","input_tokens":12,"output_tokens":3,"reasoning_tokens":`+tokens+`},{"kind":"finish","reason":"stop"}]`))
			if err != nil {
				t.Fatal(err)
			}
			parts, err := accumulator.finish()
			if err != nil {
				t.Fatal(err)
			}
			usage := parts[0].ProviderMetadata[UsageMetadataKey].(*UsageMetadata)
			if usage.Total == nil || *usage.Total != 15 || (tokens == "null") != (usage.Reasoning == nil) ||
				tokens == "0" && *usage.Reasoning != 0 || tokens == "2" && *usage.Reasoning != 2 {
				t.Fatalf("optional reasoning count %s changed: %+v", tokens, usage)
			}
		}
		accumulator := &pluginAccumulator{}
		if _, err := accumulator.push(pluginDeltas(t, `[{"kind":"tool_call","index":0,"id":"call","name":"lookup","arguments_delta":"{broken"},{"kind":"finish","reason":"tool_calls"}]`)); err != nil {
			t.Fatal(err)
		}
		parts, err := accumulator.finish()
		if err != nil {
			t.Fatal(err)
		}
		block := canonical.Block{Kind: "tool_request", Data: &canonical.ToolRequest{}}
		if err := ApplyStreamMetadata(&block, parts[0]); err != nil {
			t.Fatal(err)
		}
		if parts[0].ToolCallInput != "null" || block.Data.(*canonical.ToolRequest).Error == nil || block.Data.(*canonical.ToolRequest).Error.Code != -32602 {
			t.Fatal("invalid arguments were substituted")
		}
		if _, err := (&pluginAccumulator{}).finish(); Classify(err).Kind != InvalidResponse {
			t.Fatal("premature EOF became a network failure")
		}
	})
	t.Run("frames split UTF8 and cumulative budgets", func(t *testing.T) {
		decoder := &pluginDecoder{framing: protocol.PluginModelAdapterFramingSse, limit: 1024, first: true}
		body := []byte("\ufeff: ping\r\nevent: delta\r\ndata: 中文\r\ndata: second\r\n\r\n")
		var events []protocol.PluginModelAdapterWireEvent
		for index := range body {
			parts, err := decoder.push(body[index:index+1], index+1 == len(body))
			if err != nil {
				t.Fatal(err)
			}
			events = append(events, parts...)
		}
		if len(events) != 1 || events[0].Event == nil || *events[0].Event != "delta" || events[0].Data != "中文\nsecond" {
			t.Fatalf("split UTF8 SSE = %+v", events)
		}
		ndjson := &pluginDecoder{framing: protocol.PluginModelAdapterFramingNdjson, limit: 8, first: true}
		events, err := ndjson.push([]byte("{}\n{}"), true)
		if err != nil || len(events) != 2 || events[1].Data != "{}" {
			t.Fatalf("unterminated NDJSON = %+v, %v", events, err)
		}
		if _, err := ndjson.push([]byte("1234"), false); Classify(err) == nil || Classify(err).Kind != Other {
			t.Fatal("cumulative framing budget was reset")
		}
		plain := &pluginDecoder{framing: protocol.PluginModelAdapterFramingJson, limit: 8}
		if events, err := plain.push([]byte("{}"), false); err != nil || len(events) != 0 {
			t.Fatal("JSON was emitted before EOF")
		}
		if events, err := plain.push(nil, true); err != nil || len(events) != 1 || events[0].Data != "{}" {
			t.Fatal("JSON was not emitted at EOF")
		}
	})
	t.Run("keeps pure translation state and large image input", func(t *testing.T) {
		_, binding, capture := pluginFixture(t, `let count=0; export function decode(input){return {count:++count,input}} export function prepare(input){return input}`, nil)
		ctx, cancel := context.WithCancel(t.Context())
		defer cancel()
		worker, err := plugins.NewTranslation(ctx, capture.Bundle)
		if err != nil {
			t.Fatal(err)
		}
		defer worker.Close()
		for index := 1; index <= 2; index++ {
			raw, err := worker.Call("decode", map[string]int{"value": index}, 4096)
			if err != nil {
				t.Fatal(err)
			}
			var value struct{ Count int }
			if json.Unmarshal(raw, &value) != nil || value.Count != index {
				t.Fatalf("translation state = %s", raw)
			}
		}
		message := canonical.NewMessage(canonical.User, canonical.Block{Index: 0, Kind: "image", Data: &canonical.AssetContent{Asset: canonical.AssetRef{Digest: "image", Mime: "image/png"}}})
		request := Request{Assets: map[string][]byte{"image": bytes.Repeat([]byte{1}, 2126177)}, Records: []canonical.Record{canonical.NewRecord(protocol.NewTurnID(), "message", message, time.Unix(100, 0))}}
		for index := range 128 {
			request.Tools = append(request.Tools, Tool{Name: fmt.Sprintf("tool_%d", index), InputSchema: map[string]any{"type": "object"}})
		}
		wire, err := pluginRequest(binding, request)
		if err != nil {
			t.Fatal(err)
		}
		raw, err := worker.Call("prepare", wire, pluginRequestBytes)
		if err != nil {
			t.Fatal(err)
		}
		var echoed protocol.PluginModelAdapterWireRequest
		if json.Unmarshal(raw, &echoed) != nil || len(echoed.Tools) != 128 {
			t.Fatal("large neutral input was truncated")
		}
		image := echoed.Messages[0].Data.(*protocol.PluginModelAdapterWireMessageUserPayload).Parts[0].Data.(*protocol.PluginModelAdapterWirePartImagePayload)
		data, err := base64.StdEncoding.DecodeString(image.Data.Data.(*protocol.PluginModelAdapterWireImageInlinePayload).DataBase64)
		if err != nil || len(data) != 2126177 {
			t.Fatal("large inline image changed")
		}
		cancel()
		if _, err := worker.Call("decode", map[string]any{}, 4096); err == nil {
			t.Fatal("cancelled translation remained active")
		}
	})
}

func TestPluginModelTransport(t *testing.T) {
	t.Run("native HTTP hides credentials and private response headers", func(t *testing.T) {
		requests := 0
		model, binding, capture := pluginFixture(t, `
let count=0;
export function prepare(input) { if(JSON.stringify(input).includes("private-token")) throw Error("credential leaked"); return {method:"POST",url:input.endpoint+"/messages",headers:{"content-type":"application/json"},body:JSON.stringify(input)}; }
export function decode(input) { if(input.headers["set-cookie"] || input.headers.authorization) throw Error("private headers leaked"); ++count; return {state:count,deltas:input.events.map(event=>JSON.parse(event.data))}; }
`, func(request *http.Request) (*http.Response, error) {
			requests++
			if request.Header.Get("Authorization") != "Bearer private-token" || request.URL.Path != "/v1/messages" {
				t.Fatal("native request lost captured credential or endpoint")
			}
			body, err := io.ReadAll(request.Body)
			if err != nil || strings.Contains(string(body), "private-token") {
				t.Fatal("credential entered neutral wire input")
			}
			return &http.Response{StatusCode: 200, Header: http.Header{"Content-Type": []string{"text/event-stream"}, "Set-Cookie": []string{"private-token"}},
				Body: io.NopCloser(strings.NewReader("data: {\"kind\":\"text\",\"text\":\"Hello\"}\n\ndata: {\"kind\":\"finish\",\"reason\":\"stop\"}\n\n"))}, nil
		})
		capture.Bundle.Files[capture.Bundle.Entry] = `throw Error("later mutation")`
		call, err := BuildCall(binding, Request{System: "captured"})
		if err != nil {
			t.Fatal(err)
		}
		parts := collect(t, model, call)
		if requests != 1 || len(parts) != 2 || parts[0].Delta != "Hello" || parts[1].FinishReason != fantasy.FinishReasonStop {
			t.Fatalf("single captured translation = %d, %+v", requests, parts)
		}
	})
	t.Run("a consumer that stops on cancellation is not yielded to again", func(t *testing.T) {
		model, binding, _ := pluginFixture(t, `
export function prepare(input) { return {method:"POST",url:input.endpoint+"/messages",body:"{}"}; }
export function decode(input) { return {state:null,deltas:input.events.map(event=>JSON.parse(event.data))}; }
`, func(*http.Request) (*http.Response, error) {
			return &http.Response{StatusCode: 200, Header: http.Header{"Content-Type": []string{"text/event-stream"}},
				Body: io.NopCloser(strings.NewReader("data: {\"kind\":\"text\",\"text\":\"Hello\"}\n\ndata: {\"kind\":\"finish\",\"reason\":\"stop\"}\n\n"))}, nil
		})
		call, err := BuildCall(binding, Request{System: "captured"})
		if err != nil {
			t.Fatal(err)
		}
		ctx, cancel := context.WithCancel(t.Context())
		defer cancel()
		stream, err := model.Stream(ctx, call)
		if err != nil {
			t.Fatal(err)
		}
		parts := 0
		for range stream {
			parts++
			cancel()
			break
		}
		if parts != 1 {
			t.Fatalf("stopped consumer saw %d parts", parts)
		}
	})
	t.Run("a refusal from prepare sends nothing and keeps its message", func(t *testing.T) {
		requests := 0
		model, binding, _ := pluginFixture(t, `
export function prepare(input) {
  if (input.settings.free_only !== false && input.options.fixture_free !== true) {
    return {refusal:{class:"invalid_request",message:"fixture model is not free: private-token"}};
  }
  return {method:"POST",url:input.endpoint+"/messages",body:"{}"};
}
export function decode(input) { return {state:null,deltas:input.events.map(event=>JSON.parse(event.data))}; }
`, func(*http.Request) (*http.Response, error) {
			requests++
			return &http.Response{StatusCode: 200, Header: http.Header{"Content-Type": []string{"text/event-stream"}},
				Body: io.NopCloser(strings.NewReader("data: {\"kind\":\"finish\",\"reason\":\"stop\"}\n\n"))}, nil
		})
		send := func(binding Binding) *Failure {
			call, err := BuildCall(binding, Request{System: "captured"})
			if err != nil {
				t.Fatal(err)
			}
			stream, err := model.Stream(t.Context(), call)
			if err != nil {
				t.Fatal(err)
			}
			for part := range stream {
				if part.Type == fantasy.StreamPartTypeError {
					return Classify(part.Error)
				}
			}
			return nil
		}
		model.capture.Settings = map[string]json.RawMessage{"free_only": json.RawMessage(`true`)}
		failure := send(binding)
		if failure == nil || failure.Kind != InvalidRequest || failure.Retryable() || failure.Message != "fixture model is not free: [redacted]" || requests != 0 {
			t.Fatalf("refusal = %+v after %d requests", failure, requests)
		}
		// What discovery recorded about the model reaches prepare as an option.
		entry := protocol.DefaultConversationModel()
		entry.ID = "captured"
		capabilities := protocol.DefaultConversationDiscoveryCapabilities()
		capabilities.Options = map[string]json.RawMessage{"fixture_free": json.RawMessage(`true`)}
		entry.Capabilities = &capabilities
		stamped := binding
		stamped.Provider.Models = []protocol.ConversationModel{entry}
		if failure := send(stamped); failure != nil || requests != 1 {
			t.Fatalf("stamped model = %+v after %d requests", failure, requests)
		}
		model.capture.Settings = map[string]json.RawMessage{"free_only": json.RawMessage(`false`)}
		if failure := send(binding); failure != nil || requests != 2 {
			t.Fatalf("setting off = %+v after %d requests", failure, requests)
		}
	})
	t.Run("origin and header authority fence requests", func(t *testing.T) {
		requests := 0
		model, _, _ := pluginFixture(t, "", func(request *http.Request) (*http.Response, error) {
			requests++
			if request.Header.Get("Authorization") != "" {
				t.Fatal("credential left the configured origin")
			}
			return &http.Response{StatusCode: 200, Header: http.Header{}, Body: io.NopCloser(strings.NewReader(""))}, nil
		})
		model.capture.Declaration.Origins = []string{"https://other.test"}
		prepared := protocol.DefaultPluginModelAdapterWirePrepared()
		prepared.Url = "https://other.test/messages"
		response, err := model.send(t.Context(), prepared, pluginRequestBytes)
		if err != nil {
			t.Fatal(err)
		}
		response.Body.Close()
		prepared.Credential = &protocol.PluginModelAdapterWireInjection{Header: "Authorization", Template: "Bearer {key}"}
		if _, err := model.send(t.Context(), prepared, pluginRequestBytes); err == nil {
			t.Fatal("cross-origin credential injection was accepted")
		}
		prepared.Credential, prepared.Url = nil, "https://undeclared.test/messages"
		if _, err := model.send(t.Context(), prepared, pluginRequestBytes); err == nil {
			t.Fatal("undeclared request origin was accepted")
		}
		prepared.Url, prepared.Headers = "https://example.test/v1", map[string]string{"Host": "other.test"}
		if _, err := model.send(t.Context(), prepared, pluginRequestBytes); err == nil || requests != 1 {
			t.Fatal("forbidden header reached native HTTP")
		}
		if model.public("safe response") != nil || model.public("private-token") == nil {
			t.Fatal("private response credential was not rejected")
		}
		if _, err := model.publicHeaders(http.Header{"Retry-After": []string{"private-token"}}); err == nil {
			t.Fatal("private retry header reached translation")
		}
	})
	t.Run("keeps provider failures distinct from network failures", func(t *testing.T) {
		for _, class := range []protocol.PluginModelAdapterWireClass{protocol.PluginModelAdapterWireClassUnknown, protocol.PluginModelAdapterWireClassUnavailable} {
			failure := pluginClassified(class, "provider unavailable", nil)
			if failure.Kind != Unavailable || !failure.Retryable() {
				t.Fatalf("provider rejection = %+v", failure)
			}
		}
		failure := pluginClassified(protocol.PluginModelAdapterWireClassRateLimited, "private-token quota", []protocol.Secret{protocol.NewSecret("private-token")})
		if failure.Kind != RateLimited || failure.Message != "[redacted] quota" {
			t.Fatalf("classified private diagnostics = %+v", failure)
		}
		model, _, _ := pluginFixture(t, `export function classify(input){return {class:"quota",message:"quota exhausted"}}`, nil)
		worker, err := plugins.NewTranslation(t.Context(), model.capture.Bundle)
		if err != nil {
			t.Fatal(err)
		}
		defer worker.Close()
		response := &http.Response{StatusCode: 429, Header: http.Header{"Retry-After": []string{"2"}}, Body: io.NopCloser(strings.NewReader(`{"error":"quota"}`))}
		err = model.classify(t.Context(), response, map[string][]string{"retry-after": {"2"}}, worker)
		failure = Classify(err)
		if failure.Kind != Quota {
			t.Fatalf("classified response = %+v", failure)
		}
		model.client.Transport = pluginRoundTripper(func(*http.Request) (*http.Response, error) {
			return nil, errors.New("connection failed with private-token")
		})
		prepared := protocol.DefaultPluginModelAdapterWirePrepared()
		prepared.Url = "https://example.test/v1"
		if _, err := model.send(t.Context(), prepared, pluginRequestBytes); Classify(err).Kind != Network || strings.Contains(err.Error(), "private-token") {
			t.Fatalf("native network failure = %v", err)
		}
	})
	t.Run("discovery shares credential boundary and clamps capabilities", func(t *testing.T) {
		status := 200
		catalog := `{"endpoint":"https://example.test/v1/","models":[{"id":"fixture","context":4096,"output":1024,"capabilities":{"vision":true,"tools":true,"reasoning":true,"features":[]}}]}`
		_, binding, capture := pluginFixture(t, `
export function prepare_discovery(input){return {method:"GET",url:input.endpoint+"/models"}}
export function decode_discovery(input){if(input.status!==200)throw Error("failed body decoded");return JSON.parse(input.body)}
`, func(request *http.Request) (*http.Response, error) {
			if request.Header.Get("Authorization") != "Bearer private-token" || request.URL.Path != "/v1/models" {
				t.Fatal("discovery lost its captured request")
			}
			return &http.Response{StatusCode: status, Header: http.Header{}, Body: io.NopCloser(strings.NewReader(catalog))}, nil
		})
		capture.Declaration.Discovery = true
		capture.Declaration.Capabilities = protocol.PluginModelAdapterCapabilities{}
		(**binding.Provider.Binding).Adapter.Capabilities = capture.Declaration.Capabilities
		result, err := DiscoverPlugin(t.Context(), binding, capture)
		if err != nil {
			t.Fatal(err)
		}
		if len(result.Models) != 1 || result.Models[0].Capabilities == nil || *result.Models[0].Capabilities.Vision || *result.Models[0].Capabilities.Tools || *result.Models[0].Capabilities.Reasoning {
			t.Fatalf("discovery elevated declared capability: %+v", result)
		}
		// Discovery receives the package settings and keeps what it records
		// about a model for prepare; an oversized record is refused.
		status = 200
		catalog = `{"endpoint":"https://example.test/v1/","models":[{"id":"fixture","context":4096,"output":1024,"capabilities":{"options":{"fixture_free":true}}}]}`
		capture.Bundle.Files[capture.Bundle.Entry] = `
export function prepare_discovery(input){if(input.settings.free_only!==true)throw Error("settings missing");return {method:"GET",url:input.endpoint+"/models"}}
export function decode_discovery(input){if(input.settings.free_only!==true)throw Error("settings missing");return JSON.parse(input.body)}
`
		capture.Settings = map[string]json.RawMessage{"free_only": json.RawMessage(`true`)}
		result, err = DiscoverPlugin(t.Context(), binding, capture)
		if err != nil || len(result.Models) != 1 || string(result.Models[0].Capabilities.Options["fixture_free"]) != "true" {
			t.Fatalf("discovery options: %v %+v", err, result)
		}
		catalog = `{"endpoint":"https://example.test/v1/","models":[{"id":"fixture","capabilities":{"options":{"note":"` + strings.Repeat("a", 5000) + `"}}}]}`
		if _, err := DiscoverPlugin(t.Context(), binding, capture); err == nil {
			t.Fatal("oversized adapter options were accepted")
		}
		status, catalog = 401, "private-token"
		if _, err := DiscoverPlugin(t.Context(), binding, capture); err == nil || Classify(err).Kind != Unavailable || strings.Contains(err.Error(), "private-token") {
			t.Fatalf("failed discovery body escaped: %v", err)
		}
	})
}
