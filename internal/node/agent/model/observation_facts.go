package model

import (
	"context"
	"net/http/httptrace"
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
	requestBytes *uint64
	dispatch     *uint64
	sent         *uint64
	response     *uint64
	status       *uint16
}

func captureAttempt(ctx context.Context, call fantasy.Call, timer clock.Clock, started time.Time) (context.Context, *attemptCapture) {
	inventory, _ := call.ProviderOptions[observation.ProviderKey].(*observation.Inventory)
	capture := &attemptCapture{clock: timer, started: started, inventory: inventory}
	ctx = context.WithValue(ctx, attemptCaptureKey{}, capture)
	return httptrace.WithClientTrace(ctx, &httptrace.ClientTrace{WroteRequest: func(info httptrace.WroteRequestInfo) {
		if info.Err != nil {
			return
		}
		elapsed := uint64(max(0, capture.clock.Now().Sub(capture.started).Microseconds()))
		capture.mu.Lock()
		capture.sent = &elapsed
		capture.mu.Unlock()
	}}), capture
}

// Transport hooks retain only request attribution and measured receipt facts.
func capturePrepared(ctx context.Context, raw []byte, serializedBody bool) {
	if capture, ok := ctx.Value(attemptCaptureKey{}).(*attemptCapture); ok {
		prepared := observation.Prepared(raw, capture.inventory)
		size := uint64(len(raw))
		capture.mu.Lock()
		capture.prepared = prepared
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
}
