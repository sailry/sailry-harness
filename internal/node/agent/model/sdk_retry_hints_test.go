package model

import (
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"slices"
	"testing"

	"charm.land/fantasy"
)

func TestSDKResponseFailuresRetainAllRetryAfterValues(t *testing.T) {
	for _, kind := range []string{"responses", "chat", "anthropic", "google"} {
		for _, status := range []int{503, 200} {
			name := kind + "/HTTP"
			if status == 200 {
				name = kind + "/malformed_stream"
			}
			t.Run(name, func(t *testing.T) {
				server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
					_, _ = io.Copy(io.Discard, request.Body)
					writer.Header().Add("Retry-After", "4")
					writer.Header().Add("Retry-After", "Wed, 21 Oct 2037 07:28:00 GMT")
					if status == 503 {
						writer.Header().Set("Content-Type", "application/json")
						writer.WriteHeader(status)
						_, _ = io.WriteString(writer, `{"error":{"code":"overloaded_error","message":"busy"}}`)
					} else {
						writer.Header().Set("Content-Type", "text/event-stream")
						_, _ = io.WriteString(writer, "data: {broken\n\n")
					}
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
				stream, err := language.Stream(t.Context(), requestCall(t, kind))
				if err != nil {
					t.Fatal(err)
				}
				var caught error
				for part := range stream {
					if part.Type == fantasy.StreamPartTypeError {
						caught = part.Error
					}
				}
				var provider *fantasy.ProviderError
				if caught == nil || !errors.As(caught, &provider) || provider.StatusCode != status {
					t.Fatal("response failure lost its inspectable provider cause")
				}
				failure := Classify(caught)
				if !slices.Equal(failure.retryAfter, []string{"4", "Wed, 21 Oct 2037 07:28:00 GMT"}) {
					t.Fatal("response failure lost a Retry-After value")
				}
				if status == 503 && (failure.Kind != Unavailable || !failure.overloaded) || status == 200 && (failure.Kind != InvalidResponse || failure.Retryable() || failure.overloaded) {
					t.Fatal("response advice changed the HTTP or malformed-stream classification")
				}
			})
		}
	}
}
