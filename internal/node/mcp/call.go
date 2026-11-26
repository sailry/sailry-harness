package mcp

import (
	"context"
	"errors"
	"sync"
	"time"

	"github.com/modelcontextprotocol/go-sdk/jsonrpc"
	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/sync/semaphore"
)

type Elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)

type activeCallKey struct{}

type activeCall struct {
	mu      sync.Mutex
	owner   *connection
	ctx     context.Context
	handler Elicit
	paused  int
	spent   time.Duration
	last    time.Time
	changed chan struct{}
}

func (active *activeCall) pause(delta int) {
	active.mu.Lock()
	if active.paused == 0 {
		active.spent += active.owner.pool.clock.Since(active.last)
	}
	active.paused += delta
	active.last = active.owner.pool.clock.Now()
	active.mu.Unlock()
	select {
	case active.changed <- struct{}{}:
	default:
	}
}

func (active *activeCall) remaining() (time.Duration, bool) {
	active.mu.Lock()
	defer active.mu.Unlock()
	spent := active.spent
	if active.paused == 0 {
		spent += active.owner.pool.clock.Since(active.last)
	}
	return 120*time.Second - spent, active.paused != 0
}

func (connection *connection) elicit(ctx context.Context, request *sdk.ElicitRequest) (*sdk.ElicitResult, error) {
	active, _ := ctx.Value(activeCallKey{}).(*activeCall)
	if active == nil || active.owner != connection || active.handler == nil {
		return nil, invalid("MCP input requires an originating tool-call context")
	}
	if active.ctx.Err() != nil {
		return nil, cancelled()
	}
	active.pause(1)
	defer active.pause(-1)
	return active.handler(ctx, request)
}

func (connection *connection) lock(ctx context.Context, lock *semaphore.Weighted, weight int64) error {
	waiting, cancel := context.WithCancel(ctx)
	defer cancel()
	stop := context.AfterFunc(connection.pool.ctx, cancel)
	defer stop()
	timer := connection.pool.clock.AfterFunc(120*time.Second, cancel)
	defer timer.Stop()
	if err := lock.Acquire(waiting, weight); err == nil {
		return nil
	}
	if ctx.Err() != nil || connection.pool.ctx.Err() != nil {
		return cancelled()
	}
	return &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "MCP server is busy"}
}

func (lease *Lease) Call(ctx context.Context, name string, arguments map[string]any, readOnly bool, handler Elicit) (*sdk.CallToolResult, error) {
	if ctx.Err() != nil || lease.pool.ctx.Err() != nil {
		return nil, cancelled()
	}
	connection := lease.connection
	connection.mu.Lock()
	poisoned := connection.poisoned
	connection.mu.Unlock()
	if poisoned {
		return nil, unavailable()
	}
	exclusive := lease.config.Options.Exclusive && !readOnly
	if exclusive {
		if err := connection.lock(ctx, connection.exclusive.lock, 1); err != nil {
			return nil, err
		}
	}
	weight := int64(maxCalls)
	if readOnly {
		weight = 1
	}
	if err := connection.lock(ctx, connection.calls, weight); err != nil {
		if exclusive {
			connection.exclusive.lock.Release(1)
		}
		return nil, err
	}
	released := false
	defer func() {
		if !released {
			connection.calls.Release(weight)
			if exclusive {
				connection.exclusive.lock.Release(1)
			}
		}
	}()
	if ctx.Err() != nil || lease.pool.ctx.Err() != nil {
		return nil, cancelled()
	}
	connection.mu.Lock()
	poisoned = connection.poisoned
	connection.mu.Unlock()
	if poisoned {
		return nil, unavailable()
	}
	if lease.config.Validate != nil {
		if err := lease.config.Validate(ctx); err != nil {
			return nil, err
		}
	}
	operation, cancel := context.WithCancel(ctx)
	defer cancel()
	active := &activeCall{
		owner: connection, ctx: operation, handler: handler, last: lease.pool.clock.Now(), changed: make(chan struct{}, 1),
	}
	operation = context.WithValue(operation, activeCallKey{}, active)
	type response struct {
		value *sdk.CallToolResult
		err   error
	}
	completed := make(chan response, 1)
	// Both sides must finish before another call enters. Cancellation can
	// return before the transport stops, and a transport failure can finish
	// before the caller marks the connection's write outcome uncertain.
	classified := make(chan struct{})
	defer close(classified)
	released = true
	go func() {
		defer func() {
			connection.calls.Release(weight)
			if exclusive {
				connection.exclusive.lock.Release(1)
			}
		}()
		value, err := connection.session.CallTool(operation, &sdk.CallToolParams{Name: name, Arguments: arguments})
		completed <- response{value, err}
		<-classified
	}()
	timer := lease.pool.clock.NewTimer(120 * time.Second)
	defer timer.Stop()
	for {
		select {
		case result := <-completed:
			if result.err != nil {
				if ctx.Err() != nil || lease.pool.ctx.Err() != nil {
					return nil, connection.interrupted(readOnly, false)
				}
				var remote *jsonrpc.Error
				var responseFailure *responseReadError
				if errors.As(result.err, &remote) && !errors.As(result.err, &responseFailure) {
					return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "MCP server rejected the tool request"}
				}
				if readOnly {
					// Input-handler and MRTR errors belong to this call. The
					// session watcher retires actual transport failures.
					return nil, protocolFault(result.err)
				}
				connection.mu.Lock()
				connection.poisoned = true
				connection.mu.Unlock()
				return nil, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "MCP call failed; remote effects may be uncertain"}
			}
			return result.value, nil
		case <-active.changed:
			remaining, paused := active.remaining()
			timer.Stop()
			if !paused {
				timer.Reset(max(0, remaining))
			}
		case <-timer.Chan():
			remaining, paused := active.remaining()
			if paused {
				continue
			}
			if remaining > 0 {
				timer.Reset(remaining)
				continue
			}
			cancel()
			return nil, connection.interrupted(readOnly, true)
		case <-ctx.Done():
			cancel()
			return nil, connection.interrupted(readOnly, false)
		case <-lease.pool.ctx.Done():
			cancel()
			return nil, connection.interrupted(readOnly, false)
		}
	}
}

func (connection *connection) interrupted(readOnly, timedOut bool) error {
	// An interrupted write may still be executing remotely. Retire its
	// connection without replay; cancelling a read leaves other calls intact.
	if !readOnly {
		connection.mu.Lock()
		connection.poisoned = true
		connection.mu.Unlock()
	}
	if readOnly {
		if timedOut {
			return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "MCP read timed out"}
		}
		return cancelled()
	}
	message := "MCP call was cancelled; remote effects may be uncertain"
	if timedOut {
		message = "MCP call timed out; remote effects may be uncertain"
	}
	return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: message}
}
