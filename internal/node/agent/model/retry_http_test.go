package model

import (
	"io"
	"net/http"
	"net/http/httptest"
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
				timer.delays[0] < 800*time.Millisecond || timer.delays[0] > 1200*time.Millisecond ||
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
