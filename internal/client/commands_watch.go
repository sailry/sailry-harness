package client

import (
	"context"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type CommandsState struct {
	Items     []protocol.ProcessInfo
	Connected bool
	Revision  uint64
	Err       error
}
type commandsWatch struct {
	ctx       context.Context
	cancel    context.CancelFunc
	session   protocol.SessionID
	state     CommandsState
	callbacks map[uint64]func()
	next      uint64
}

func (c *Client) Commands(session protocol.SessionID) (CommandsState, bool) {
	c.commandsMu.Lock()
	defer c.commandsMu.Unlock()
	w := c.commands[session]
	if w == nil {
		return CommandsState{}, false
	}
	return copyValue(w.state), true
}
func (c *Client) ObserveCommands(ctx context.Context, session protocol.SessionID, changed func()) (func(), error) {
	c.lifecycle.Lock()
	defer c.lifecycle.Unlock()
	if c.closed {
		return nil, fault(protocol.ErrorCodeUnavailable, "Client is closed")
	}
	c.commandsMu.Lock()
	if c.commands == nil {
		c.commands = map[protocol.SessionID]*commandsWatch{}
	}
	w := c.commands[session]
	if w == nil || w.ctx.Err() != nil {
		parent := c.context
		if parent == nil {
			parent = ctx
		}
		owner, cancel := context.WithCancel(parent)
		w = &commandsWatch{ctx: owner, cancel: cancel, session: session, callbacks: map[uint64]func(){}}
		c.commands[session] = w
		c.commandOwners.Add(1)
		go c.observeCommands(w)
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
				// An unobserved watch retains nothing; a later observer starts a new owner.
				if c.commands[session] == w {
					delete(c.commands, session)
				}
			}
			c.commandsMu.Unlock()
		})
	}
	stop := context.AfterFunc(ctx, release)
	return func() { stop(); release() }, nil
}
func (c *Client) commandsChanged(w *commandsWatch) {
	c.commandsMu.Lock()
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
}
func (c *Client) observeCommands(w *commandsWatch) {
	defer c.commandOwners.Done()
	defer func() { c.commandsMu.Lock(); w.state.Connected = false; c.commandsMu.Unlock(); c.commandsChanged(w) }()
	delay := 250 * time.Millisecond
	for w.ctx.Err() == nil {
		subscription, err := c.transport.Subscribe(w.ctx, protocol.Topic{Kind: "commands", Data: w.session})
		if err == nil {
			for w.ctx.Err() == nil {
				update, e := subscription.Next(w.ctx)
				err = e
				if err != nil {
					break
				}
				value, e := payload[protocol.UpdateCommandsPayload](update.Data)
				if update.Kind != "commands" || e != nil || value.Node != c.Target() || value.Session != w.session {
					err = fault(protocol.ErrorCodeInvalidRequest, "unexpected commands update")
					break
				}
				valid := true
				for _, item := range value.Items {
					if item.Session != w.session {
						valid = false
						break
					}
				}
				if !valid {
					err = fault(protocol.ErrorCodeInvalidRequest, "command belongs to another session")
					break
				}
				c.commandsMu.Lock()
				w.state.Items = copyValue(value.Items)
				w.state.Revision++
				w.state.Connected, w.state.Err = true, nil
				c.commandsMu.Unlock()
				delay = 250 * time.Millisecond
				c.commandsChanged(w)
			}
			_ = subscription.Close()
		}
		if w.ctx.Err() != nil {
			return
		}
		c.commandsMu.Lock()
		w.state.Connected, w.state.Err = false, err
		c.commandsMu.Unlock()
		c.commandsChanged(w)
		if !transient(err) {
			return
		}
		wait := make(chan struct{})
		timer := time.AfterFunc(delay, func() { close(wait) })
		select {
		case <-w.ctx.Done():
			timer.Stop()
			return
		case <-wait:
		}
		delay = min(delay*2, 5*time.Second)
	}
}
func (c *Client) closeCommands() {
	c.commandsMu.Lock()
	for _, w := range c.commands {
		w.cancel()
	}
	for _, w := range c.commandOutputs {
		w.cancel()
	}
	c.commandsMu.Unlock()
	c.commandOwners.Wait()
}
