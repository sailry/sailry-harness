package model

import (
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"testing"

	"charm.land/fantasy"
)

func TestResponsesStreamFailureKeepsProviderMessage(t *testing.T) {
	t.Parallel()
	const diagnostic = "The service is temporarily unavailable. Please retry later."
	for _, message := range []string{diagnostic, diagnostic + " (request id: fixture-request)"} {
		event := fmt.Sprintf(`data: {"type":"response.failed","response":{"error":{"code":"server_error","message":%q}}}`, message)
		server := serverFixture(t, event+"\n\n")
		language, err := newSDK(t.Context(), transportConfig{Kind: "responses", Model: "fixture", Endpoint: server.URL, Key: "fixture-key"})
		if err != nil {
			t.Fatal(err)
		}
		_, err = language.Generate(t.Context(), requestCall(t, "responses"))
		failure := Classify(err)
		if failure == nil || failure.Kind != Unavailable || !failure.Retryable() || failure.status == nil || *failure.status != http.StatusOK {
			t.Fatalf("lost stream failure or HTTP status: %+v", failure)
		}
		if failure.Message != message {
			t.Errorf("message = %q, want %q", failure.Message, message)
		}
	}
}

func TestStreamFailureKeepsTransportCause(t *testing.T) {
	t.Parallel()
	cause := &url.Error{Op: "Post", URL: "https://private.example/v1/responses", Err: errors.New("read tcp: connection reset by peer")}
	failure := Classify(&fantasy.ProviderError{Message: "model stream failed", Cause: cause})
	if failure.Message != "model stream failed: read tcp: connection reset by peer" || strings.Contains(failure.Message, "private.example") {
		t.Fatalf("message %q", failure.Message)
	}
	withBody := Classify(&fantasy.ProviderError{Message: "model stream failed", Cause: cause, StatusCode: 500, ResponseBody: []byte(`{"error":{"message":"boom"}}`)})
	if withBody.Message != "boom (HTTP 500)" {
		t.Fatalf("message with body %q", withBody.Message)
	}
}
