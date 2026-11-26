package model

import (
	"bytes"
	"context"
	"crypto/tls"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"net/url"
	"reflect"
	"strings"
	"sync/atomic"
	"testing"
	"testing/iotest"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"golang.org/x/net/http2"
	"golang.org/x/net/http2/hpack"
)

func TestHTTP2TransportFailureClassification(t *testing.T) {
	t.Parallel()
	protocol := http2.StreamError{StreamID: 1, Code: http2.ErrCodeProtocol, Cause: errors.New("received from peer")}
	internal := http2.StreamError{StreamID: 559, Code: http2.ErrCodeInternal, Cause: errors.New("received from peer")}
	for _, cause := range []error{
		internal,
		protocol,
		http2.StreamError{StreamID: 3, Code: http2.ErrCodeRefusedStream},
		http2.StreamError{StreamID: 3, Code: http2.ErrCodeCancel, Cause: errors.New("received from peer")},
		http2.StreamError{StreamID: 3, Code: http2.ErrCodeHTTP11Required},
		http2.StreamError{StreamID: 3, Code: http2.ErrCode(0xab)},
		fmt.Errorf("wrapped: %w", protocol),
		errors.New("http2: client connection lost"),
		fmt.Errorf("read: %w", errors.New("http2: client connection lost")),
	} {
		t.Run(cause.Error(), func(t *testing.T) {
			for _, err := range []error{cause, &fantasy.ProviderError{Message: "model stream failed", StatusCode: 200, Cause: cause}} {
				if failure := Classify(err); failure.Kind != Network || !failure.Retryable() || !strings.Contains(failure.Message, cause.Error()) {
					t.Fatalf("HTTP/2 transport failure lost its retryable cause: %+v", failure)
				}
			}
		})
	}
	for _, test := range []struct {
		err      error
		fallback bool
	}{
		{protocol, true},
		{internal, false},
		{http2.StreamError{StreamID: 3, Code: http2.ErrCodeHTTP11Required}, true},
		{&fantasy.ProviderError{Cause: protocol}, true},
		{fmt.Errorf("wrapped: %w", protocol), true},
		{http2.ConnectionError(http2.ErrCodeProtocol), false},
		{errors.New(protocol.Error()), false},
		{http2.StreamError{StreamID: 1, Code: http2.ErrCodeRefusedStream}, false},
		{errors.New("http2: client connection lost"), false},
		{errors.New("PROTOCOL_ERROR"), false},
	} {
		if http2ProtocolFailure(test.err) != test.fallback {
			t.Errorf("protocol fallback for %v = %t", test.err, !test.fallback)
		}
	}
	provider := &fantasy.ProviderError{StatusCode: 200, ResponseBody: []byte(`{"error":{"type":"api_error","message":"stream error: stream ID 1; PROTOCOL_ERROR; received from peer"}}`)}
	for _, cause := range []error{internal, errors.New(protocol.Error()), protocol, errors.New("http2: client connection lost")} {
		provider.Cause = cause
		if failure := Classify(provider); failure.Kind != InvalidResponse || failure.Retryable() || http2ProtocolFailure(provider) {
			t.Fatalf("provider diagnostic was treated as a local HTTP/2 failure: %+v", failure)
		}
	}
}

func TestResponsesRetriesHTTP2InternalReset(t *testing.T) {
	t.Parallel()
	var calls atomic.Int32
	success := fixture(t, "responses-text.sse")
	consumed := make(chan struct{})
	server := httptest.NewUnstartedServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.ProtoMajor != 2 {
			t.Error("a peer internal error should retry over HTTP/2")
		}
		w.Header().Set("Content-Type", "text/event-stream")
		if calls.Add(1) == 1 {
			_, _ = io.WriteString(w, "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"message\",\"delta\":\"Discarded\"}\n\n")
			w.(http.Flusher).Flush()
			select {
			case <-consumed:
			case <-r.Context().Done():
			}
			panic(http.ErrAbortHandler)
		}
		_, _ = w.Write(success)
	}))
	server.EnableHTTP2 = true
	server.StartTLS()
	defer server.Close()
	language, err := newSDK(t.Context(), transportConfig{Kind: "responses", Model: "fixture", Endpoint: server.URL, Key: "fixture-key"})
	if err != nil {
		t.Fatal(err)
	}
	language.(*sdkModel).client.Transport = responseTransport{base: server.Client().Transport, stall: stallTimeout}
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	var attempts []Attempt
	var text string
	var resets int
	err = Run(ctx, language, requestCall(t, "responses"), Main, &policyClock{}, func(a Attempt) error {
		attempts = append(attempts, a)
		return nil
	}, func(p fantasy.StreamPart) bool {
		if p.Type == fantasy.StreamPartTypeTextDelta {
			text += p.Delta
			if p.Delta == "Discarded" {
				close(consumed)
			}
		}
		return true
	}, func() { text = ""; resets++ })
	if err != nil || len(attempts) != 2 || calls.Load() != 2 || text != "Read complete" || resets != 1 {
		t.Fatalf("peer internal reset: err=%v attempts=%d requests=%d text=%q resets=%d", err, len(attempts), calls.Load(), text, resets)
	}
	first := attempts[0]
	if first.Failure == nil || first.Failure.Kind != Network || !strings.Contains(first.Failure.Message, "INTERNAL_ERROR; received from peer") || first.RetryDelay == nil || first.RequestID != attempts[1].RequestID || attempts[1].Failure != nil {
		t.Fatalf("unexpected recovery: %+v", attempts)
	}
}

type protocolRequest struct {
	body       string
	major      int
	connection int32
	stream     uint32
}

func protocolResetServer(t *testing.T, success []byte, prefix string, firstHTTP1Status int) (*httptest.Server, <-chan protocolRequest, chan<- struct{}) {
	t.Helper()
	requests := make(chan protocolRequest, 2*(MainRetries+1))
	read := make(chan struct{}, 2)
	var replied atomic.Bool
	server := httptest.NewUnstartedServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		body, err := io.ReadAll(request.Body)
		if err != nil {
			t.Error(err)
		}
		requests <- protocolRequest{body: string(body), major: request.ProtoMajor}
		if firstHTTP1Status != 0 && !replied.Swap(true) {
			writer.Header().Set("Content-Type", "application/json")
			writer.Header().Set("Retry-After", "4")
			writer.WriteHeader(firstHTTP1Status)
			_, _ = io.WriteString(writer, `{"error":{"message":"Try again later"}}`)
			return
		}
		if success == nil {
			writer.Header().Set("Content-Type", "application/json")
			writer.WriteHeader(http.StatusBadGateway)
			_, _ = io.WriteString(writer, `{"error":{"code":"upstream_bad_gateway","message":"Gateway unavailable"}}`)
			return
		}
		writer.Header().Set("Content-Type", "text/event-stream")
		_, _ = writer.Write(success)
	}))
	var connections atomic.Int32
	server.EnableHTTP2 = true
	server.TLS = &tls.Config{NextProtos: []string{"h2", "http/1.1"}}
	server.Config.TLSNextProto = map[string]func(*http.Server, *tls.Conn, http.Handler){"h2": func(_ *http.Server, connection *tls.Conn, _ http.Handler) {
		defer connection.Close()
		if err := connection.SetDeadline(time.Now().Add(5 * time.Second)); err != nil {
			t.Error(err)
			return
		}
		id := connections.Add(1)
		preface := make([]byte, len(http2.ClientPreface))
		if _, err := io.ReadFull(connection, preface); err != nil || string(preface) != http2.ClientPreface {
			t.Errorf("HTTP/2 preface = %q, error=%v", preface, err)
			return
		}
		framer := http2.NewFramer(connection, connection)
		framer.ReadMetaHeaders = hpack.NewDecoder(4096, nil)
		// The reset fixture reads the entire body, including large model requests.
		if err := framer.WriteSettings(http2.Setting{ID: http2.SettingInitialWindowSize, Val: 2 * largeHTTP1BodyBytes}); err != nil {
			t.Error(err)
			return
		}
		if err := framer.WriteWindowUpdate(0, 2*largeHTTP1BodyBytes); err != nil {
			t.Error(err)
			return
		}
		bodies := map[uint32]*bytes.Buffer{}
		for {
			frame, err := framer.ReadFrame()
			if err != nil {
				return
			}
			stream, ended := frame.Header().StreamID, false
			switch frame := frame.(type) {
			case *http2.SettingsFrame:
				if !frame.IsAck() {
					if err := framer.WriteSettingsAck(); err != nil {
						return
					}
				}
			case *http2.MetaHeadersFrame:
				bodies[stream] = &bytes.Buffer{}
				ended = frame.StreamEnded()
			case *http2.DataFrame:
				if body := bodies[stream]; body != nil {
					body.Write(frame.Data())
				}
				ended = frame.StreamEnded()
			}
			if !ended {
				continue
			}
			requests <- protocolRequest{body: bodies[stream].String(), major: 2, connection: id, stream: stream}
			if prefix != "" {
				var block bytes.Buffer
				encoder := hpack.NewEncoder(&block)
				for _, field := range []hpack.HeaderField{{Name: ":status", Value: "200"}, {Name: "content-type", Value: "text/event-stream"}} {
					if err := encoder.WriteField(field); err != nil {
						t.Error(err)
						return
					}
				}
				if err := framer.WriteHeaders(http2.HeadersFrameParam{StreamID: stream, BlockFragment: block.Bytes(), EndHeaders: true}); err != nil {
					return
				}
				if err := framer.WriteData(stream, false, []byte(prefix)); err != nil {
					return
				}
				select {
				case <-read:
				case <-t.Context().Done():
					return
				case <-time.After(5 * time.Second):
					t.Error("HTTP/2 fixture output was not consumed before its reset")
					return
				}
			}
			if err := framer.WriteRSTStream(stream, http2.ErrCodeProtocol); err != nil {
				return
			}
		}
	}}
	server.StartTLS()
	t.Cleanup(server.Close)
	return server, requests, read
}

func TestHTTP2ConnectionLossAfterChatFinish(t *testing.T) {
	t.Parallel()
	const chunk = `data: {"choices":[{"index":0,"delta":{"content":"Answer"},"finish_reason":"stop"}]}` + "\n\n"
	language, err := newSDK(t.Context(), transportConfig{Kind: "chat", Model: "fixture", Endpoint: "https://model.invalid", Key: "fixture-key"})
	if err != nil {
		t.Fatal(err)
	}
	requests := 0
	language.(*sdkModel).client.Transport = responseTransport{base: sdkFixtureRoundTripper(func(r *http.Request) (*http.Response, error) {
		requests++
		return &http.Response{StatusCode: http.StatusOK, Request: r, Header: http.Header{"Content-Type": {"text/event-stream"}}, Body: io.NopCloser(io.MultiReader(strings.NewReader(chunk), iotest.ErrReader(errors.New("http2: client connection lost"))))}, nil
	}), stall: stallTimeout}
	var text string
	var finishes int
	err = Run(t.Context(), language, requestCall(t, "chat"), Main, &policyClock{}, nil, func(part fantasy.StreamPart) bool {
		if part.Type == fantasy.StreamPartTypeTextDelta {
			text += part.Delta
		}
		if part.Type == fantasy.StreamPartTypeFinish {
			finishes++
		}
		return true
	}, func() { t.Fatal("replayed a completed choice") })
	if err != nil || requests != 1 || finishes != 1 || text != "Answer" {
		t.Fatalf("lost optional trailer: err=%v requests=%d finishes=%d text=%q", err, requests, finishes, text)
	}
}

func localConnectProxy(t *testing.T, target string) *url.URL {
	t.Helper()
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.Method != http.MethodConnect || request.Host != target {
			t.Error("expected CONNECT to the local reset server")
			writer.WriteHeader(http.StatusBadRequest)
			return
		}
		upstream, err := net.Dial("tcp", target)
		if err != nil {
			t.Error(err)
			writer.WriteHeader(http.StatusBadGateway)
			return
		}
		defer upstream.Close()
		client, buffer, err := writer.(http.Hijacker).Hijack()
		if err != nil {
			t.Error(err)
			return
		}
		defer client.Close()
		_, _ = buffer.WriteString("HTTP/1.1 200 Connection Established\r\n\r\n")
		if err := buffer.Flush(); err != nil {
			return
		}
		done := make(chan struct{})
		go func() {
			_, _ = io.Copy(upstream, buffer)
			upstream.Close()
			close(done)
		}()
		_, _ = io.Copy(client, upstream)
		client.Close()
		<-done
	}))
	t.Cleanup(server.Close)
	address, err := url.Parse(server.URL)
	if err != nil {
		t.Fatal(err)
	}
	return address
}

func TestSDKHTTP2ProtocolResetWithoutReplay(t *testing.T) {
	t.Parallel()
	for _, complete := range []bool{false, true} {
		t.Run(fmt.Sprintf("choice_complete_%t", complete), func(t *testing.T) {
			reason := "null"
			if complete {
				reason = `"stop"`
			}
			prefix := fmt.Sprintf(`data: {"choices":[{"index":0,"delta":{"content":"Answer"},"finish_reason":%s}]}`, reason) + "\n\n"
			server, requests, read := protocolResetServer(t, nil, prefix, 0)
			language, err := newSDK(t.Context(), transportConfig{Kind: "chat", Model: "fixture", Endpoint: server.URL, Key: "fixture-key"})
			if err != nil {
				t.Fatal(err)
			}
			sdk := language.(*sdkModel)
			sdk.client.Transport = responseTransport{base: server.Client().Transport, stall: stallTimeout}
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			defer cancel()
			var attempts []Attempt
			var output string
			var finishes int
			var reset func()
			if complete {
				reset = func() { t.Error("a completed choice was replayed for its lost usage trailer") }
			}
			err = Run(ctx, sdk, requestCall(t, "chat"), Main, &policyClock{}, func(attempt Attempt) error {
				attempts = append(attempts, attempt)
				return nil
			}, func(part fantasy.StreamPart) bool {
				switch part.Type {
				case fantasy.StreamPartTypeTextDelta:
					output += part.Delta
					read <- struct{}{}
				case fantasy.StreamPartTypeFinish:
					finishes++
				}
				return true
			}, reset)
			if len(attempts) != 1 || len(requests) != 1 || attempts[0].RetryDelay != nil || !reflect.DeepEqual(attempts[0].Usage, canonical.Usage{}) || output != "Answer" {
				t.Fatalf("peer reset replayed output or invented usage: attempts=%+v requests=%d output=%q", attempts, len(requests), output)
			}
			if complete {
				if err != nil || finishes != 1 {
					t.Fatalf("a lost optional usage trailer invalidated a completed choice: finishes=%d error=%v", finishes, err)
				}
			} else if failure := Classify(err); failure == nil || failure.Kind != Network || finishes != 0 {
				t.Fatalf("unretractable partial output hid a peer reset: finishes=%d failure=%+v", finishes, failure)
			}
		})
	}
}

func TestSDKHTTP2ProtocolRecoveryIsModelScoped(t *testing.T) {
	t.Parallel()
	prefixes := map[string]string{
		"responses": "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"message\",\"delta\":\"Discarded\"}\n\n",
		"google":    "data: {\"candidates\":[{\"index\":0,\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Discarded\"}]}}]}\n\n",
		"chat":      "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Discarded\"}}]}\n\n",
		"anthropic": "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"Discarded\"}}\n\n",
	}
	for _, test := range []struct {
		name, kind                      string
		headers, proxy, oauth, recovers bool
	}{
		{name: "responses/preheaders", kind: "responses", recovers: true},
		{name: "responses/body", kind: "responses", headers: true, recovers: true},
		{name: "responses/proxy", kind: "responses", headers: true, proxy: true, recovers: true},
		{name: "chat/body", kind: "chat", headers: true, recovers: true},
		{name: "google/body", kind: "google", headers: true, recovers: true},
		{name: "anthropic/body", kind: "anthropic", headers: true, recovers: true},
		{name: "anthropic/oauth", kind: "anthropic", headers: true, oauth: true, recovers: true},
		{name: "fallback unavailable", kind: "responses", headers: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			prefix := ""
			if test.headers {
				prefix = prefixes[test.kind]
			}
			var success []byte
			if test.recovers {
				success = fixture(t, test.kind+"-text.sse")
			}
			server, requests, read := protocolResetServer(t, success, prefix, 0)
			base := server.Client().Transport.(*http.Transport).Clone()
			var proxy *url.URL
			if test.proxy {
				proxy = localConnectProxy(t, server.Listener.Addr().String())
			}
			t.Cleanup(base.CloseIdleConnections)
			var proxyCalls atomic.Int32
			base.Proxy = func(*http.Request) (*url.URL, error) { proxyCalls.Add(1); return proxy, nil }
			language, err := newSDK(t.Context(), transportConfig{Kind: test.kind, Model: "fixture", Endpoint: server.URL, Key: "fixture-key", OAuth: test.oauth})
			if err != nil {
				t.Fatal(err)
			}
			sdk := language.(*sdkModel)
			var transport http.RoundTripper = responseTransport{base: base, stall: stallTimeout}
			if test.oauth {
				transport = claudeTransport{base: transport}
			}
			sdk.client.Transport = transport
			iterations := 1
			if test.recovers {
				iterations = 2
			}
			for run := range iterations {
				ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
				defer cancel()
				timer := &policyClock{}
				var attempts []Attempt
				output, finishes, resets := "", 0, 0
				err := Run(ctx, sdk, requestCall(t, test.kind), Main, timer, func(attempt Attempt) error {
					attempts = append(attempts, attempt)
					return nil
				}, func(part fantasy.StreamPart) bool {
					if part.Type == fantasy.StreamPartTypeTextDelta {
						output += part.Delta
						if part.Delta == "Discarded" {
							read <- struct{}{}
						}
					}
					if part.Type == fantasy.StreamPartTypeFinish {
						finishes++
					}
					return true
				}, func() {
					if test.headers && resets == 0 && output != "Discarded" {
						t.Fatalf("protocol recovery lost its provisional output before reset: %q", output)
					}
					output = ""
					resets++
				})
				if run == 1 {
					if count := len(attempts); err != nil || count != 1 || attempts[0].Failure != nil || len(timer.delays) != 0 || resets != 0 || output != "Read complete" || finishes != 1 {
						t.Fatalf("later step repeated a confirmed protocol reset: attempts=%d waits=%v resets=%d text=%q err=%v", count, timer.delays, resets, output, err)
					}
					if request := <-requests; request.major != 1 {
						t.Fatalf("later step negotiated HTTP/%d", request.major)
					}
					continue
				}
				count := MainRetries + 1
				if test.recovers {
					count = 2
					if err != nil || output != "Read complete" || finishes != 1 {
						t.Fatalf("request-scoped HTTP/1 recovery failed: output=%q finishes=%d error=%v", output, finishes, err)
					}
				} else if failure := Classify(err); failure == nil || failure.Kind != Network || !failure.Exhausted || finishes != 0 {
					t.Fatalf("unavailable fallback exceeded its budget or became success: %+v", failure)
				}
				if len(attempts) != count || len(timer.delays) != count-1 || resets != count-1 || sdk.client.Transport != transport {
					t.Fatalf("protocol recovery changed its budget or shared transport: attempts=%d waits=%v resets=%d", len(attempts), timer.delays, resets)
				}
				first := attempts[0]
				if first.Failure == nil || first.Failure.Kind != Network || !strings.Contains(first.Failure.Message, "stream error: stream ID 1; PROTOCOL_ERROR; received from peer") || !reflect.DeepEqual(first.Usage, canonical.Usage{}) || test.headers && (first.Status == nil || *first.Status != 200) || !test.headers && first.Status != nil {
					t.Fatalf("peer reset lost its cause, status or unknown usage: %+v", first)
				}
				body := ""
				for index := range count {
					select {
					case request := <-requests:
						if index == 0 {
							body = request.body
						}
						if request.body != body {
							t.Fatal("protocol recovery changed the captured request body")
						}
						if index == 1 {
							if request.major != 1 || !test.recovers && (attempts[index].Failure == nil || attempts[index].Failure.Kind != Unavailable) {
								t.Fatalf("HTTP/1.1 fallback lost its wire protocol or gateway failure: request=%+v attempt=%+v", request, attempts[index])
							}
						} else if request.major != 2 || request.stream != 1 || request.connection != int32(run+max(index, 1)) {
							t.Fatalf("HTTP/2 was disabled outside the fallback attempt or the reset connection was reused: %+v", request)
						}
					case <-ctx.Done():
						t.Fatal("protocol recovery lost a captured request")
					}
					if attempts[index].RequestID != first.RequestID || attempts[index].Index != uint64(index) {
						t.Fatal("protocol recovery changed the logical request identity")
					}
				}
			}
			if proxyCalls.Load() < int32(iterations+1) {
				t.Fatal("HTTP/1 fallback bypassed the captured proxy function")
			}
		})
	}
}

func TestSDKRechecksHTTP2AfterHTTP1Fails(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		name      string
		preferred bool
		status    int
	}{
		{name: "preferred HTTP/1.1 fails", preferred: true},
		{name: "large HTTP/1.1 rejected", status: http.StatusUpgradeRequired},
	} {
		t.Run(test.name, func(t *testing.T) {
			server, requests, _ := protocolResetServer(t, nil, "", test.status)
			language, err := newSDK(t.Context(), transportConfig{Kind: "responses", Model: "fixture", Endpoint: server.URL, Key: "fixture-key"})
			if err != nil {
				t.Fatal(err)
			}
			sdk := language.(*sdkModel)
			base := server.Client().Transport.(*http.Transport)
			http1 := http1Transport(base)
			t.Cleanup(http1.CloseIdleConnections)
			sdk.client.Transport = responseTransport{base: base, http1: http1, stall: stallTimeout}
			sdk.preferHTTP1.Store(test.preferred)
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			defer cancel()
			err = Run(ctx, sdk, largeResponsesCall(t), Main, &policyClock{}, nil, nil, nil)
			if failure := Classify(err); failure == nil || !failure.Exhausted || sdk.preferHTTP1.Load() || len(requests) != MainRetries+1 {
				t.Fatalf("failed HTTP/1 route changed retry limits or remained preferred: attempts=%d error=%v", len(requests), err)
			}
			for index := range MainRetries + 1 {
				request := <-requests
				if index == 0 && request.major != 1 || index > 0 && request.major != 2 {
					t.Fatalf("attempt %d used HTTP/%d", index+1, request.major)
				}
			}
		})
	}
}

func TestSDKHTTP1ProbeRetriesProviderThrottle(t *testing.T) {
	t.Parallel()
	for _, status := range []int{http.StatusTooManyRequests, 529} {
		t.Run(fmt.Sprint(status), func(t *testing.T) {
			server, requests, _ := protocolResetServer(t, fixture(t, "responses-text.sse"), "", status)
			language, err := newSDK(t.Context(), transportConfig{Kind: "responses", Model: "fixture", Endpoint: server.URL, Key: "fixture-key"})
			if err != nil {
				t.Fatal(err)
			}
			sdk := language.(*sdkModel)
			sdk.client.Transport = responseTransport{base: server.Client().Transport, stall: stallTimeout}
			var attempts []Attempt
			err = Run(t.Context(), sdk, requestCall(t, "responses"), Main, &policyClock{}, func(a Attempt) error {
				if a.Failure != nil && sdk.preferHTTP1.Load() {
					t.Fatal("HTTP/1 preferred before a completed response")
				}
				attempts = append(attempts, a)
				return nil
			}, nil, nil)
			if err != nil || len(attempts) != 3 || !sdk.preferHTTP1.Load() || len(requests) != 3 {
				t.Fatalf("attempts=%d requests=%d preferHTTP1=%t error=%v", len(attempts), len(requests), sdk.preferHTTP1.Load(), err)
			}
			throttle := attempts[1]
			if throttle.Status == nil || int(*throttle.Status) != status || throttle.RetryDelay == nil || *throttle.RetryDelay != 4*time.Second {
				t.Fatalf("provider retry hint lost: %+v", throttle)
			}
			for index, protocol := range []int{2, 1, 1} {
				if r := <-requests; r.major != protocol || attempts[index].RequestID != attempts[0].RequestID {
					t.Fatalf("attempt %d protocol=%d request=%s", index, r.major, attempts[index].RequestID)
				}
			}
		})
	}
}

func TestSDKHTTP1OriginPersistsAcrossFreshModelsAndExpires(t *testing.T) {
	t.Parallel()
	server, requests, _ := protocolResetServer(t, fixture(t, "responses-text.sse"), "", 0)
	base := server.Client().Transport.(*http.Transport).Clone()
	t.Cleanup(base.CloseIdleConnections)
	origin := endpointOrigin(server.URL)
	t.Cleanup(func() { defaultHTTP1Origins.forget(origin) })

	newFresh := func() *sdkModel {
		language, err := newSDK(t.Context(), transportConfig{Kind: "responses", Model: "fixture", Endpoint: server.URL, Key: "fixture-key"})
		if err != nil {
			t.Fatal(err)
		}
		sdk := language.(*sdkModel)
		sdk.client.Transport = responseTransport{base: base, stall: stallTimeout}
		return sdk
	}

	clock := &policyClock{now: time.Date(2026, 11, 26, 12, 0, 0, 0, time.UTC)}
	// Turn 1 hits HTTP/2 PROTOCOL_ERROR, recovers on HTTP/1.1, and records the origin.
	var firstAttempts []Attempt
	if err := Run(t.Context(), newFresh(), requestCall(t, "responses"), Main, clock, func(a Attempt) error {
		firstAttempts = append(firstAttempts, a)
		return nil
	}, nil, nil); err != nil {
		t.Fatalf("first turn failed: %v", err)
	}
	if len(firstAttempts) != 2 {
		t.Fatalf("first turn attempts = %d, want 2", len(firstAttempts))
	}
	if r0, r1 := <-requests, <-requests; r0.major != 2 || r1.major != 1 {
		t.Fatalf("first turn protocols = HTTP/%d then HTTP/%d, want 2 then 1", r0.major, r1.major)
	}

	// Turn 2 creates a brand-new sdkModel (preferHTTP1 == false) and should
	// immediately use HTTP/1.1 without re-hitting HTTP/2.
	clock.now = clock.now.Add(5 * time.Minute)
	var secondAttempts []Attempt
	if err := Run(t.Context(), newFresh(), requestCall(t, "responses"), Main, clock, func(a Attempt) error {
		secondAttempts = append(secondAttempts, a)
		return nil
	}, nil, nil); err != nil {
		t.Fatalf("second turn failed: %v", err)
	}
	if len(secondAttempts) != 1 || secondAttempts[0].Failure != nil {
		t.Fatalf("second turn repeated HTTP/2 failure: %+v", secondAttempts)
	}
	if r := <-requests; r.major != 1 {
		t.Fatalf("second turn negotiated HTTP/%d, want HTTP/1", r.major)
	}

	// After the TTL elapses, a fresh model probes HTTP/2 again.
	clock.now = clock.now.Add(http1OriginTTL + time.Minute)
	var expiredAttempts []Attempt
	if err := Run(t.Context(), newFresh(), requestCall(t, "responses"), Main, clock, func(a Attempt) error {
		expiredAttempts = append(expiredAttempts, a)
		return nil
	}, nil, nil); err != nil {
		t.Fatalf("post-expiry turn failed: %v", err)
	}
	if len(expiredAttempts) != 2 {
		t.Fatalf("post-expiry attempts = %d, want 2 (re-probe HTTP/2 then recover HTTP/1.1)", len(expiredAttempts))
	}
	if r0, r1 := <-requests, <-requests; r0.major != 2 || r1.major != 1 {
		t.Fatalf("post-expiry protocols = HTTP/%d then HTTP/%d, want 2 then 1", r0.major, r1.major)
	}
}

func largeResponsesCall(t *testing.T) fantasy.Call {
	t.Helper()
	call := requestCall(t, "responses")
	call.Prompt = fantasy.Prompt{{
		Role:    fantasy.MessageRoleUser,
		Content: []fantasy.MessagePart{fantasy.TextPart{Text: strings.Repeat("A", largeHTTP1BodyBytes+4096)}},
	}}
	return call
}

func TestSDKLargeBodyUsesHTTP1DirectlyAndFallsBackToHTTP2(t *testing.T) {
	t.Parallel()
	success := fixture(t, "responses-text.sse")
	largeCall := largeResponsesCall(t)
	for _, test := range []struct {
		name         string
		preference   string
		http1Rejects []int
		protocols    []int
	}{
		{name: "direct", protocols: []int{1}},
		{name: "unavailable", http1Rejects: []int{http.StatusServiceUnavailable}, protocols: []int{1, 2}},
		{name: "upgrade required", http1Rejects: []int{http.StatusUpgradeRequired}, protocols: []int{1, 2}},
		{name: "upgrade required after HTTP resend", http1Rejects: []int{http.StatusBadRequest, http.StatusUpgradeRequired}, protocols: []int{1, 1, 2}},
		{name: "preferred model receives upgrade required", preference: "model", http1Rejects: []int{http.StatusUpgradeRequired}, protocols: []int{1, 2}},
		{name: "remembered origin receives version not supported", preference: "origin", http1Rejects: []int{http.StatusHTTPVersionNotSupported}, protocols: []int{1, 2}},
	} {
		t.Run(test.name, func(t *testing.T) {
			requests := make(chan int, 2*(MainRetries+1))
			var http1Calls atomic.Int32
			server := httptest.NewUnstartedServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				_, _ = io.Copy(io.Discard, r.Body)
				requests <- r.ProtoMajor
				if r.ProtoMajor == 1 && len(test.http1Rejects) > 0 {
					index := min(int(http1Calls.Add(1))-1, len(test.http1Rejects)-1)
					w.WriteHeader(test.http1Rejects[index])
					_, _ = io.WriteString(w, "HTTP/1.1 request rejected")
					return
				}
				w.Header().Set("Content-Type", "text/event-stream")
				_, _ = w.Write(success)
			}))
			server.EnableHTTP2 = true
			server.StartTLS()
			t.Cleanup(server.Close)
			base := server.Client().Transport.(*http.Transport)
			http1 := http1Transport(base)
			t.Cleanup(http1.CloseIdleConnections)

			language, err := newSDK(t.Context(), transportConfig{Kind: "responses", Model: "fixture", Endpoint: server.URL, Key: "fixture-key"})
			if err != nil {
				t.Fatal(err)
			}
			sdk := language.(*sdkModel)
			sdk.client.Transport = responseTransport{base: base, http1: http1, stall: stallTimeout}
			timer := &policyClock{now: time.Now()}
			if test.preference == "model" {
				sdk.preferHTTP1.Store(true)
			}
			if test.preference != "" {
				defaultHTTP1Origins.remember(sdk.origin(), timer.Now())
				t.Cleanup(func() { defaultHTTP1Origins.forget(sdk.origin()) })
			}

			var attempts []Attempt
			err = Run(t.Context(), sdk, largeCall, Main, timer, func(a Attempt) error {
				attempts = append(attempts, a)
				return nil
			}, nil, nil)
			majors := make([]int, len(requests))
			for index := range majors {
				majors[index] = <-requests
			}
			if err != nil || len(attempts) != len(test.protocols) || !reflect.DeepEqual(majors, test.protocols) {
				t.Fatalf("attempts=%d protocols=%v, want %v: %v", len(attempts), majors, test.protocols, err)
			}
			for index, status := range test.http1Rejects {
				attempt := attempts[index]
				if attempt.Status == nil || int(*attempt.Status) != status || attempt.Failure == nil || attempt.RetryDelay == nil || attempt.RequestID != attempts[index+1].RequestID {
					t.Fatalf("rejected attempt lost its status or shared resend: %+v", attempt)
				}
			}
			if test.preference != "" && (sdk.preferHTTP1.Load() || defaultHTTP1Origins.preferred(sdk.origin(), timer.Now())) {
				t.Fatal("the origin or the model still prefers the rejected HTTP/1.1 route")
			}
		})
	}
}

func TestSDKLargeHTTP1DoesNotReplayEscapedOutput(t *testing.T) {
	t.Parallel()
	language, err := newSDK(t.Context(), transportConfig{Kind: "responses", Model: "fixture", Endpoint: "https://large-output.invalid", Key: "fixture-key"})
	if err != nil {
		t.Fatal(err)
	}
	const chunk = "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"message\",\"delta\":\"Answer\"}\n\n"
	readErr := &net.OpError{Op: "read", Net: "tcp", Err: io.ErrClosedPipe}
	language.(*sdkModel).client.Transport = responseTransport{
		base: sdkFixtureRoundTripper(func(*http.Request) (*http.Response, error) {
			t.Fatal("replayed already visible output")
			return nil, readErr
		}),
		http1: sdkFixtureRoundTripper(func(r *http.Request) (*http.Response, error) {
			_ = r.Body.Close()
			return &http.Response{StatusCode: http.StatusOK, Request: r, Header: http.Header{"Content-Type": {"text/event-stream"}}, Body: io.NopCloser(io.MultiReader(strings.NewReader(chunk), iotest.ErrReader(readErr)))}, nil
		}),
		stall: stallTimeout,
	}
	var attempts []Attempt
	var output string
	err = Run(t.Context(), language, largeResponsesCall(t), Main, &policyClock{}, func(a Attempt) error {
		attempts = append(attempts, a)
		return nil
	}, func(part fantasy.StreamPart) bool {
		if part.Type == fantasy.StreamPartTypeTextDelta {
			output += part.Delta
		}
		return true
	}, nil)
	if failure := Classify(err); failure == nil || failure.Kind != Network || len(attempts) != 1 || attempts[0].RetryDelay != nil || output != "Answer" {
		t.Fatalf("partial outcome was replayed or lost: attempts=%+v output=%q error=%v", attempts, output, err)
	}
}
