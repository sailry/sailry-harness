package model

import (
	"bytes"
	"context"
	"errors"
	"io"
	"net/http"
	"reflect"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/clock"
	"github.com/sailry/sailry-harness/internal/node/agent/observation"
)

func responsesLongHistoryFixture(tb testing.TB) []byte {
	tb.Helper()
	const historyCount = 1024
	const content = "Earlier discussion detail retained in the conversation history. "
	prompt := make(fantasy.Prompt, 0, historyCount)
	for index := range historyCount {
		role := fantasy.MessageRoleUser
		if index%2 == 1 {
			role = fantasy.MessageRoleAssistant
		}
		prompt = append(prompt, fantasy.Message{
			Role:    role,
			Content: []fantasy.MessagePart{fantasy.TextPart{Text: strings.Repeat(content, 4)}},
		})
	}
	call := fantasy.Call{Prompt: prompt}
	config := transportConfig{Kind: "responses", Model: "fixture-model"}
	body, err := requestJSON(config, call)
	if err != nil {
		tb.Fatalf("build Responses request: %v", err)
	}
	raw, err := sourceRequestBytes(config, call, body)
	if err != nil {
		tb.Fatalf("encode Responses request: %v", err)
	}
	return raw
}

func newResponsesRoundTripFixture(tb testing.TB, raw []byte) (responseTransport, *http.Request, *attemptCapture) {
	tb.Helper()
	ctx, capture := captureAttempt(context.Background(), fantasy.Call{}, clock.System{}, time.Now())
	capture.trace = &observation.ResponsesTrace{}
	ctx = context.WithValue(ctx, factsKey{}, &responseFacts{family: "responses"})
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, "https://api.example.test/v1/responses", nil)
	if err != nil {
		tb.Fatalf("build fixture request: %v", err)
	}
	request.ContentLength = int64(len(raw))
	request.GetBody = func() (io.ReadCloser, error) {
		return io.NopCloser(bytes.NewReader(raw)), nil
	}
	request.Body = io.NopCloser(bytes.NewReader(raw))
	base := sdkFixtureRoundTripper(func(request *http.Request) (*http.Response, error) {
		if _, err := io.Copy(io.Discard, request.Body); err != nil {
			return nil, err
		}
		request.Body.Close()
		return &http.Response{
			StatusCode: http.StatusOK,
			Header:     make(http.Header),
			Body:       io.NopCloser(strings.NewReader("")),
			Request:    request,
		}, nil
	})
	return responseTransport{base: base, stall: time.Hour}, request, capture
}

type sdkFixtureRoundTripper func(*http.Request) (*http.Response, error)

func (roundTrip sdkFixtureRoundTripper) RoundTrip(request *http.Request) (*http.Response, error) {
	return roundTrip(request)
}

func BenchmarkSDKRoundTripPreparedLongHistory(b *testing.B) {
	raw := responsesLongHistoryFixture(b)
	for _, fast := range []bool{false, true} {
		name := "getbody_copy"
		if fast {
			name = "prepared_snapshot"
		}
		b.Run(name, func(b *testing.B) {
			transport, request, _ := newResponsesRoundTripFixture(b, raw)
			baseContext := request.Context()
			var preflightStarted time.Time
			var preflight time.Duration
			transport.base = sdkFixtureRoundTripper(func(request *http.Request) (*http.Response, error) {
				preflight += time.Since(preflightStarted)
				request.Body.Close()
				return &http.Response{
					StatusCode: http.StatusOK,
					Header:     make(http.Header),
					Body:       io.NopCloser(strings.NewReader("")),
					Request:    request,
				}, nil
			})
			b.ReportAllocs()
			b.SetBytes(int64(len(raw)))
			b.ResetTimer()
			for range b.N {
				request.Body = io.NopCloser(bytes.NewReader(raw))
				if fast {
					request = request.WithContext(baseContext)
					request = withPreparedRequestObservation(request, raw)
				}
				preflightStarted = time.Now()
				response, err := transport.RoundTrip(request)
				if err != nil {
					b.Fatal(err)
				}
				response.Body.Close()
			}
			b.StopTimer()
			b.ReportMetric(float64(preflight.Nanoseconds())/float64(b.N), "preflight-ns/op")
		})
	}
}

func TestResponseTransportUsesPreparedRequestSnapshot(t *testing.T) {
	t.Parallel()
	raw := []byte(`{"model":"fixture-model","stream":true,"input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"history"}]}]}`)
	transport, request, capture := newResponsesRoundTripFixture(t, raw)
	var sent []byte
	transport.base = sdkFixtureRoundTripper(func(request *http.Request) (*http.Response, error) {
		var err error
		sent, err = io.ReadAll(request.Body)
		request.Body.Close()
		if err != nil {
			return nil, err
		}
		return &http.Response{StatusCode: http.StatusOK, Header: make(http.Header), Body: io.NopCloser(strings.NewReader("")), Request: request}, nil
	})
	request.GetBody = func() (io.ReadCloser, error) {
		return nil, errors.New("GetBody called for the original prepared request")
	}
	request = withPreparedRequestObservation(request, raw)
	response, err := transport.RoundTrip(request)
	if err != nil {
		t.Fatal(err)
	}
	response.Body.Close()
	if !bytes.Equal(sent, raw) {
		t.Fatalf("sent request changed: got %d bytes, want %d", len(sent), len(raw))
	}
	wantPrepared, wantShape := (&observation.ResponsesTrace{}).Prepare(raw, nil)
	if !reflect.DeepEqual(capture.prepared, wantPrepared) || !reflect.DeepEqual(capture.shape, wantShape) {
		t.Fatal("prepared observation differs from the original Responses JSON")
	}
	if capture.requestBytes == nil || *capture.requestBytes != uint64(len(raw)) {
		t.Fatalf("request byte observation = %v, want %d", capture.requestBytes, len(raw))
	}
}

func TestSDKResponsesStreamKeepsPreparedRequestSnapshot(t *testing.T) {
	t.Parallel()
	config := transportConfig{
		Kind:     "responses",
		Model:    "fixture-model",
		Endpoint: "https://api.example.test/v1",
		Key:      "fixture-key",
	}
	call := fantasy.Call{Prompt: fantasy.Prompt{{
		Role:    fantasy.MessageRoleUser,
		Content: []fantasy.MessagePart{fantasy.TextPart{Text: "history"}},
	}}}
	body, err := requestJSON(config, call)
	if err != nil {
		t.Fatal(err)
	}
	raw, err := sourceRequestBytes(config, call, body)
	if err != nil {
		t.Fatal(err)
	}
	responseStream := "data: " + `{"type":"response.completed","response":{"id":"response","status":"completed","output":[{"id":"message","type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]}]}}` + "\n\n"
	transport := sdkFixtureRoundTripper(func(request *http.Request) (*http.Response, error) {
		prepared, ok := preparedRequestObservationBody(request)
		if !ok || !bytes.Equal(prepared, raw) {
			t.Errorf("SDK request did not retain its prepared observation snapshot")
		}
		sent, err := io.ReadAll(request.Body)
		request.Body.Close()
		if err != nil {
			return nil, err
		}
		if !bytes.Equal(sent, raw) {
			t.Errorf("SDK sent %d bytes, want the original %d-byte request", len(sent), len(raw))
		}
		return &http.Response{
			StatusCode: http.StatusOK,
			Header:     http.Header{"Content-Type": []string{"text/event-stream"}},
			Body:       io.NopCloser(strings.NewReader(responseStream)),
			Request:    request,
		}, nil
	})
	model := &sdkModel{
		config: config,
		client: &http.Client{
			Transport:     responseTransport{base: transport, stall: time.Hour},
			CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse },
		},
	}
	ctx, capture := captureAttempt(context.Background(), call, clock.System{}, time.Now())
	capture.trace = &observation.ResponsesTrace{}
	stream, err := model.Stream(ctx, call)
	if err != nil {
		t.Fatal(err)
	}
	for part := range stream {
		if part.Type == fantasy.StreamPartTypeError {
			t.Fatal(part.Error)
		}
	}
	wantPrepared, wantShape := (&observation.ResponsesTrace{}).Prepare(raw, nil)
	if !reflect.DeepEqual(capture.prepared, wantPrepared) || !reflect.DeepEqual(capture.shape, wantShape) {
		t.Fatal("SDK stream observation differs from the original Responses JSON")
	}
	if capture.requestBytes == nil || *capture.requestBytes != uint64(len(raw)) {
		t.Fatalf("request byte observation = %v, want %d", capture.requestBytes, len(raw))
	}
}

func TestResponseTransportSnapshotDoesNotReplaceChangedRequests(t *testing.T) {
	t.Parallel()
	raw := []byte(`{"model":"fixture","input":[]}`)
	changed := []byte(`{"model":"changed","input":[]}`)
	tests := []struct {
		name       string
		method     string
		targetHost string
		body       []byte
	}{
		{name: "cross-domain redirect", method: http.MethodPost, targetHost: "redirect.example.test", body: raw},
		{name: "method-changing redirect", method: http.MethodGet, targetHost: "api.example.test"},
		{name: "modified SDK body", method: http.MethodPost, targetHost: "api.example.test", body: changed},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			transport, request, capture := newResponsesRoundTripFixture(t, raw)
			request = withPreparedRequestObservation(request, raw)
			request.Method = test.method
			request.URL.Host = test.targetHost
			request.ContentLength = int64(len(test.body))
			getBodyCalls := 0
			if len(test.body) == 0 {
				request.Body = http.NoBody
				request.GetBody = nil
			} else {
				request.Body = io.NopCloser(bytes.NewReader(test.body))
				request.GetBody = func() (io.ReadCloser, error) {
					getBodyCalls++
					return io.NopCloser(bytes.NewReader(test.body)), nil
				}
			}
			var sent []byte
			transport.base = sdkFixtureRoundTripper(func(request *http.Request) (*http.Response, error) {
				var err error
				sent, err = io.ReadAll(request.Body)
				request.Body.Close()
				if err != nil {
					return nil, err
				}
				return &http.Response{StatusCode: http.StatusOK, Header: make(http.Header), Body: io.NopCloser(strings.NewReader("")), Request: request}, nil
			})
			response, err := transport.RoundTrip(request)
			if err != nil {
				t.Fatal(err)
			}
			response.Body.Close()
			if !bytes.Equal(sent, test.body) {
				t.Fatalf("sent request body = %q, want %q", sent, test.body)
			}
			if len(test.body) == 0 {
				if getBodyCalls != 0 {
					t.Fatalf("GetBody calls = %d, want 0 for the empty method-changing redirect", getBodyCalls)
				}
				if capture.requestBytes != nil {
					t.Fatalf("empty redirected request retained original request byte count: %d", *capture.requestBytes)
				}
				return
			}
			if getBodyCalls != 1 {
				t.Fatalf("GetBody calls = %d, want fallback for the changed request", getBodyCalls)
			}
			wantPrepared, wantShape := (&observation.ResponsesTrace{}).Prepare(test.body, nil)
			if !reflect.DeepEqual(capture.prepared, wantPrepared) || !reflect.DeepEqual(capture.shape, wantShape) {
				t.Fatal("prepared observation does not describe the changed request")
			}
			if capture.requestBytes == nil || *capture.requestBytes != uint64(len(test.body)) {
				t.Fatalf("request byte observation = %v, want %d", capture.requestBytes, len(test.body))
			}
		})
	}
}
