package model

import (
	"bytes"
	"context"
	"io"
	"net/http"
	"net/http/httptest"
	"strconv"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"
)

func TestHTTP408RetriesOnlyThroughSailryPolicy(t *testing.T) {
	for _, kind := range []string{"responses", "chat", "anthropic", "google"} {
		t.Run(kind, func(t *testing.T) {
			success := fixture(t, kind+"-text.sse")
			var requests atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
				_, _ = io.Copy(io.Discard, request.Body)
				if requests.Add(1) == 1 {
					writer.Header().Set("Content-Type", "application/json")
					writer.Header().Set("Retry-After", "7")
					writer.WriteHeader(http.StatusRequestTimeout)
					_, _ = io.WriteString(writer, `{"error":{"code":"request_timeout","message":"Request timed out"}}`)
					return
				}
				writer.Header().Set("Content-Type", "text/event-stream")
				_, _ = writer.Write(success)
			}))
			t.Cleanup(server.Close)
			name := "fixture"
			if kind == "anthropic" {
				name = "claude-opus-4-8"
			}
			language, err := newSDK(t.Context(), transportConfig{Kind: kind, Model: name, Endpoint: server.URL, Key: "captured-key"})
			if err != nil {
				t.Fatal(err)
			}
			waits := 0
			timer := &policyClock{beforeSleep: func() {
				waits++
				if count := requests.Load(); count != 1 {
					t.Fatalf("SDK sent %d requests before Sailry's retry wait", count)
				}
			}}
			var observed []Attempt
			finished := false
			err = Run(t.Context(), language, requestCall(t, kind), Main, timer, func(attempt Attempt) error {
				observed = append(observed, attempt)
				return nil
			}, func(part fantasy.StreamPart) bool {
				finished = finished || part.Type == fantasy.StreamPartTypeFinish
				return true
			}, nil)
			if err != nil || !finished || requests.Load() != 2 || len(observed) != 2 || waits != 1 {
				t.Fatalf("HTTP 408 recovery: requests=%d attempts=%d waits=%d finished=%t error=%v", requests.Load(), len(observed), waits, finished, err)
			}
			first, last := observed[0], observed[1]
			if first.Failure == nil || first.Failure.Kind != Unavailable || first.Status == nil || *first.Status != 408 ||
				first.RetryDelay == nil || *first.RetryDelay != 7*time.Second || len(timer.delays) != 1 || timer.delays[0] != 7*time.Second ||
				last.Failure != nil || last.RetryDelay != nil || first.RequestID != last.RequestID || last.Index != 1 {
				t.Fatalf("HTTP 408 lost its policy, cause or request identity: %+v, %+v", first, last)
			}
		})
	}
}

func TestSDKPreheaderEOFRetriesTheSameRequest(t *testing.T) {
	for _, kind := range []string{"responses", "chat", "anthropic", "google"} {
		t.Run(kind, func(t *testing.T) {
			var requests atomic.Int32
			bodies := make(chan string, MainRetries+1)
			server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
				body, err := io.ReadAll(request.Body)
				if err != nil {
					t.Error(err)
				}
				bodies <- string(body)
				if requests.Add(1) == 1 {
					connection, _, err := writer.(http.Hijacker).Hijack()
					if err != nil {
						t.Error(err)
						return
					}
					defer connection.Close()
					_, _ = io.WriteString(connection, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n")
					return
				}
				writer.Header().Set("Content-Type", "text/event-stream")
				_, _ = writer.Write(fixture(t, kind+"-text.sse"))
			}))
			t.Cleanup(server.Close)
			name := "fixture"
			if kind == "anthropic" {
				name = "claude-opus-4-8"
			}
			language, err := newSDK(t.Context(), transportConfig{Kind: kind, Model: name, Endpoint: server.URL, Key: "fixture-key"})
			if err != nil {
				t.Fatal(err)
			}
			timer := &policyClock{beforeSleep: func() {
				if count := requests.Load(); count != 1 {
					t.Fatalf("SDK sent %d requests before Sailry's retry wait", count)
				}
			}}
			var attempts []Attempt
			output := ""
			err = Run(t.Context(), language, requestCall(t, kind), Main, timer, func(attempt Attempt) error {
				attempts = append(attempts, attempt)
				return nil
			}, func(part fantasy.StreamPart) bool {
				if part.Type == fantasy.StreamPartTypeTextDelta {
					output += part.Delta
				}
				return true
			}, nil)
			if err != nil || requests.Load() != 2 || len(attempts) != 2 || len(timer.delays) != 1 || output != "Read complete" {
				t.Fatalf("preheader EOF recovery: requests=%d attempts=%d waits=%v output=%q error=%v", requests.Load(), len(attempts), timer.delays, output, err)
			}
			first, last := attempts[0], attempts[1]
			if first.Failure == nil || first.Failure.Kind != Network || first.Failure.Message != "Model connection ended before response headers" || !first.Failure.Retryable() ||
				first.Status != nil || first.ResponseUs != nil || first.RetryDelay == nil || *first.RetryDelay != timer.delays[0] ||
				timer.delays[0] < 240*time.Millisecond || timer.delays[0] > 360*time.Millisecond ||
				last.Failure != nil || last.Status == nil || *last.Status != http.StatusOK || first.RequestID != last.RequestID || last.Index != 1 {
				t.Fatalf("preheader EOF lost its transport boundary or retry identity: %+v", attempts)
			}
			if first, second := <-bodies, <-bodies; first != second {
				t.Fatal("retry changed the captured request body")
			}
		})
	}
}

func TestSDKTruncatedStreamsRetryTheSameRequestAndResetOutput(t *testing.T) {
	prefixes := map[string]string{
		"responses": "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"message\",\"delta\":\"discard\"}\n\n",
		"chat":      "data: {\"id\":\"chat\",\"object\":\"chat.completion.chunk\",\"model\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"discard\"},\"finish_reason\":null}]}\n\n",
		"anthropic": "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n" +
			"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"discard\"}}\n\n",
		"google": "data: {\"candidates\":[{\"index\":0,\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"discard\"}]}}]}\n\n",
	}
	for kind, prefix := range prefixes {
		t.Run(kind, func(t *testing.T) {
			var requests atomic.Int32
			bodies := make(chan string, MainRetries+1)
			server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
				body, err := io.ReadAll(request.Body)
				if err != nil {
					t.Error(err)
				}
				bodies <- string(body)
				writer.Header().Set("Content-Type", "text/event-stream")
				if requests.Add(1) == 1 {
					_, _ = io.WriteString(writer, prefix)
					return
				}
				_, _ = writer.Write(fixture(t, kind+"-text.sse"))
			}))
			t.Cleanup(server.Close)
			language, err := newSDK(t.Context(), transportConfig{Kind: kind, Model: "fixture", Endpoint: server.URL, Key: "fixture-key"})
			if err != nil {
				t.Fatal(err)
			}
			output, resets := "", 0
			var attempts []Attempt
			err = Run(t.Context(), language, requestCall(t, kind), Main, &policyClock{}, func(attempt Attempt) error {
				attempts = append(attempts, attempt)
				return nil
			}, func(part fantasy.StreamPart) bool {
				if part.Type == fantasy.StreamPartTypeTextDelta {
					output += part.Delta
				}
				return true
			}, func() {
				if output != "discard" {
					t.Fatalf("SDK discarded the fixture's partial output before recovery: %q", output)
				}
				output = ""
				resets++
			})
			if err != nil || requests.Load() != 2 || len(attempts) != 2 || resets != 1 || output != "Read complete" {
				t.Fatalf("SDK stream recovery: requests=%d attempts=%d resets=%d output=%q error=%v", requests.Load(), len(attempts), resets, output, err)
			}
			if first := attempts[0]; first.Failure == nil || first.Failure.Kind != InvalidResponse || first.Failure.Message != "Model stream ended before a completion event" || !first.Failure.Retryable() || first.Status == nil || *first.Status != 200 || first.ResponseUs == nil || first.RetryDelay == nil || attempts[1].Failure != nil {
				t.Fatalf("SDK EOF lost its status, cause or retry observation: %+v", attempts)
			}
			if first, second := <-bodies, <-bodies; first != second {
				t.Fatal("retry changed the captured request body")
			}
		})
	}
}

func TestSDKTerminalEventsDoNotWaitForHTTPStreamClosure(t *testing.T) {
	for _, kind := range []string{"responses", "anthropic", "google", "chat"} {
		endings := []string{"idle", "truncated"}
		if kind == "responses" {
			endings = append(endings, "missing-separator")
		}
		for _, content := range []string{"text", "tool"} {
			for _, ending := range endings {
				t.Run(kind+"/"+content+"/"+ending, func(t *testing.T) {
					var payload []byte
					wantUsage := true
					wantOutput := uint64(3)
					if kind == "google" {
						// The fixture reports three visible and two reasoning tokens.
						wantOutput = 5
					}
					if kind == "chat" && content == "tool" {
						// The choice completed, but the optional usage trailer was lost.
						payload = []byte("data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"stable-call\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\\\"file\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n")
						wantUsage = false
					} else {
						payload = fixture(t, kind+"-"+content+".sse")
						if kind == "chat" {
							payload = bytes.TrimSuffix(payload, []byte("data: [DONE]\n\n"))
						}
					}
					if ending == "missing-separator" {
						payload = bytes.TrimRight(payload, "\r\n")
						if content == "tool" {
							item := `data: {"type":"response.output_item.done","item":{"type":"function_call","call_id":"stable-call","id":"item","name":"read","arguments":"{\"path\":\"file\"}"}}` + "\n\n"
							payload = append([]byte(item), payload...)
						}
					}
					var requests atomic.Int32
					closed := make(chan struct{}, MainRetries+1)
					server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
						_, _ = io.Copy(io.Discard, request.Body)
						requests.Add(1)
						writer.Header().Set("Content-Type", "text/event-stream")
						if ending == "truncated" {
							writer.Header().Set("Content-Length", strconv.Itoa(len(payload)+100))
						}
						_, _ = writer.Write(payload)
						writer.(http.Flusher).Flush()
						if ending == "idle" {
							<-request.Context().Done()
						}
						closed <- struct{}{}
					}))
					t.Cleanup(server.Close)
					language, err := newSDK(t.Context(), transportConfig{Kind: kind, Model: "fixture", Endpoint: server.URL, Key: "fixture-key"})
					if err != nil {
						t.Fatal(err)
					}
					// Bound the read of an optional trailer in the stalled fixture.
					sdk := language.(*sdkModel)
					transport := sdk.client.Transport.(responseTransport)
					transport.stall = 250 * time.Millisecond
					sdk.client.Transport = transport
					timer := &policyClock{}
					var attempts []Attempt
					text, calls, finishes, resets := "", 0, 0, 0
					err = Run(t.Context(), language, requestCall(t, kind), Main, timer, func(attempt Attempt) error {
						attempts = append(attempts, attempt)
						return nil
					}, func(part fantasy.StreamPart) bool {
						switch part.Type {
						case fantasy.StreamPartTypeTextDelta:
							text += part.Delta
						case fantasy.StreamPartTypeToolCall:
							calls++
							if part.ID != "stable-call" || part.ToolCallName != "read" || part.ToolCallInput != `{"path":"file"}` {
								t.Errorf("completed tool call changed: %+v", part)
							}
						case fantasy.StreamPartTypeFinish:
							finishes++
						}
						return true
					}, func() { resets++ })
					if err != nil || requests.Load() != 1 || len(attempts) != 1 || len(timer.delays) != 0 || resets != 0 || finishes != 1 {
						t.Fatalf("completed response retried or failed: requests=%d attempts=%d waits=%v resets=%d finishes=%d error=%v", requests.Load(), len(attempts), timer.delays, resets, finishes, err)
					}
					if content == "text" && (text != "Read complete" || calls != 0) || content == "tool" && calls != 1 {
						t.Fatalf("completed output changed: text=%q calls=%d", text, calls)
					}
					attempt := attempts[0]
					if attempt.Failure != nil || attempt.Status == nil || *attempt.Status != http.StatusOK || attempt.ResponseUs == nil {
						t.Fatalf("completed response lost status: %+v", attempt)
					}
					if got := attempt.Usage.Output; wantUsage && (got == nil || *got != wantOutput) || !wantUsage && got != nil {
						t.Fatalf("completed response lost or invented final usage: %+v", attempt.Usage)
					}
					select {
					case <-closed:
					case <-time.After(2 * time.Second):
						t.Fatal("completed stream left its HTTP request open")
					}
				})
			}
		}
	}
}

func TestResponsesTailWithoutTerminalRetries(t *testing.T) {
	for _, sample := range []struct {
		name, tail string
	}{
		{"cut-json", `{"type":"response.completed","response":`},
		{"missing-output", `{"type":"response.completed","response":{"status":"completed"}}`},
		{"mismatched-status", `{"type":"response.completed","response":{"status":"in_progress","output":[]}}`},
		{"item-only", `{"type":"response.output_item.done","item":{"type":"function_call","call_id":"partial","id":"item","name":"read","arguments":"{}"}}`},
		{"sentinel-only", `[DONE]`},
	} {
		t.Run(sample.name, func(t *testing.T) {
			server := serverFixture(t, "data: "+sample.tail, string(fixture(t, "responses-text.sse")))
			language, err := newSDK(t.Context(), transportConfig{Kind: "responses", Model: "fixture", Endpoint: server.URL, Key: "fixture-key"})
			if err != nil {
				t.Fatal(err)
			}
			var attempts []Attempt
			finished := false
			err = Run(t.Context(), language, requestCall(t, "responses"), Main, &policyClock{}, func(attempt Attempt) error {
				attempts = append(attempts, attempt)
				return nil
			}, func(part fantasy.StreamPart) bool {
				if part.Type == fantasy.StreamPartTypeToolCall {
					t.Error("an unfinished response produced a tool call")
				}
				finished = finished || part.Type == fantasy.StreamPartTypeFinish
				return true
			}, nil)
			if err != nil || !finished || len(attempts) != 2 || len(server.captured()) != 2 {
				t.Fatalf("partial response recovery: attempts=%+v finished=%t error=%v", attempts, finished, err)
			}
			if first := attempts[0]; first.Failure == nil || !first.Failure.truncated || first.RetryDelay == nil || attempts[1].Failure != nil {
				t.Fatalf("partial response lost its retry state: %+v", attempts)
			}
		})
	}
}

func TestSDKIncompleteToolCallIsRejected(t *testing.T) {
	for _, test := range []struct {
		kind, frame string
	}{
		{"chat", `{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call","function":{"name":"read","arguments":"{"}}]},"finish_reason":"tool_calls"}]}`},
		{"google", `{"candidates":[{"index":0,"content":{"role":"model","parts":[{"functionCall":{"name":"read","args":{},"willContinue":true}}]},"finishReason":"STOP"}]}`},
	} {
		t.Run(test.kind, func(t *testing.T) {
			server := serverFixture(t, "data: "+test.frame+"\n\n")
			language, err := newSDK(t.Context(), transportConfig{Kind: test.kind, Model: "fixture", Endpoint: server.URL, Key: "fixture-key"})
			if err != nil {
				t.Fatal(err)
			}
			err = Run(t.Context(), language, requestCall(t, test.kind), Main, &policyClock{}, nil, func(part fantasy.StreamPart) bool {
				if part.Type == fantasy.StreamPartTypeToolCall || part.Type == fantasy.StreamPartTypeFinish {
					t.Errorf("incomplete tool input reached the caller as %s", part.Type)
				}
				return true
			}, nil)
			if err == nil || len(server.captured()) != 1 {
				t.Fatalf("malformed tool call was accepted or replayed: %v", err)
			}
		})
	}
}

func TestSDKCancellationReleasesRequestWithoutRetry(t *testing.T) {
	prefixes := map[string]string{
		"responses": "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"message\",\"delta\":\"retain\"}\n\n",
		"chat":      "data: {\"id\":\"chat\",\"object\":\"chat.completion.chunk\",\"model\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"retain\"},\"finish_reason\":null}]}\n\n",
		"anthropic": "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n" +
			"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"retain\"}}\n\n",
		"google": "data: {\"candidates\":[{\"index\":0,\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"retain\"}]}}]}\n\n",
	}
	for kind, prefix := range prefixes {
		for _, phase := range []string{"before_headers", "partial_output"} {
			t.Run(kind+"/"+phase, func(t *testing.T) {
				ctx, cancel := context.WithCancel(t.Context())
				defer cancel()
				var requests atomic.Int32
				closed := make(chan struct{}, 1)
				server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
					_, _ = io.Copy(io.Discard, request.Body)
					requests.Add(1)
					if phase == "before_headers" {
						cancel()
					} else {
						writer.Header().Set("Content-Type", "text/event-stream")
						_, _ = io.WriteString(writer, prefix)
						writer.(http.Flusher).Flush()
					}
					<-request.Context().Done()
					closed <- struct{}{}
				}))
				t.Cleanup(server.Close)
				language, err := newSDK(ctx, transportConfig{Kind: kind, Model: "fixture", Endpoint: server.URL, Key: "fixture-key"})
				if err != nil {
					t.Fatal(err)
				}
				timer := &policyClock{}
				var attempts []Attempt
				output, resets, finishes := "", 0, 0
				err = Run(ctx, language, requestCall(t, kind), Main, timer, func(attempt Attempt) error {
					attempts = append(attempts, attempt)
					return nil
				}, func(part fantasy.StreamPart) bool {
					if part.Type == fantasy.StreamPartTypeTextDelta {
						output += part.Delta
						cancel()
					}
					if part.Type == fantasy.StreamPartTypeFinish {
						finishes++
					}
					return true
				}, func() { resets++ })
				failure := Classify(err)
				if failure == nil || failure.Kind != Cancelled || requests.Load() != 1 || len(attempts) != 1 || len(timer.delays) != 0 || resets != 0 || finishes != 0 {
					t.Fatalf("cancellation retried or completed: failure=%+v requests=%d attempts=%d waits=%v resets=%d finishes=%d", failure, requests.Load(), len(attempts), timer.delays, resets, finishes)
				}
				attempt := attempts[0]
				if attempt.Failure == nil || attempt.Failure.Kind != Cancelled || attempt.RetryDelay != nil || phase == "before_headers" && (attempt.Status != nil || attempt.ResponseUs != nil) || phase == "partial_output" && (output != "retain" || attempt.Status == nil || *attempt.Status != http.StatusOK || attempt.ResponseUs == nil) {
					t.Fatalf("cancellation changed output or response boundary: output=%q attempt=%+v", output, attempt)
				}
				select {
				case <-closed:
				case <-time.After(2 * time.Second):
					t.Fatal("cancelled stream left its HTTP request open")
				}
			})
		}
	}
}
