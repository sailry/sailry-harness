package client

import (
	"context"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// terminalView is one delivery of a terminal's state. A screen sent as a VT
// stream carries, in order, every byte no delivery before carried: its reader
// writes them to its own terminal once. Reset tells that those bytes start
// that terminal over, as the first of a subscription and the first of another
// viewport size do, so the reader clears it and sizes it to the viewport
// before writing them.
type terminalView struct {
	Snapshot   *protocol.TerminalSnapshot `json:"snapshot"`
	Reset      bool                       `json:"reset"`
	Connected  bool                       `json:"connected"`
	Recovering bool                       `json:"recovering"`
	Error      *protocol.Fault            `json:"error"`
}

// watchTerminalView follows one terminal's screen. It subscribes again after
// a gap or a transient loss and ends on any other failure. Recovery only
// reads: it never claims input ownership or replays input.
//
// A reader that stops taking deliveries holds at most terminalMaxPending VT
// bytes: past that they are dropped with the subscription, and the next one
// starts once the reader has taken a delivery again.
func (client *Client) watchTerminalView(ctx context.Context, terminal protocol.TerminalID) *Observer {
	observer := newObserver(ctx)
	projection := newTerminalProjection(client.Target(), terminal)
	var state terminalView
	// resume is closed by the delivery after the pending bytes were dropped.
	var resume chan struct{}
	end := observer.ending(func() any {
		view := state
		view.Snapshot, view.Reset = projection.take()
		if resume != nil {
			close(resume)
			resume = nil
		}
		return view
	})
	observer.run(func() {
		scoped, release := client.interruptible(observer.ctx)
		defer release()
		err := reconnecting(scoped, func() (bool, error) {
			observer.mu.Lock()
			projection.reconnect()
			taken := resume
			observer.mu.Unlock()
			if taken != nil {
				select {
				case <-taken:
				case <-scoped.Done():
					return false, nil
				}
			}
			stream, err := client.transport.Subscribe(scoped, protocol.Topic{Kind: "terminal", Data: terminal})
			progressed := false
			for err == nil {
				var update protocol.Update
				if update, err = stream.Next(scoped); err != nil {
					break
				}
				observer.mu.Lock()
				applied, failure := projection.apply(update)
				if applied == Applied {
					state = terminalView{Connected: true}
				} else if projection.stalled && resume == nil {
					resume = make(chan struct{})
				}
				observer.mu.Unlock()
				if err = failure; err != nil || applied == Recover {
					break
				}
				if applied == Applied {
					progressed = true
					observer.notify()
				}
			}
			if stream != nil {
				_ = stream.Close()
			}
			if scoped.Err() == nil {
				observer.mu.Lock()
				state.Connected, state.Recovering, state.Error = false, err == nil, observerFault(err)
				observer.mu.Unlock()
				observer.notify()
			}
			return progressed, err
		})
		observer.mu.Lock()
		state.Connected, state.Recovering = false, false
		observer.mu.Unlock()
		end(err)
	})
	return observer
}

// reconnecting runs attempt until it fails with anything but a transient
// fault, and returns that failure, or nil once ctx ends. Attempts are 250 ms
// apart, doubling to 5 s; one that made progress starts over at 250 ms.
func reconnecting(ctx context.Context, attempt func() (progressed bool, err error)) error {
	delay := 250 * time.Millisecond
	for {
		progressed, err := attempt()
		if ctx.Err() != nil {
			return nil
		}
		if err != nil && !transient(err) {
			return err
		}
		if progressed {
			delay = 250 * time.Millisecond
		}
		timer := time.NewTimer(delay)
		select {
		case <-ctx.Done():
			timer.Stop()
			return nil
		case <-timer.C:
		}
		delay = min(delay*2, 5*time.Second)
	}
}
