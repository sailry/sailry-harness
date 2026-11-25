package model

import "context"

type RequestOutcome string

const (
	RequestRecovered RequestOutcome = "recovered"
	RequestFailed    RequestOutcome = "failed"
)

// RequestObservation contains classifications and counters, never provider
// diagnostics, model names or request identity. Attempts include the first call.
type RequestObservation struct {
	Outcome  RequestOutcome
	Attempts uint64
	Failure  FailureKind
	Status   uint16
}

type requestObserverKey struct{}

func WithRequestObserver(ctx context.Context, observe func(RequestObservation)) context.Context {
	return context.WithValue(ctx, requestObserverKey{}, observe)
}

func RequestObserver(ctx context.Context) func(RequestObservation) {
	observe, _ := ctx.Value(requestObserverKey{}).(func(RequestObservation))
	return observe
}

type requestObservation struct {
	attempts  uint64
	failure   FailureKind
	status    uint16
	cancelled bool
}

func (r *requestObservation) record(attempt Attempt) {
	r.attempts = attempt.Index + 1
	if attempt.Failure == nil {
		return
	}
	r.cancelled = attempt.Failure.Kind == Cancelled
	r.failure, r.status = attempt.Failure.Kind, 0
	if attempt.Status != nil && *attempt.Status >= 100 && *attempt.Status <= 599 {
		r.status = *attempt.Status
	}
}

func (r *requestObservation) finish(ctx context.Context, failed bool) bool {
	observe := RequestObserver(ctx)
	if observe == nil || ctx.Err() != nil || r.cancelled || r.failure == "" {
		return false
	}
	outcome := RequestRecovered
	if failed {
		outcome = RequestFailed
	}
	observe(RequestObservation{Outcome: outcome, Attempts: r.attempts, Failure: r.failure, Status: r.status})
	return true
}
