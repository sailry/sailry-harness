package model

import (
	"errors"
	"net/url"
	"strings"
	"testing"

	"charm.land/fantasy"
)

func TestStreamFailureKeepsTransportCause(t *testing.T) {
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
