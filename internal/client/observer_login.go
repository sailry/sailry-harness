package client

import (
	"context"
	"reflect"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type loginView[U any] struct {
	Update    *U              `json:"update"`
	Connected bool            `json:"connected"`
	Error     *protocol.Fault `json:"error"`
}

// loginProjection keeps the latest update of one authorization attempt,
// provider or connector. facts reads an update's attempt, revision and state.
type loginProjection[A comparable, U any] struct {
	attempt A
	facts   func(U) (A, uint64, string)
	update  *U
}

// active reports whether the attempt can still change.
func (projection *loginProjection[A, U]) active() bool {
	if projection.update == nil {
		return true
	}
	_, _, state := projection.facts(*projection.update)
	return state != "connected" && state != "failed" && state != "cancelled"
}

func (projection *loginProjection[A, U]) apply(update U) (Apply, error) {
	attempt, revision, _ := projection.facts(update)
	if attempt != projection.attempt || revision == 0 {
		return Ignored, fault(protocol.ErrorCodeInvalidRequest, "authorization update belongs to another attempt or has an invalid revision")
	}
	if projection.update != nil {
		_, current, _ := projection.facts(*projection.update)
		if revision < current {
			return Ignored, nil
		}
		if revision == current {
			if reflect.DeepEqual(update, *projection.update) {
				return Ignored, nil
			}
			return Ignored, fault(protocol.ErrorCodeInvalidRequest, "authorization revision changed its contents")
		}
		if !projection.active() {
			return Ignored, fault(protocol.ErrorCodeInvalidRequest, "completed authorization cannot change")
		}
	}
	projection.update = &update
	return Applied, nil
}

// watchLoginView follows one authorization attempt on the topic named kind,
// whose updates carry the same kind. It subscribes again after a transient
// loss and ends once the attempt can no longer change or on any other
// failure. Observing never starts or repeats an authorization.
func watchLoginView[A comparable, U any](ctx context.Context, client *Client, kind string, id protocol.RequestID, attempt A, facts func(U) (A, uint64, string)) *Observer {
	observer := newObserver(ctx)
	var state loginView[U]
	end := observer.ending(func() any { return state })
	observer.run(func() {
		scoped, release := client.interruptible(observer.ctx)
		defer release()
		projection := &loginProjection[A, U]{attempt: attempt, facts: facts}
		err := reconnecting(scoped, func() (bool, bool, error) {
			stream, err := client.transport.Subscribe(scoped, protocol.Topic{Kind: kind, Data: id})
			progressed := false
			for err == nil {
				var update protocol.Update
				if update, err = stream.Next(scoped); err != nil {
					break
				}
				if update.Kind != kind {
					err = fault(protocol.ErrorCodeInvalidRequest, "authorization update expected")
					break
				}
				var value U
				if value, err = payload[U](update.Data); err != nil {
					break
				}
				if _, err = projection.apply(value); err != nil {
					break
				}
				// An update the projection already holds still shows the
				// attempt is observed again after a loss.
				progressed = true
				observer.mu.Lock()
				state = loginView[U]{Update: projection.update, Connected: true}
				observer.mu.Unlock()
				observer.notify()
				if !projection.active() {
					_ = stream.Close()
					return true, true, nil
				}
			}
			if stream != nil {
				_ = stream.Close()
			}
			if scoped.Err() == nil {
				observer.mu.Lock()
				state.Connected, state.Error = false, observerFault(err)
				observer.mu.Unlock()
				observer.notify()
			}
			return progressed, false, err
		})
		observer.mu.Lock()
		state.Connected = false
		observer.mu.Unlock()
		end(err)
	})
	return observer
}
