package model

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"errors"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"net/http/httptrace"
	"slices"
	"strconv"
	"strings"
	"sync/atomic"
	"syscall"
	"testing"
	"time"

	"charm.land/fantasy"
)

type tlsRecordConn struct {
	net.Conn
	corrupt atomic.Bool
	closed  chan struct{}
}

func (conn *tlsRecordConn) Write(data []byte) (int, error) {
	if len(data) > 0 && conn.corrupt.Swap(false) {
		data = slices.Clone(data)
		data[len(data)-1] ^= 1
	}
	return conn.Conn.Write(data)
}

type tlsRecordListener struct{ net.Listener }

func (listener tlsRecordListener) Accept() (net.Conn, error) {
	conn, err := listener.Listener.Accept()
	if err != nil {
		return nil, err
	}
	return &tlsRecordConn{Conn: conn, closed: make(chan struct{})}, nil
}

type tlsRecordConnKey struct{}

func TestSDKBadRecordMACRecovery(t *testing.T) {
	const responseDelta = `data: {"type":"response.output_text.delta","item_id":"message","delta":"Discarded"}`
	const toolCall = `data: {"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","call_id":"stable-call","name":"read","arguments":"{}"}}`
	for _, test := range []struct {
		name, kind, prefix string
		http2, tool        bool
	}{
		{name: "before headers", kind: "responses"},
		{name: "responses", kind: "responses", prefix: responseDelta},
		{name: "chat", kind: "chat", prefix: `data: {"choices":[{"index":0,"delta":{"content":"Discarded"}}]}`},
		{name: "anthropic", kind: "anthropic", prefix: "event: content_block_start\n" + `data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":"Discarded"}}`},
		{name: "google", kind: "google", prefix: `data: {"candidates":[{"content":{"role":"model","parts":[{"text":"Discarded"}]}}]}`},
		{name: "http2", kind: "responses", prefix: responseDelta, http2: true},
		{name: "tool without reset", kind: "responses", prefix: toolCall, tool: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			defer cancel()
			success := fixture(t, test.kind+"-text.sse")
			type wireRequest struct {
				conn  *tlsRecordConn
				major int
				body  string
			}
			wire := make(chan wireRequest, MainRetries+4)
			var requests atomic.Int32
			server := httptest.NewUnstartedServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
				body, err := io.ReadAll(request.Body)
				if err != nil {
					t.Error(err)
					return
				}
				conn := request.Context().Value(tlsRecordConnKey{}).(*tlsRecordConn)
				index := requests.Add(1)
				select {
				case wire <- wireRequest{conn: conn, major: request.ProtoMajor, body: string(body)}:
				case <-ctx.Done():
					return
				}
				writer.Header().Set("Content-Type", "text/event-stream")
				if index == 2 {
					if test.prefix != "" {
						_, _ = io.WriteString(writer, test.prefix+"\n\n")
						writer.(http.Flusher).Flush()
					}
					conn.corrupt.Store(true)
					_, _ = writer.Write(success)
					writer.(http.Flusher).Flush()
					return
				}
				writer.Header().Set("Content-Length", strconv.Itoa(len(success)))
				_, _ = writer.Write(success)
			}))
			server.Listener = tlsRecordListener{server.Listener}
			server.EnableHTTP2 = test.http2
			server.Config.ConnContext = func(ctx context.Context, conn net.Conn) context.Context {
				return context.WithValue(ctx, tlsRecordConnKey{}, conn.(*tls.Conn).NetConn())
			}
			server.Config.ConnState = func(conn net.Conn, state http.ConnState) {
				if state == http.StateClosed {
					close(conn.(*tls.Conn).NetConn().(*tlsRecordConn).closed)
				}
			}
			server.StartTLS()
			t.Cleanup(server.Close)
			language, err := newSDK(ctx, transportConfig{Kind: test.kind, Model: "fixture", Endpoint: server.URL, Key: "fixture-key"})
			if err != nil {
				t.Fatal(err)
			}
			language.(*sdkModel).client.Transport = responseTransport{base: server.Client().Transport, stall: stallTimeout}
			call := requestCall(t, test.kind)
			if err := Run(ctx, language, call, Main, &policyClock{}, nil, nil, nil); err != nil {
				t.Fatalf("initial model request: %v", err)
			}
			timer := &policyClock{beforeSleep: func() {
				if count := requests.Load(); count != 2 {
					t.Fatalf("model request sent %d times before Sailry admitted a retry", count)
				}
			}}
			var attempts []Attempt
			var text, callID string
			var calls, finishes, resets int
			var reset func()
			if !test.tool {
				reset = func() {
					if test.prefix != "" && text != "Discarded" {
						t.Fatalf("missing provisional output before retry: %q", text)
					}
					text = ""
					resets++
				}
			}
			err = Run(ctx, language, call, Main, timer, func(attempt Attempt) error {
				attempts = append(attempts, attempt)
				return nil
			}, func(part fantasy.StreamPart) bool {
				switch part.Type {
				case fantasy.StreamPartTypeTextDelta:
					text += part.Delta
				case fantasy.StreamPartTypeToolCall:
					calls++
					callID = part.ID
				case fantasy.StreamPartTypeFinish:
					finishes++
				}
				return true
			}, reset)
			if len(attempts) == 0 {
				t.Fatalf("missing failed attempt: %v", err)
			}
			first := attempts[0]
			if first.Failure == nil || first.Failure.Kind != Network || !first.Failure.Retryable() || first.Failure.Message != "model stream failed: local error: tls: bad record MAC" {
				t.Fatalf("TLS record failure lost the network diagnostic: %+v", first.Failure)
			}
			if test.prefix == "" {
				if first.Status != nil || first.ResponseUs != nil {
					t.Fatalf("failed TLS record created response facts: %+v", first)
				}
			} else if first.Status == nil || *first.Status != http.StatusOK || first.ResponseUs == nil {
				t.Fatalf("failed TLS record lost received headers: %+v", first)
			}
			wantRequests := 4
			if test.tool {
				wantRequests = 3
				if err == nil || len(attempts) != 1 || first.RetryDelay != nil || len(timer.delays) != 0 || finishes != 0 || calls != 1 || callID != "stable-call" {
					t.Fatalf("unretractable tool was replayed: attempts=%d waits=%v finishes=%d calls=%d id=%q error=%v", len(attempts), timer.delays, finishes, calls, callID, err)
				}
			} else {
				if err != nil || len(attempts) != 2 || len(timer.delays) != 1 || resets != 1 || finishes != 1 || text != "Read complete" || calls != 0 {
					t.Fatalf("record failure recovery: attempts=%d waits=%v resets=%d finishes=%d calls=%d output=%q error=%v", len(attempts), timer.delays, resets, finishes, calls, text, err)
				}
				if first.RetryDelay == nil || attempts[1].Failure != nil || attempts[1].RequestID != first.RequestID || attempts[1].Index != 1 {
					t.Fatalf("retry lost the admitted request identity: %+v", attempts)
				}
			}
			if err := Run(ctx, language, call, Main, &policyClock{}, nil, nil, nil); err != nil {
				t.Fatalf("next model request: %v", err)
			}
			if got := len(wire); got != wantRequests {
				t.Fatalf("got %d wire requests, want %d", got, wantRequests)
			}
			warm, failed, recovered := <-wire, <-wire, <-wire
			select {
			case <-failed.conn.closed:
			case <-ctx.Done():
				t.Fatal("damaged TLS connection was left open")
			}
			if warm.conn != failed.conn || recovered.conn == failed.conn {
				t.Fatal("the transport did not retire only the damaged connection")
			}
			if !test.tool {
				next := <-wire
				if recovered.conn != next.conn {
					t.Fatal("a healthy TLS connection was not reused")
				}
			}
			major := 1
			if test.http2 {
				major = 2
			}
			if warm.major != major || failed.major != major || recovered.major != major {
				t.Fatal("TLS recovery changed the HTTP protocol")
			}
			if !test.tool && failed.body != recovered.body {
				t.Fatal("TLS retry changed the captured request body")
			}
		})
	}
}

func TestSDKCertificateVerificationDoesNotRetry(t *testing.T) {
	server := httptest.NewTLSServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		t.Error("model request sent without a trusted certificate")
	}))
	t.Cleanup(server.Close)
	base := server.Client().Transport.(*http.Transport).Clone()
	base.TLSClientConfig.RootCAs = x509.NewCertPool()
	t.Cleanup(base.CloseIdleConnections)
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	var handshakes atomic.Int32
	ctx = httptrace.WithClientTrace(ctx, &httptrace.ClientTrace{TLSHandshakeDone: func(_ tls.ConnectionState, err error) {
		if _, ok := errors.AsType[*tls.CertificateVerificationError](err); !ok {
			t.Errorf("TLS handshake did not report a verification error: %v", err)
		}
		handshakes.Add(1)
	}})
	language, err := newSDK(ctx, transportConfig{Kind: "responses", Model: "fixture", Endpoint: server.URL, Key: "fixture-key"})
	if err != nil {
		t.Fatal(err)
	}
	language.(*sdkModel).client.Transport = responseTransport{base: base, stall: stallTimeout}
	var attempts []Attempt
	err = Run(ctx, language, requestCall(t, "responses"), Main, &policyClock{}, func(attempt Attempt) error {
		attempts = append(attempts, attempt)
		return nil
	}, nil, nil)
	failure := Classify(err)
	if failure == nil || failure.Kind != InvalidRequest || failure.Retryable() || !strings.Contains(failure.Message, "tls: failed to verify certificate") {
		t.Fatalf("unexpected certificate verification failure: %+v", failure)
	}
	if len(attempts) != 1 || attempts[0].RetryDelay != nil || attempts[0].Status != nil || handshakes.Load() != 1 {
		t.Fatalf("certificate verification took extra attempts: attempts=%d handshakes=%d", len(attempts), handshakes.Load())
	}
}

type discardedSDKBody struct {
	io.Reader
	closed bool
}

func (body *discardedSDKBody) Close() error {
	body.closed = true
	return nil
}

func TestSDKFailedRoundTripClosesDiscardedBody(t *testing.T) {
	language, err := newSDK(t.Context(), transportConfig{Kind: "responses", Model: "fixture", Endpoint: "https://fixture.test", Key: "fixture-key"})
	if err != nil {
		t.Fatal(err)
	}
	discarded := &discardedSDKBody{Reader: strings.NewReader("discard")}
	success := fixture(t, "responses-text.sse")
	var failedContext context.Context
	var requests int
	language.(*sdkModel).client.Transport = responseTransport{stall: stallTimeout, base: pluginRoundTripper(func(request *http.Request) (*http.Response, error) {
		request.Body.Close()
		requests++
		response := &http.Response{StatusCode: http.StatusOK, Header: http.Header{"Content-Type": {"text/event-stream"}}}
		if requests == 1 {
			failedContext = request.Context()
			response.Body = discarded
			return response, &net.OpError{Op: "read", Err: syscall.ECONNRESET}
		}
		response.Body = io.NopCloser(strings.NewReader(string(success)))
		return response, nil
	})}
	var attempts []Attempt
	err = Run(t.Context(), language, requestCall(t, "responses"), Main, &policyClock{}, func(attempt Attempt) error {
		if len(attempts) == 0 && (!discarded.closed || failedContext.Err() == nil) {
			t.Fatal("discarded response and its transport context were left open")
		}
		attempts = append(attempts, attempt)
		return nil
	}, nil, nil)
	if err != nil || requests != 2 || len(attempts) != 2 {
		t.Fatalf("failed transaction was not retried once: requests=%d attempts=%d error=%v", requests, len(attempts), err)
	}
	if first := attempts[0]; first.Failure == nil || first.Failure.Kind != Network || first.Status != nil || first.ResponseUs != nil {
		t.Fatalf("discarded response created HTTP response facts: %+v", first)
	}
}
