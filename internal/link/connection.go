package link

import (
	"context"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/link/iroh"
)

type connectionTelemetry struct {
	mu      sync.Mutex
	value   ConnectionInfo
	changed chan struct{}
	closed  bool
	cancel  context.CancelFunc
	done    chan struct{}
}

func connectionTicks() (<-chan time.Time, func()) {
	ticker := time.NewTicker(time.Second)
	return ticker.C, ticker.Stop
}

func newConnectionTelemetry(ctx context.Context, initial ConnectionInfo, sample func(context.Context) (iroh.ConnectionInfo, error), waitPath func(context.Context) error, ticks func() (<-chan time.Time, func())) *connectionTelemetry {
	lifetime, cancel := context.WithCancel(ctx)
	state := &connectionTelemetry{
		value: copyConnectionInfo(initial), changed: make(chan struct{}), done: make(chan struct{}), cancel: cancel,
	}
	go func() {
		defer close(state.done)
		defer state.close()
		pathChanges := make(chan struct{}, 1)
		if waitPath != nil {
			pathDone := make(chan struct{})
			go func() {
				defer close(pathDone)
				for lifetime.Err() == nil {
					err := waitPath(lifetime)
					select {
					case pathChanges <- struct{}{}:
					case <-lifetime.Done():
						return
					}
					if err != nil {
						return
					}
				}
			}()
			defer func() {
				cancel()
				<-pathDone
			}()
		}
		ticks, stop := ticks()
		defer stop()
		for {
			select {
			case <-lifetime.Done():
				return
			case <-ticks:
			case <-pathChanges:
			}
			info, err := sample(lifetime)
			if err != nil || info.Closed != nil {
				return
			}
			state.publish(selectedConnectionInfo(info))
		}
	}()
	return state
}

func selectedConnectionInfo(info iroh.ConnectionInfo) ConnectionInfo {
	if info.Closed != nil || info.Route != "p2p" && info.Route != "relay" {
		return ConnectionInfo{}
	}
	route := Route(info.Route)
	return copyConnectionInfo(ConnectionInfo{Route: &route, RTTMS: info.RTTMS})
}

func (state *connectionTelemetry) publish(value ConnectionInfo) {
	state.mu.Lock()
	defer state.mu.Unlock()
	if !state.closed && !equalConnectionInfo(state.value, value) {
		state.value = copyConnectionInfo(value)
		close(state.changed)
		state.changed = make(chan struct{})
	}
}

func (state *connectionTelemetry) snapshot() (ConnectionInfo, <-chan struct{}, bool) {
	state.mu.Lock()
	defer state.mu.Unlock()
	return copyConnectionInfo(state.value), state.changed, state.closed
}

func (state *connectionTelemetry) close() {
	state.cancel()
	state.mu.Lock()
	defer state.mu.Unlock()
	if !state.closed {
		state.closed = true
		state.value = ConnectionInfo{}
		close(state.changed)
	}
}

func (remote *Remote) WatchConnection(ctx context.Context, callback func(ConnectionInfo)) error {
	if callback == nil {
		return invalidLink("connection observer callback is required")
	}
	last := ConnectionInfo{}
	callback(last)
	deliver := func(value ConnectionInfo) {
		if !equalConnectionInfo(last, value) {
			last = copyConnectionInfo(value)
			callback(copyConnectionInfo(value))
		}
	}
	defer func() { deliver(ConnectionInfo{}) }()
	link := remote.link
	scoped, cancel := link.operation(ctx, false)
	defer cancel()
	for {
		if scoped.Err() != nil {
			return scoped.Err()
		}
		if _, _, err := link.connection(scoped, remote.address, false); err == nil {
			link.mu.RLock()
			slot := link.outgoing[remote.address.ID]
			link.mu.RUnlock()
			var state *connectionTelemetry
			if slot != nil {
				slot.mu.Lock()
				state = slot.telemetry
				slot.mu.Unlock()
			}
			for state != nil {
				value, changed, closed := state.snapshot()
				deliver(value)
				if closed {
					break
				}
				select {
				case <-scoped.Done():
					return scoped.Err()
				case <-changed:
				}
			}
		}
		deliver(ConnectionInfo{})
		ticks := link.ticks
		if ticks == nil {
			ticks = connectionTicks
		}
		retry, stop := ticks()
		select {
		case <-scoped.Done():
			stop()
			return scoped.Err()
		case <-retry:
			stop()
		}
	}
}

func equalConnectionInfo(first, second ConnectionInfo) bool {
	if first.Route == nil != (second.Route == nil) || first.RTTMS == nil != (second.RTTMS == nil) {
		return false
	}
	return (first.Route == nil || *first.Route == *second.Route) && (first.RTTMS == nil || *first.RTTMS == *second.RTTMS)
}

func copyConnectionInfo(value ConnectionInfo) ConnectionInfo {
	if value.Route != nil {
		route := *value.Route
		value.Route = &route
	}
	if value.RTTMS != nil {
		rtt := *value.RTTMS
		value.RTTMS = &rtt
	}
	return value
}
