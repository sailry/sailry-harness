package mcp

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"testing/synctest"
	"time"

	"github.com/modelcontextprotocol/go-sdk/jsonrpc"
	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type mcpRoundTripFunc func(*http.Request) (*http.Response, error)

func (fn mcpRoundTripFunc) RoundTrip(request *http.Request) (*http.Response, error) {
	return fn(request)
}

type heldJSONBody struct {
	ctx         context.Context
	reader      *bytes.Reader
	started     chan struct{}
	release     chan struct{}
	once        sync.Once
	releaseOnce sync.Once
	closed      atomic.Bool
}

func (body *heldJSONBody) Read(dst []byte) (int, error) {
	body.once.Do(func() { close(body.started) })
	select {
	case <-body.ctx.Done():
		return 0, body.ctx.Err()
	case <-body.release:
		return body.reader.Read(dst)
	}
}

func (body *heldJSONBody) Close() error {
	body.closed.Store(true)
	return nil
}

func (body *heldJSONBody) finish() { body.releaseOnce.Do(func() { close(body.release) }) }

func TestMCPHTTPJSONCancellationKeepsConnection(t *testing.T) {
	t.Parallel()
	for _, deadline := range []bool{false, true} {
		t.Run(map[bool]string{false: "cancel", true: "deadline"}[deadline], func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				first := &heldJSONBody{started: make(chan struct{}), release: make(chan struct{})}
				second := &heldJSONBody{started: make(chan struct{}), release: make(chan struct{})}
				var discoveries atomic.Int32
				base := mcpRoundTripFunc(func(request *http.Request) (*http.Response, error) {
					if request.Method == http.MethodDelete {
						return &http.Response{StatusCode: http.StatusNoContent, Header: make(http.Header), Body: http.NoBody, Request: request}, nil
					}
					defer request.Body.Close()
					var call struct {
						ID     json.RawMessage `json:"id"`
						Method string          `json:"method"`
						Params struct {
							Name string `json:"name"`
						} `json:"params"`
					}
					if err := json.NewDecoder(request.Body).Decode(&call); err != nil {
						return nil, err
					}
					var result any
					switch call.Method {
					case "server/discover":
						discoveries.Add(1)
						result = map[string]any{"resultType": "complete", "supportedVersions": []string{"2026-07-28"}, "capabilities": map[string]any{"tools": map[string]any{}}}
					case "tools/call":
						result = map[string]any{"resultType": "complete", "content": []any{map[string]any{"type": "text", "text": call.Params.Name}}}
					case "notifications/cancelled":
						return &http.Response{StatusCode: http.StatusAccepted, Header: make(http.Header), Body: http.NoBody, Request: request}, nil
					default:
						return nil, fmt.Errorf("unexpected MCP method %q", call.Method)
					}
					data, err := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": call.ID, "result": result})
					if err != nil {
						return nil, err
					}
					response := &http.Response{StatusCode: http.StatusOK, Header: http.Header{"Content-Type": {"application/json; charset=utf-8"}}, Body: io.NopCloser(bytes.NewReader(data)), Request: request}
					if call.Method == "tools/call" {
						body := map[string]*heldJSONBody{"first": first, "second": second}[call.Params.Name]
						if body != nil {
							body.ctx, body.reader = request.Context(), bytes.NewReader(data)
							response.Body = body
						}
					}
					return response, nil
				})
				session, err := Open(t.Context(), Launch{Kind: "streamable-http", URL: "https://mcp.invalid/rpc", Client: &http.Client{Transport: base}}, nil)
				if err != nil {
					t.Fatal(err)
				}
				defer func() {
					first.finish()
					second.finish()
					session.Close()
				}()
				var ctx context.Context
				var cancel context.CancelFunc
				if deadline {
					ctx, cancel = context.WithTimeout(t.Context(), time.Minute)
				} else {
					ctx, cancel = context.WithCancel(t.Context())
				}
				defer cancel()
				one, two := make(chan error, 1), make(chan error, 1)
				invoke := func(ctx context.Context, name string, done chan<- error) {
					result, err := session.CallTool(ctx, &sdk.CallToolParams{Name: name})
					if err == nil {
						if len(result.Content) != 1 {
							err = fmt.Errorf("%s: unexpected content count %d", name, len(result.Content))
						} else if text, ok := result.Content[0].(*sdk.TextContent); !ok || text.Text != name {
							err = fmt.Errorf("%s: received another call's result", name)
						}
					}
					done <- err
				}
				go invoke(ctx, "first", one)
				<-first.started
				go invoke(t.Context(), "second", two)
				<-second.started
				// Both response bodies are pending at once: buffering must not
				// serialize independent SDK requests.
				if deadline {
					time.Sleep(time.Minute)
				} else {
					cancel()
				}
				if err := <-one; !errors.Is(err, ctx.Err()) {
					t.Fatalf("interrupted call = %v, want %v", err, ctx.Err())
				}
				synctest.Wait()
				if !first.closed.Load() {
					t.Fatal("cancelled JSON body was not closed")
				}
				second.finish()
				if err := <-two; err != nil {
					t.Fatalf("neighbor failed after cancellation: %v", err)
				}
				if !second.closed.Load() {
					t.Fatal("completed JSON body was not closed")
				}
				invoke(t.Context(), "after", two)
				if err := <-two; err != nil || discoveries.Load() != 1 {
					t.Fatalf("connection was not reusable: error=%v, discoveries=%d", err, discoveries.Load())
				}
			})
		})
	}
}

type observedResponseBody struct {
	reader io.Reader
	read   int
	closed bool
}

func (body *observedResponseBody) Read(dst []byte) (int, error) {
	n, err := body.reader.Read(dst)
	body.read += n
	return n, err
}

func (body *observedResponseBody) Close() error {
	body.closed = true
	return nil
}

func TestMCPHTTPJSONBodyLimit(t *testing.T) {
	t.Parallel()
	for _, size := range []int{maxMessage, maxMessage + 1, maxMessage + 1024} {
		t.Run(fmt.Sprint(size), func(t *testing.T) {
			body := &observedResponseBody{reader: strings.NewReader(strings.Repeat("x", size))}
			transport := headerTransport{base: mcpRoundTripFunc(func(*http.Request) (*http.Response, error) {
				return &http.Response{StatusCode: http.StatusOK, Header: http.Header{"Content-Type": {"application/json"}}, Body: body}, nil
			})}
			request, _ := http.NewRequestWithContext(t.Context(), http.MethodPost, "https://mcp.invalid/rpc", nil)
			response, err := transport.RoundTrip(request)
			if !body.closed || body.read != min(size, maxMessage+1) {
				t.Fatalf("body read=%d closed=%t", body.read, body.closed)
			}
			if size > maxMessage {
				var incomplete *responseReadError
				if !errors.As(err, &incomplete) || response != nil {
					t.Fatalf("oversized response=%v error=%v", response, err)
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			defer response.Body.Close()
			data, err := io.ReadAll(response.Body)
			if err != nil || len(data) != size {
				t.Fatalf("buffered bytes=%d error=%v", len(data), err)
			}
		})
	}
}

type truncatedJSONReader struct{}

func (truncatedJSONReader) Read([]byte) (int, error) { return 0, io.ErrUnexpectedEOF }

func httpJSONFailureFixture(body func() io.Reader) (Launch, *atomic.Int32) {
	submitted := new(atomic.Int32)
	base := mcpRoundTripFunc(func(request *http.Request) (*http.Response, error) {
		if request.Method == http.MethodDelete {
			return &http.Response{StatusCode: http.StatusNoContent, Header: make(http.Header), Body: http.NoBody, Request: request}, nil
		}
		defer request.Body.Close()
		var call struct {
			ID     json.RawMessage `json:"id"`
			Method string          `json:"method"`
		}
		if err := json.NewDecoder(request.Body).Decode(&call); err != nil {
			return nil, err
		}
		response := &http.Response{StatusCode: http.StatusOK, Header: http.Header{"Content-Type": {"application/json"}}, Request: request}
		switch call.Method {
		case "server/discover":
			data, err := json.Marshal(map[string]any{
				"jsonrpc": "2.0", "id": call.ID,
				"result": map[string]any{"resultType": "complete", "supportedVersions": []string{"2026-07-28"}, "capabilities": map[string]any{"tools": map[string]any{}}},
			})
			if err != nil {
				return nil, err
			}
			response.Body = io.NopCloser(bytes.NewReader(data))
		case "tools/call":
			// The tool has been submitted; only delivery of its result fails.
			submitted.Add(1)
			response.Body = io.NopCloser(body())
		default:
			return nil, fmt.Errorf("unexpected MCP method %q", call.Method)
		}
		return response, nil
	})
	return Launch{Kind: "streamable-http", URL: "https://mcp.invalid/rpc", Client: &http.Client{Transport: base}}, submitted
}

func TestMCPHTTPJSONReadErrorSurvivesSDKWrapping(t *testing.T) {
	t.Parallel()
	launch, submitted := httpJSONFailureFixture(func() io.Reader {
		return io.MultiReader(strings.NewReader(`{"jsonrpc":"2.0",`), truncatedJSONReader{})
	})
	session, err := Open(t.Context(), launch, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	_, err = session.CallTool(t.Context(), &sdk.CallToolParams{Name: "write"})
	var incomplete *responseReadError
	var rejection *jsonrpc.Error
	if !errors.As(err, &incomplete) || !errors.As(err, &rejection) || !errors.Is(err, io.ErrUnexpectedEOF) || submitted.Load() != 1 {
		t.Fatalf("SDK lost the incomplete response classification: error=%v, submissions=%d", err, submitted.Load())
	}
}

func TestMCPHTTPIncompleteToolResultIsNotRejection(t *testing.T) {
	t.Parallel()
	for _, tc := range []struct {
		name string
		body func() io.Reader
	}{
		{"truncated", func() io.Reader { return io.MultiReader(strings.NewReader(`{"jsonrpc":"2.0",`), truncatedJSONReader{}) }},
		{"oversized", func() io.Reader { return strings.NewReader(strings.Repeat("x", maxMessage+1)) }},
	} {
		for _, readOnly := range []bool{true, false} {
			t.Run(tc.name+"/"+map[bool]string{true: "read", false: "write"}[readOnly], func(t *testing.T) {
				launch, submitted := httpJSONFailureFixture(tc.body)
				pool := New(t.Context(), Options{})
				defer pool.Close()
				lease, err := pool.Acquire(t.Context(), Configuration{
					Package: protocol.PluginReference{Name: "http-results", Digest: "fixture"}, Server: "server",
					Launch: launch, Options: protocol.DefaultPluginMcpOptions(),
				})
				if err != nil {
					t.Fatal(err)
				}
				defer lease.Close()
				result, err := lease.Call(t.Context(), "tool", nil, readOnly, nil)
				want := protocol.ErrorCodeOutcomeUnknown
				if readOnly {
					want = protocol.ErrorCodeUnavailable
				}
				var fault *protocol.Fault
				if result != nil || !errors.As(err, &fault) || fault.Code != want {
					t.Fatalf("incomplete result=%v error=%v, want %s", result, err, want)
				}
				if submitted.Load() != 1 {
					t.Fatalf("tool was replayed after its response failed: submissions=%d", submitted.Load())
				}
				lease.connection.mu.Lock()
				poisoned := lease.connection.poisoned
				lease.connection.mu.Unlock()
				if poisoned == readOnly {
					t.Fatalf("readOnly=%t poisoned=%t", readOnly, poisoned)
				}
			})
		}
	}
}

func TestMCPHTTPLeavesStreamingAndErrorBodiesUnread(t *testing.T) {
	t.Parallel()
	for _, tc := range []struct {
		name        string
		status      int
		contentType string
	}{
		{"stream", http.StatusOK, "text/event-stream"},
		{"oauth", http.StatusUnauthorized, "application/json"},
		{"error", http.StatusInternalServerError, "application/json"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			body := &observedResponseBody{reader: strings.NewReader("original response")}
			original := &http.Response{StatusCode: tc.status, Header: http.Header{"Content-Type": {tc.contentType}}, Body: body}
			transport := headerTransport{base: mcpRoundTripFunc(func(*http.Request) (*http.Response, error) { return original, nil })}
			request, _ := http.NewRequestWithContext(t.Context(), http.MethodPost, "https://mcp.invalid/rpc", nil)
			response, err := transport.RoundTrip(request)
			if err != nil || response != original || response.Body != body || body.read != 0 || body.closed {
				t.Fatalf("response changed: response=%v error=%v read=%d closed=%t", response, err, body.read, body.closed)
			}
			response.Body.Close()
		})
	}
}
