package mcp

import (
	"context"
	"errors"
	"sync"
	"time"

	"github.com/modelcontextprotocol/go-sdk/jsonrpc"
	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Elicit func(context.Context, *sdk.ElicitRequest) (*sdk.ElicitResult, error)

type activeCall struct {
	mu      sync.Mutex
	pool    *Pool
	handler Elicit
	paused  int
	spent   time.Duration
	last    time.Time
	changed chan struct{}
}

func (active *activeCall) pause(delta int) {
	active.mu.Lock()
	if active.paused == 0 {
		active.spent += active.pool.clock.Since(active.last)
	}
	active.paused += delta
	active.last = active.pool.clock.Now()
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
		spent += active.pool.clock.Since(active.last)
	}
	return 120*time.Second - spent, active.paused != 0
}

func (connection *connection) elicit(ctx context.Context, request *sdk.ElicitRequest) (*sdk.ElicitResult, error) {
	connection.mu.Lock()
	active := connection.active
	connection.mu.Unlock()
	if active == nil || active.handler == nil {
		return nil, invalid("MCP input requires an active tool call")
	}
	active.pause(1)
	defer active.pause(-1)
	return active.handler(ctx, request)
}

func (connection *connection) lock(ctx context.Context, lock chan struct{}) error {
	timer := connection.pool.clock.NewTimer(120 * time.Second)
	defer timer.Stop()
	select {
	case lock <- struct{}{}:
		return nil
	case <-timer.Chan():
		return &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "MCP server is busy"}
	case <-ctx.Done():
		return cancelled()
	case <-connection.pool.ctx.Done():
		return cancelled()
	}
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
	if lease.config.Options.Exclusive && !readOnly {
		if err := connection.lock(ctx, connection.exclusive.lock); err != nil {
			return nil, err
		}
		defer func() { <-connection.exclusive.lock }()
	}
	if err := connection.lock(ctx, connection.serial); err != nil {
		return nil, err
	}
	defer func() { <-connection.serial }()
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
		pool: lease.pool, handler: handler, last: lease.pool.clock.Now(), changed: make(chan struct{}, 1),
	}
	connection.mu.Lock()
	connection.active = active
	connection.mu.Unlock()
	defer func() {
		connection.mu.Lock()
		connection.active = nil
		connection.mu.Unlock()
	}()
	type response struct {
		value *sdk.CallToolResult
		err   error
	}
	completed := make(chan response, 1)
	go func() {
		value, err := connection.session.CallTool(operation, &sdk.CallToolParams{Name: name, Arguments: arguments})
		completed <- response{value, err}
	}()
	timer := lease.pool.clock.NewTimer(120 * time.Second)
	defer timer.Stop()
	for {
		select {
		case result := <-completed:
			if result.err != nil {
				var remote *jsonrpc.Error
				if errors.As(result.err, &remote) {
					return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "MCP server rejected the tool request"}
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
			connection.mu.Lock()
			connection.poisoned = true
			connection.mu.Unlock()
			return nil, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "MCP call timed out; remote effects may be uncertain"}
		case <-ctx.Done():
			cancel()
			connection.mu.Lock()
			connection.poisoned = true
			connection.mu.Unlock()
			return nil, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "MCP call was cancelled; remote effects may be uncertain"}
		case <-lease.pool.ctx.Done():
			cancel()
			return nil, cancelled()
		}
	}
}
