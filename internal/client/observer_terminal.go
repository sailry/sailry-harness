package client

import (
	"context"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// terminalView is the latest state of one terminal. A screen sent as a VT
// stream holds the bytes of its own update only, which continue the update
// before it, so a view is enough for a reader of the parsed rows and the
// terminal's info but not for one that replays VT bytes.
type terminalView struct {
	Snapshot   *protocol.TerminalSnapshot `json:"snapshot"`
	Connected  bool                       `json:"connected"`
	Recovering bool                       `json:"recovering"`
	Error      *protocol.Fault            `json:"error"`
}

// watchTerminalView follows one terminal's screen. It subscribes again after
// a gap or a transient loss and ends on any other failure. Recovery only
// reads: it never claims input ownership or replays input.
func (client *Client) watchTerminalView(ctx context.Context, terminal protocol.TerminalID) *Observer {
	observer := newObserver(ctx)
	var state terminalView
	end := observer.ending(func() any { return state })
	observer.run(func() {
		scoped, release := client.interruptible(observer.ctx)
		defer release()
		projection := newTerminalProjection(client.Target(), terminal)
		err := reconnecting(scoped, func() (bool, bool, error) {
			projection.reconnect()
			stream, err := client.transport.Subscribe(scoped, protocol.Topic{Kind: "terminal", Data: terminal})
			progressed := false
			for err == nil {
				var update protocol.Update
				if update, err = stream.Next(scoped); err != nil {
					break
				}
				var applied Apply
				if applied, err = projection.apply(update); err != nil || applied == Recover {
					break
				}
				if applied == Applied {
					progressed = true
					observer.mu.Lock()
					state = terminalView{Snapshot: projection.snapshot, Connected: true}
					observer.mu.Unlock()
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
			return progressed, false, err
		})
		observer.mu.Lock()
		state.Connected, state.Recovering = false, false
		observer.mu.Unlock()
		end(err)
	})
	return observer
}

// reconnecting runs attempt until it reports that it finished or fails with
// anything but a transient fault, and returns that failure; it returns nil
// once ctx ends. Attempts are 250 ms apart, doubling to 5 s; one that made
// progress starts over at 250 ms.
func reconnecting(ctx context.Context, attempt func() (progressed, finished bool, err error)) error {
	delay := 250 * time.Millisecond
	for {
		progressed, finished, err := attempt()
		if finished || ctx.Err() != nil {
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
