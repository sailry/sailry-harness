package web

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"math"
	"net/http"
	"reflect"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func nativeFixture(api protocol.ConversationModelApi) string {
	switch api {
	case protocol.ConversationModelApiResponses:
		return `{"status":"completed","output":[{"type":"reasoning","encrypted_content":"opaque-private-state"},{"type":"web_search_call","status":"completed","action":{"sources":[{"url":"https://example.com/source"}]}},{"type":"message","content":[{"type":"output_text","text":"Generated answer","annotations":[{"type":"url_citation","url":"https://example.com/source","title":"Public source"}]}]}],"usage":{"input_tokens":100,"output_tokens":40,"input_tokens_details":{"cached_tokens":20},"output_tokens_details":{"reasoning_tokens":5}}}`
	case protocol.ConversationModelApiAnthropic:
		return `{"type":"message","stop_reason":"end_turn","content":[{"type":"web_search_tool_result","content":[{"type":"web_search_result","url":"https://example.com/source","title":"Public source","encrypted_content":"opaque-private-state"}]},{"type":"text","text":"Generated answer","citations":[{"type":"web_search_result_location","url":"https://example.com/source","title":"Public source","cited_text":"Original source excerpt"}]}],"usage":{"input_tokens":30,"output_tokens":40,"cache_read_input_tokens":20,"cache_creation_input_tokens":50}}`
	case protocol.ConversationModelApiGemini:
		return `{"candidates":[{"finishReason":"STOP","content":{"parts":[{"text":"Private reasoning","thought":true},{"text":"Generated answer"}]},"groundingMetadata":{"groundingChunks":[{"web":{"uri":"https://example.com/source","title":"Public source"}}],"groundingSupports":[{"segment":{"text":"Generated answer"}}],"webSearchQueries":["search fixture"]}}],"usageMetadata":{"promptTokenCount":100,"candidatesTokenCount":35,"thoughtsTokenCount":5,"cachedContentTokenCount":20}}`
	}
	return `{}`
}

func nativeSearchFixture(api protocol.ConversationModelApi) NativeSearch {
	return NativeSearch{
		Provider: protocol.ConversationProvider{Enabled: true, Api: api, Authentication: protocol.AuthenticationApiKey, Models: []protocol.ConversationModel{{ID: "fixture-model"}}},
		Model:    "fixture-model", Input: protocol.SearchRequest{Query: "search fixture", Limit: 5}, Now: func() time.Time { return time.Date(2026, 10, 30, 12, 0, 0, 0, time.UTC) },
		Prepare: func(ctx context.Context, body map[string]any) (*http.Request, error) {
			encoded, err := json.Marshal(body)
			if err != nil {
				return nil, err
			}
			return http.NewRequestWithContext(ctx, http.MethodPost, "https://model.test/search", bytes.NewReader(encoded))
		},
		Transport: fixtureTransport(func(*http.Request) (*http.Response, error) {
			return responseFixture(nativeFixture(api), "application/json"), nil
		}),
	}
}

func TestNativeSearchContracts(t *testing.T) {
	t.Parallel()
	for _, api := range []protocol.ConversationModelApi{protocol.ConversationModelApiResponses, protocol.ConversationModelApiAnthropic, protocol.ConversationModelApiGemini} {
		t.Run(string(api), func(t *testing.T) {
			search := nativeSearchFixture(api)
			freshness := protocol.SearchFreshnessHour
			search.Input.Freshness, search.Input.IncludeDomains, search.Input.ExcludeDomains = &freshness, []string{"example.com"}, []string{"private.example.com"}
			var recorded []canonical.Usage
			search.Record = func(_ context.Context, usage canonical.Usage) error { recorded = append(recorded, usage); return nil }
			search.Transport = fixtureTransport(func(request *http.Request) (*http.Response, error) {
				var body map[string]any
				if err := json.NewDecoder(request.Body).Decode(&body); err != nil {
					t.Fatal(err)
				}
				query := "search fixture site:example.com -site:private.example.com"
				switch api {
				case protocol.ConversationModelApiResponses:
					items, _ := body["input"].([]any)
					tools, _ := body["tools"].([]any)
					if len(items) != 2 || jsonAt(items[0], "role") != "system" || body["stream"] != true || body["store"] != false || request.Header.Get("Accept") != "text/event-stream" {
						t.Fatal("Responses search framing changed", body)
					}
					content, _ := jsonAt(items[1], "content").([]any)
					if jsonAt(content[0], "text") != query || jsonAt(tools[0], "type") != "web_search" || !reflect.DeepEqual(jsonAt(tools[0], "filters", "blocked_domains"), []any{"private.example.com"}) {
						t.Fatal("Responses domain policy changed", body)
					}
				case protocol.ConversationModelApiAnthropic:
					messages, _ := body["messages"].([]any)
					tools, _ := body["tools"].([]any)
					if jsonAt(messages[0], "content") != query || jsonAt(tools[0], "blocked_domains") != nil || jsonAt(tools[0], "type") != "web_search_20250305" || body["max_tokens"] != float64(4096) || !strings.Contains(jsonString(body["system"]), "2026-10-30T11:00:00+00:00") {
						t.Fatal("Messages search request changed", body)
					}
				case protocol.ConversationModelApiGemini:
					tools, _ := body["tools"].([]any)
					if jsonAt(tools[0], "googleSearch", "timeRangeFilter", "startTime") != "2026-10-30T11:00:00Z" || body["systemInstruction"] == nil || body["model"] != nil {
						t.Fatal("Gemini grounding request changed", body)
					}
				}
				return responseFixture(nativeFixture(api), "application/json"), nil
			})
			page, err := SearchNative(context.Background(), search)
			if err != nil || len(page.Results) != 1 || page.Summary == nil || *page.Summary != "Generated answer" || page.Results[0].Title != "Public source" || page.Results[0].Published != nil || !page.Untrusted {
				t.Fatal(page, err)
			}
			if len(recorded) != 1 || recorded[0].Input == nil || *recorded[0].Input != 100 || recorded[0].Output == nil || *recorded[0].Output != 40 || recorded[0].CacheRead == nil || *recorded[0].CacheRead != 20 {
				t.Fatal("search usage lost provider token counts", recorded)
			}
			want := ""
			if api == protocol.ConversationModelApiAnthropic {
				want = "Original source excerpt"
				if recorded[0].CacheWrite == nil || *recorded[0].CacheWrite != 50 {
					t.Fatal("cache creation usage lost")
				}
			}
			if page.Results[0].Snippet != want {
				t.Fatal("generated answer became source excerpt", page)
			}
		})
	}
	t.Run("usage survives invalid content and caller cancellation", func(t *testing.T) {
		for _, cancelAfterResponse := range []bool{false, true} {
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			search := nativeSearchFixture(protocol.ConversationModelApiResponses)
			search.Transport = fixtureTransport(func(*http.Request) (*http.Response, error) {
				if cancelAfterResponse {
					cancel()
				}
				return responseFixture(`{"usage":{"input_tokens":12,"output_tokens":4},"status":"completed","output":[]}`, "application/json"), nil
			})
			recorded := 0
			search.Record = func(ctx context.Context, usage canonical.Usage) error {
				recorded++
				if ctx.Err() != nil || usage.Input == nil || *usage.Input != 12 {
					t.Fatal("billed usage append inherited cancellation", usage, ctx.Err())
				}
				return nil
			}
			if _, err := SearchNative(ctx, search); err == nil || recorded != 1 {
				t.Fatal("invalid billed response lost usage", err, recorded)
			}
		}
	})
	t.Run("usage append failure is permanent", func(t *testing.T) {
		search := nativeSearchFixture(protocol.ConversationModelApiResponses)
		search.Record = func(context.Context, canonical.Usage) error {
			return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "usage writer failed"}
		}
		_, err := SearchNative(context.Background(), search)
		if err == nil || searchFailureOf(err).temporary {
			t.Fatal("usage append failure allowed selected driver fallback", err)
		}
	})
	t.Run("usage preserves saturation and missing counts", func(t *testing.T) {
		value, err := nativeDecode([]byte(`{"usage":{"input_tokens":18446744073709551615,"cache_read_input_tokens":2,"cache_creation_input_tokens":3}}`))
		if err != nil {
			t.Fatal(err)
		}
		usage := nativeUsage(protocol.ConversationModelApiAnthropic, value)
		if *usage.Input != math.MaxUint64 || *usage.Output != 0 || *usage.Reasoning != 0 {
			t.Fatal(usage)
		}
	})
}

type nativeFragmentReader struct{ io.Reader }

func (reader nativeFragmentReader) Read(target []byte) (int, error) {
	return reader.Reader.Read(target[:min(len(target), 3)])
}

func TestNativeSearchStream(t *testing.T) {
	t.Parallel()
	now := time.Unix(1, 0)
	for _, output := range []string{`[]`, `null`, "missing"} {
		t.Run("collects completed items with "+output, func(t *testing.T) {
			response := `{"status":"completed","usage":{"input_tokens":31,"output_tokens":9}}`
			if output != "missing" {
				response = `{"status":"completed","output":` + output + `,"usage":{"input_tokens":31,"output_tokens":9}}`
			}
			stream := ": heartbeat\r\nevent: response.created\r\ndata: {\"type\":\"response.created\"}\r\n\r\n" +
				"data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"Unicode fixture: 中文\"}]}}\n\n" +
				"data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"reasoning\"}}\n\n" +
				"data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"web_search_call\",\"status\":\"completed\"}}\n\n" +
				"event: response.completed\ndata: {\"type\":\"response.completed\",\ndata: \"response\":" + response + "}\n\n"
			for _, contentType := range []string{"text/event-stream; charset=utf-8", ""} {
				response := responseFixture(stream, contentType)
				response.Body = io.NopCloser(nativeFragmentReader{strings.NewReader(stream)})
				value, err := nativeRead(response, protocol.ConversationModelApiResponses, now)
				if err != nil {
					t.Fatal(err)
				}
				page, err := nativeResponsesParse(value, now)
				if err != nil || page.Summary == nil || *page.Summary != "Unicode fixture: 中文" || nativeUsage(protocol.ConversationModelApiResponses, value) == nil {
					t.Fatal(page, err)
				}
			}
		})
	}
	t.Run("authoritative completion wins", func(t *testing.T) {
		stream := "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"reasoning\"}}\n\n" + "data: {\"type\":\"response.completed\",\"response\":" + nativeFixture(protocol.ConversationModelApiResponses) + "}\n\n"
		value, err := nativeRead(responseFixture(stream, "text/event-stream"), protocol.ConversationModelApiResponses, now)
		if err != nil {
			t.Fatal(err)
		}
		page, err := nativeResponsesParse(value, now)
		if err != nil || len(page.Results) != 1 {
			t.Fatal(page, err)
		}
	})
	t.Run("rejects failed truncated oversized and invalid streams", func(t *testing.T) {
		for _, stream := range []string{
			"data: [DONE]\n\n", "data: {\"type\":\"response.incomplete\"}\n\n", "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"private-access\"}}}\n\n", "data: {\"type\":\"error\",\"message\":\"private-access\"}\n\n", "data: {bad-json}\n\n", strings.Repeat("x", searchBodyLimit+1),
		} {
			_, err := nativeRead(responseFixture(stream, "text/event-stream"), protocol.ConversationModelApiResponses, now)
			if err == nil || strings.Contains(err.Error(), "private-access") {
				t.Fatal("unsafe stream was accepted", err)
			}
		}
	})
}

func TestNativeSearchContinuation(t *testing.T) {
	t.Parallel()
	paused := `{"type":"message","stop_reason":"pause_turn","content":[{"type":"web_search_tool_result","content":[{"type":"web_search_result","url":"https://example.com/paused","title":"Paused source","encrypted_content":"opaque-original"}]}],"usage":{"input_tokens":10,"output_tokens":3,"cache_read_input_tokens":2,"cache_creation_input_tokens":1}}`
	for _, exhausted := range []bool{false, true} {
		t.Run(map[bool]string{false: "preserves original blocks and records each response", true: "bounds pauses and records billed responses"}[exhausted], func(t *testing.T) {
			search := nativeSearchFixture(protocol.ConversationModelApiAnthropic)
			calls, recorded := 0, 0
			var inputTokens uint64
			search.Record = func(_ context.Context, usage canonical.Usage) error {
				recorded++
				inputTokens += *usage.Input
				return nil
			}
			search.Transport = fixtureTransport(func(request *http.Request) (*http.Response, error) {
				calls++
				encoded, err := io.ReadAll(request.Body)
				if err != nil {
					t.Fatal(err)
				}
				if calls > 1 && !strings.Contains(string(encoded), `"encrypted_content":"opaque-original"`) {
					t.Fatal("continuation lost opaque source blocks")
				}
				if exhausted || calls == 1 {
					return responseFixture(paused, "application/json"), nil
				}
				return responseFixture(nativeFixture(protocol.ConversationModelApiAnthropic), "application/json"), nil
			})
			page, err := SearchNative(context.Background(), search)
			if exhausted {
				if err == nil || err.Error() != "invalid_request: model search continuation limit reached" || calls != 4 || recorded != 4 {
					t.Fatal(page, err, calls, recorded)
				}
			} else if err != nil || calls != 2 || recorded != 2 || inputTokens != 113 || len(page.Results) != 2 || page.Results[1].Snippet != "Original source excerpt" {
				t.Fatal(page, err, calls, recorded, inputTokens)
			}
		})
	}
	t.Run("search budget preserves earlier sources", func(t *testing.T) {
		value, err := nativeDecode([]byte(nativeFixture(protocol.ConversationModelApiAnthropic)))
		if err != nil {
			t.Fatal(err)
		}
		content := value["content"].([]any)
		budgetError := map[string]any{"type": "web_search_tool_result", "content": map[string]any{"type": "web_search_tool_result_error", "error_code": "max_uses_exceeded"}}
		value["content"] = append(content, budgetError)
		if page, err := nativeAnthropicParse(value, time.Now()); err != nil || len(page.Results) != 1 {
			t.Fatal(page, err)
		}
		value["content"] = []any{budgetError}
		if _, err := nativeAnthropicParse(value, time.Now()); err == nil {
			t.Fatal("budget-only failure became successful empty search")
		}
	})
}

func TestNativeSearchProtection(t *testing.T) {
	t.Parallel()
	t.Run("rejects plain answers unsafe citations and protected output", func(t *testing.T) {
		for _, body := range []string{
			`{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"Answer"}]}]}`,
			strings.ReplaceAll(nativeFixture(protocol.ConversationModelApiResponses), "https://example.com/source", "file:///private/source"),
			strings.ReplaceAll(nativeFixture(protocol.ConversationModelApiResponses), "https://example.com/source", "https://name:password@example.com/"),
			strings.ReplaceAll(nativeFixture(protocol.ConversationModelApiResponses), "Generated answer", "Echo: private-access"),
			strings.ReplaceAll(nativeFixture(protocol.ConversationModelApiResponses), "Public source", "Echo: private-access"),
		} {
			search := nativeSearchFixture(protocol.ConversationModelApiResponses)
			search.Secrets = []protocol.Secret{protocol.NewSecret("private-access")}
			search.Transport = fixtureTransport(func(*http.Request) (*http.Response, error) { return responseFixture(body, "application/json"), nil })
			if _, err := SearchNative(context.Background(), search); err == nil || strings.Contains(err.Error(), "private-access") {
				t.Fatal("unsafe model output escaped", err)
			}
		}
	})
	t.Run("sanitizes provider errors without widening unsupported detection", func(t *testing.T) {
		for _, check := range []struct {
			message     string
			unsupported bool
		}{
			{"This model does not support web_search", true}, {"Unknown name \"googleSearch\" at tools[0]", true}, {"Google Search tool is not supported for this model", true}, {"max_tokens is not supported while using web_search", false}, {"controlled generation is not supported with google_search tool", false},
		} {
			value := map[string]any{"error": map[string]any{"message": check.message + " private-access"}}
			failure := nativeRejected(400, value, time.Now())
			if (failure.fault.Details != nil && failure.fault.Details.Kind == "search_unsupported") != check.unsupported || strings.Contains(failure.Error(), "private-access") {
				t.Fatal(check.message, failure)
			}
		}
		for _, code := range []string{"too_many_requests", "unavailable", "invalid_tool_input", "max_uses_exceeded"} {
			failure := nativeUpstream(map[string]any{"type": "web_search_tool_result_error", "error_code": code, "message": "private-access"}, time.Now())
			if strings.Contains(failure.Error(), "private-access") || (failure.retryAt != nil) != (code == "too_many_requests") {
				t.Fatal(failure)
			}
		}
	})
	t.Run("enriches public final sources with bounded concurrency", func(t *testing.T) {
		input := protocol.SearchRequest{Limit: 6, IncludeDomains: []string{"allowed.example"}, ExcludeDomains: []string{"private.allowed.example"}}
		results := []protocol.SearchResult{}
		for _, value := range []string{"https://other.example/source", "https://private.allowed.example/source", "https://allowed.example/first", "https://vertexaisearch.cloud.google.com/grounding-api-redirect/opaque", "https://allowed.example/third", "https://allowed.example/fourth", "https://allowed.example/fifth", "https://allowed.example/sixth"} {
			results = append(results, protocol.SearchResult{Url: value, Title: "Citation title"})
		}
		results = nativeSelect(results, input, time.Now())
		if len(results) != 6 || results[1].Url != "https://vertexaisearch.cloud.google.com/grounding-api-redirect/opaque" {
			t.Fatal("grounding redirect was filtered before final source read", results)
		}
		var active, maximum atomic.Int32
		gate := make(chan struct{})
		started := make(chan struct{}, 6)
		done := make(chan struct{})
		var enriched []protocol.SearchResult
		var enrichErr error
		go func() {
			defer close(done)
			enriched, enrichErr = nativeEnrich(context.Background(), results, func(ctx context.Context, _ protocol.WebRequest) (protocol.WebPage, error) {
				count := active.Add(1)
				defer active.Add(-1)
				for old := maximum.Load(); count > old; old = maximum.Load() {
					if maximum.CompareAndSwap(old, count) {
						break
					}
				}
				started <- struct{}{}
				select {
				case <-gate:
				case <-ctx.Done():
					return protocol.WebPage{}, ctx.Err()
				}
				return protocol.WebPage{Url: "https://allowed.example/final", Status: 200, Content: strings.Repeat("界", 4001)}, nil
			})
		}()
		for range 4 {
			<-started
		}
		close(gate)
		<-done
		if enrichErr != nil || len(enriched) != 1 || maximum.Load() > 4 || len([]rune(enriched[0].Snippet)) != 4000 || enriched[0].Title != "Citation title" {
			t.Fatal(enriched, enrichErr, maximum.Load())
		}
	})
	t.Run("protects fetched excerpts and respects cancellation", func(t *testing.T) {
		search := nativeSearchFixture(protocol.ConversationModelApiResponses)
		search.Secrets = []protocol.Secret{protocol.NewSecret("private-access")}
		search.Fetch = func(context.Context, protocol.WebRequest) (protocol.WebPage, error) {
			return protocol.WebPage{Url: "https://example.com/final", Status: 200, Content: "private-access"}, nil
		}
		if _, err := SearchNative(context.Background(), search); err == nil || strings.Contains(err.Error(), "private-access") {
			t.Fatal("fetched protected data escaped", err)
		}
		ctx, cancel := context.WithCancel(context.Background())
		cancel()
		if _, err := nativeEnrich(ctx, []protocol.SearchResult{{Url: "https://example.com/"}}, func(context.Context, protocol.WebRequest) (protocol.WebPage, error) {
			return protocol.WebPage{}, errors.New("unused")
		}); err == nil {
			t.Fatal("cancelled enrichment succeeded")
		}
	})
}
