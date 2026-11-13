package model

import (
	"errors"
	"io"
	"net/http"
	"slices"
	"strings"
	"testing"

	"charm.land/fantasy"
)

type failedPluginBody struct{}

func (failedPluginBody) Read([]byte) (int, error) { return 0, errors.New("fixture read failed") }
func (failedPluginBody) Close() error             { return nil }

func TestPluginRetryHintsAcrossResponseFailures(t *testing.T) {
	const prepare = `export function prepare(input) { return {method:"POST",url:input.endpoint+"/messages",body:"{}"}; }`
	const decode = `export function decode(input) { return {state:null,deltas:input.events.map(event=>JSON.parse(event.data))}; }`
	const classify = `export function classify(input) { return {class:"unavailable",message:"busy",retry_after_ms:9000,code:"overloaded_error"}; }`
	for _, test := range []struct {
		name       string
		status     int
		script     string
		body       string
		readFails  bool
		kind       FailureKind
		overloaded bool
		hint       bool
	}{
		{"HTTP fallback", 503, decode, `{}`, false, Unavailable, false, false},
		{"HTTP overload fallback", 529, decode, `{}`, false, Unavailable, true, false},
		{"classified HTTP", 503, decode + classify, `{}`, false, Unavailable, true, true},
		{"classified generic HTTP", 503, decode + `export function classify(input) { return {class:"unavailable",message:"unavailable"}; }`, `{}`, false, Unavailable, false, false},
		{"classified quota", 429, decode + `export function classify(input) { return {class:"quota",message:"quota",retry_after_ms:9000,code:"overloaded_error"}; }`, `{}`, false, Quota, false, true},
		{"stream rate limit", 200, decode, "data: {\"kind\":\"error\",\"class\":\"rate_limited\",\"message\":\"rate limited\",\"retry_after_ms\":9000}\n\n", false, RateLimited, true, true},
		{"stream busy code", 200, decode, "data: {\"kind\":\"error\",\"class\":\"unavailable\",\"message\":\"busy\",\"retry_after_ms\":9000,\"code\":\"server_busy\"}\n\n", false, Unavailable, true, true},
		{"stream overload type", 200, decode, "data: {\"kind\":\"error\",\"class\":\"unavailable\",\"message\":\"busy\",\"retry_after_ms\":9000,\"error_type\":\"overloaded_error\"}\n\n", false, Unavailable, true, true},
		{"stream generic server error", 200, decode, "data: {\"kind\":\"error\",\"class\":\"unavailable\",\"message\":\"failed\",\"code\":\"internal_error\"}\n\n", false, Unavailable, false, false},
		{"stream authentication", 200, decode, "data: {\"kind\":\"error\",\"class\":\"authentication\",\"message\":\"failed\",\"retry_after_ms\":9000,\"code\":\"overloaded_error\"}\n\n", false, Authentication, false, true},
		{"invalid framing", 200, decode, "data: \xff\n\n", false, InvalidRequest, false, false},
		{"decode failure", 200, `export function decode(input) { throw Error("fixture decode failed"); }`, "data: {}\n\n", false, InvalidRequest, false, false},
		{"invalid decoded response", 200, `export function decode(input) { return {state:null,deltas:[{kind:"unknown"}]}; }`, "data: {}\n\n", false, InvalidRequest, false, false},
		{"incomplete stream", 200, decode, "", false, InvalidResponse, false, false},
		{"stream read failure", 200, decode, "", true, Network, false, false},
		{"HTTP body read failure", 503, decode, "", true, Network, false, false},
		{"HTTP classify failure", 503, decode + `export function classify(input) { throw Error("fixture classification failed"); }`, `{}`, false, InvalidRequest, false, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			model, binding, _ := pluginFixture(t, prepare+test.script, func(*http.Request) (*http.Response, error) {
				var body io.ReadCloser = io.NopCloser(strings.NewReader(test.body))
				if test.readFails {
					body = failedPluginBody{}
				}
				return &http.Response{StatusCode: test.status, Header: http.Header{"Retry-After": {"4", "8"}, "Content-Type": {"text/event-stream"}}, Body: body}, nil
			})
			call, err := BuildCall(binding, Request{System: "fixture"})
			if err != nil {
				t.Fatal(err)
			}
			stream, err := model.Stream(t.Context(), call)
			if err != nil {
				t.Fatal(err)
			}
			var failure *Failure
			for part := range stream {
				if part.Type == fantasy.StreamPartTypeError {
					failure = Classify(part.Error)
				}
			}
			if failure == nil || failure.Kind != test.kind || failure.overloaded != test.overloaded {
				t.Fatalf("response failure changed classification: expected kind=%s overloaded=%t", test.kind, test.overloaded)
			}
			if !slices.Equal(failure.retryAfter, []string{"4", "8"}) || failure.status == nil || *failure.status != uint16(test.status) {
				t.Fatal("response failure lost its raw HTTP advice or status")
			}
			if test.hint != (failure.retryAfterMs != nil) || test.hint && *failure.retryAfterMs != 9000 {
				t.Fatal("plugin millisecond advice was lost or substituted for HTTP advice")
			}
		})
	}
}

func TestPluginPreparationRetryHint(t *testing.T) {
	requests := 0
	model, binding, _ := pluginFixture(t, `export function prepare(input) { return {refusal:{class:"rate_limited",message:"busy",retry_after_ms:9000}}; }`, func(*http.Request) (*http.Response, error) {
		requests++
		return nil, errors.New("unexpected HTTP request")
	})
	call, err := BuildCall(binding, Request{System: "fixture"})
	if err != nil {
		t.Fatal(err)
	}
	stream, err := model.Stream(t.Context(), call)
	if err != nil {
		t.Fatal(err)
	}
	var failure *Failure
	for part := range stream {
		if part.Type == fantasy.StreamPartTypeError {
			failure = Classify(part.Error)
		}
	}
	if requests != 0 || failure == nil || failure.Kind != RateLimited || !failure.overloaded || failure.retryAfterMs == nil || *failure.retryAfterMs != 9000 || len(failure.retryAfter) != 0 {
		t.Fatal("preparation refusal sent HTTP or lost its own retry advice")
	}
}
