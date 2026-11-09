package model

import (
	"context"
	"sync"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/clock"
	"github.com/sailry/sailry-harness/internal/node/agent/observation"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type attemptCaptureKey struct{}

type attemptCapture struct {
	mu        sync.Mutex
	clock     clock.Clock
	started   time.Time
	inventory *observation.Inventory
	prepared  observation.Result
	response  *uint64
	status    *uint16
}

func captureAttempt(ctx context.Context, call fantasy.Call, timer clock.Clock, started time.Time) (context.Context, *attemptCapture) {
	inventory, _ := call.ProviderOptions[observation.ProviderKey].(*observation.Inventory)
	capture := &attemptCapture{clock: timer, started: started, inventory: inventory}
	return context.WithValue(ctx, attemptCaptureKey{}, capture), capture
}

// Transport hooks retain only request attribution and measured receipt facts.
func capturePrepared(ctx context.Context, raw []byte) {
	if capture, ok := ctx.Value(attemptCaptureKey{}).(*attemptCapture); ok {
		prepared := observation.Prepared(raw, capture.inventory)
		capture.mu.Lock()
		capture.prepared = prepared
		capture.response, capture.status = nil, nil
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
	attempt.Categories = append([]protocol.ConversationObservationCategory(nil), capture.prepared.Categories...)
	attempt.CompleteContext = capture.prepared.Complete
	if capture.status != nil {
		attempt.Status = capture.status
	}
}
