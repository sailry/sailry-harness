package model

import (
	"context"
	"net/http/httptrace"
	"slices"
	"sync"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/clock"
	"github.com/sailry/sailry-harness/internal/node/agent/observation"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type attemptCaptureKey struct{}

type attemptCapture struct {
	mu           sync.Mutex
	clock        clock.Clock
	started      time.Time
	inventory    *observation.Inventory
	prepared     observation.Result
	trace        *observation.ResponsesTrace
	shape        *observation.ResponsesShape
	requestBytes *uint64
	dispatch     *uint64
	sent         *uint64
	response     *uint64
	status       *uint16
	dispatches   uint64
	writes       uint64
	// secrets are credentials a transport minted for this attempt, which
	// its failure must not repeat.
	secrets []protocol.Secret
}

// captureSecret adds a credential the attempt sent to its redactions.
func captureSecret(ctx context.Context, secret protocol.Secret) {
	if capture, ok := ctx.Value(attemptCaptureKey{}).(*attemptCapture); ok {
		capture.mu.Lock()
		capture.secrets = append(capture.secrets, secret)
		capture.mu.Unlock()
	}
}

// redact is the attempt's secrets after the route's own.
func (capture *attemptCapture) redact(route []protocol.Secret) []protocol.Secret {
	capture.mu.Lock()
	defer capture.mu.Unlock()
	return append(slices.Clip(route), capture.secrets...)
}

func captureAttempt(ctx context.Context, call fantasy.Call, timer clock.Clock, started time.Time) (context.Context, *attemptCapture) {
	inventory, _ := call.ProviderOptions[observation.ProviderKey].(*observation.Inventory)
	capture := &attemptCapture{clock: timer, started: started, inventory: inventory, trace: observation.ResponsesTraceFrom(ctx)}
	ctx = context.WithValue(ctx, attemptCaptureKey{}, capture)
	return httptrace.WithClientTrace(ctx, &httptrace.ClientTrace{WroteRequest: func(info httptrace.WroteRequestInfo) {
		capture.mu.Lock()
		capture.writes++
		if info.Err == nil {
			elapsed := uint64(max(0, capture.clock.Now().Sub(capture.started).Microseconds()))
			capture.sent = &elapsed
		}
		capture.mu.Unlock()
	}}), capture
}

// Transport hooks retain only request attribution and measured receipt facts.
func capturePrepared(ctx context.Context, raw []byte, serializedBody bool) {
	if capture, ok := ctx.Value(attemptCaptureKey{}).(*attemptCapture); ok {
		var prepared observation.Result
		var shape *observation.ResponsesShape
		if facts, ok := ctx.Value(factsKey{}).(*responseFacts); ok && facts.family == "responses" && serializedBody && capture.trace != nil {
			prepared, shape = capture.trace.Prepare(raw, capture.inventory)
		} else {
			prepared = observation.Prepared(raw, capture.inventory)
		}
		size := uint64(len(raw))
		capture.mu.Lock()
		capture.prepared = prepared
		capture.shape = shape
		capture.requestBytes = nil
		if serializedBody {
			capture.requestBytes = &size
		}
		capture.dispatch, capture.sent, capture.response, capture.status = nil, nil, nil, nil
		capture.mu.Unlock()
	}
}

// Dispatch marks the boundary after serialization and request observation, just
// before the HTTP transport. All timing offsets share the attempt's start.
func captureDispatch(ctx context.Context) {
	if capture, ok := ctx.Value(attemptCaptureKey{}).(*attemptCapture); ok {
		elapsed := uint64(max(0, capture.clock.Now().Sub(capture.started).Microseconds()))
		capture.mu.Lock()
		capture.dispatches++
		capture.dispatch = &elapsed
		capture.mu.Unlock()
	}
}

func captureHeaders(ctx context.Context, status int) {
	if capture, ok := ctx.Value(attemptCaptureKey{}).(*attemptCapture); ok {
		elapsed := uint64(max(0, capture.clock.Now().Sub(capture.started).Microseconds()))
		code := uint16(status)
		capture.mu.Lock()
		capture.response, capture.status = &elapsed, &code
		capture.mu.Unlock()
	}
}

func (capture *attemptCapture) apply(attempt *Attempt) {
	capture.mu.Lock()
	defer capture.mu.Unlock()
	attempt.ResponseUs = capture.response
	attempt.RequestBytes = capture.requestBytes
	attempt.DispatchUs = capture.dispatch
	attempt.RequestSentUs = capture.sent
	attempt.Categories = append([]protocol.ConversationObservationCategory(nil), capture.prepared.Categories...)
	attempt.CompleteContext = capture.prepared.Complete
	if capture.status != nil {
		attempt.Status = capture.status
	}
	if capture.trace != nil {
		var shape *observation.ResponsesShape
		// A second dispatch or write leaves more than one possible receipt.
		writeConfirmed := capture.writes == 0 || (capture.writes == 1 && capture.sent != nil)
		if capture.status != nil && capture.dispatches == 1 && writeConfirmed {
			shape = capture.shape
		}
		capture.shape = nil
		attempt.ResponsesBoundary = capture.trace.Record(attempt.RequestID, attempt.Index+1, attempt.Usage.CacheRead, shape)
	}
}
