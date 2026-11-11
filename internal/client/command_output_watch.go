package client

import (
	"context"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type commandOutputKey struct {
	session protocol.SessionID
	command protocol.RequestID
}
type CommandOutputState struct {
	Snapshot protocol.ProcessSnapshot
	Known    bool
	Err      error
}
type commandOutputWatch struct {
	ctx       context.Context
	cancel    context.CancelFunc
	key       commandOutputKey
	state     CommandOutputState
	callbacks map[uint64]func()
	next      uint64
}

func (c *Client) CommandOutput(session protocol.SessionID, id protocol.RequestID) (CommandOutputState, bool) {
	c.commandsMu.Lock()
	defer c.commandsMu.Unlock()
	w := c.commandOutputs[commandOutputKey{session, id}]
	if w == nil {
		return CommandOutputState{}, false
	}
	return copyValue(w.state), true
}
func (c *Client) ObserveCommandOutput(ctx context.Context, session protocol.SessionID, id protocol.RequestID, changed func()) (func(), error) {
	c.lifecycle.Lock()
	defer c.lifecycle.Unlock()
	if c.closed {
		return nil, fault(protocol.ErrorCodeUnavailable, "Client is closed")
	}
	c.commandsMu.Lock()
	if c.commandOutputs == nil {
		c.commandOutputs = map[commandOutputKey]*commandOutputWatch{}
	}
	key := commandOutputKey{session, id}
	w := c.commandOutputs[key]
	if w == nil || w.ctx.Err() != nil {
		parent := c.context
		if parent == nil {
			parent = ctx
		}
		owner, cancel := context.WithCancel(parent)
		w = &commandOutputWatch{ctx: owner, cancel: cancel, key: key, callbacks: map[uint64]func(){}}
		c.commandOutputs[key] = w
		c.commandOwners.Add(1)
		go c.observeCommandOutput(w)
	}
	w.next++
	token := w.next
	w.callbacks[token] = changed
	c.commandsMu.Unlock()
	var once sync.Once
	release := func() {
		once.Do(func() {
			c.commandsMu.Lock()
			delete(w.callbacks, token)
			if len(w.callbacks) == 0 {
				w.cancel()
				if c.commandOutputs[key] == w {
					delete(c.commandOutputs, key)
				}
			}
			c.commandsMu.Unlock()
		})
	}
	stop := context.AfterFunc(ctx, release)
	return func() { stop(); release() }, nil
}
func (c *Client) observeCommandOutput(w *commandOutputWatch) {
	defer c.commandOwners.Done()
	wait := uint64(0)
	for w.ctx.Err() == nil {
		output, err := c.Do(w.ctx, protocol.Command{Kind: "read_command", Data: protocol.CommandReadCommandPayload{Session: w.key.session, ID: w.key.command, WaitMs: wait}})
		var snapshot protocol.ProcessSnapshot
		if err == nil {
			snapshot, err = payload[protocol.ProcessSnapshot](output.Data)
			if err == nil && (output.Kind != "command_output" || snapshot.Info.ID != w.key.command || snapshot.Info.Session != w.key.session) {
				err = fault(protocol.ErrorCodeInvalidRequest, "unexpected command output")
			}
		}
		if w.ctx.Err() != nil {
			return
		}
		c.commandsMu.Lock()
		w.state.Err = err
		if err == nil {
			w.state.Snapshot, w.state.Known = snapshot, true
		}
		callbacks := make([]func(), 0, len(w.callbacks))
		for _, f := range w.callbacks {
			callbacks = append(callbacks, f)
		}
		c.commandsMu.Unlock()
		for _, f := range callbacks {
			if f != nil {
				f()
			}
		}
		c.changed()
		if err != nil || snapshot.Info.Status.Kind != "running" && snapshot.Info.Status.Kind != "stopping" {
			return
		}
		wait = 30000
	}
}
