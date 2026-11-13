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
			})
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
