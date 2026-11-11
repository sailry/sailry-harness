package client

import (
	"context"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type WorkspaceState struct {
	Revision, References uint64
	Connected            bool
	Err                  error
}
type workspaceKey struct {
	worktree   protocol.WorktreeID
	repository bool
}
type workspaceWatch struct {
	key       workspaceKey
	ctx       context.Context
	cancel    context.CancelFunc
	state     WorkspaceState
	callbacks map[uint64]func(protocol.Update)
	next      uint64
}

func (c *Client) Workspace(id protocol.WorktreeID, repository bool) (WorkspaceState, bool) {
	c.workspaceMu.Lock()
	defer c.workspaceMu.Unlock()
	w := c.workspaces[workspaceKey{id, repository}]
	if w == nil {
		return WorkspaceState{}, false
	}
	return w.state, true
}

// ObserveWorkspace shares invalidation ownership across panels and panes. Each
// callback reads only its loaded resources; missed intermediate changes coalesce.
func (c *Client) ObserveWorkspace(ctx context.Context, id protocol.WorktreeID, repository bool, changed func(protocol.Update)) (func(), error) {
	c.lifecycle.Lock()
	defer c.lifecycle.Unlock()
	if c.closed {
		return nil, fault(protocol.ErrorCodeUnavailable, "Client is closed")
	}
	c.workspaceMu.Lock()
	if c.workspaces == nil {
		c.workspaces = map[workspaceKey]*workspaceWatch{}
	}
	key := workspaceKey{id, repository}
	w := c.workspaces[key]
	if w == nil || w.ctx.Err() != nil {
		parent := c.context
		if parent == nil {
			parent = ctx
		}
		owner, cancel := context.WithCancel(parent)
		w = &workspaceWatch{key: key, ctx: owner, cancel: cancel, callbacks: map[uint64]func(protocol.Update){}}
		c.workspaces[key] = w
		c.workspaceOwners.Add(1)
		go c.observeWorkspace(w)
	}
	w.next++
	token := w.next
	w.callbacks[token] = changed
	c.workspaceMu.Unlock()
	var once sync.Once
	release := func() {
		once.Do(func() {
			c.workspaceMu.Lock()
			delete(w.callbacks, token)
			if len(w.callbacks) == 0 {
				w.cancel()
				if c.workspaces[key] == w {
					delete(c.workspaces, key)
				}
			}
			c.workspaceMu.Unlock()
		})
	}
	stop := context.AfterFunc(ctx, release)
	return func() { stop(); release() }, nil
}
func (c *Client) workspaceChanged(w *workspaceWatch, update protocol.Update) {
	c.workspaceMu.Lock()
	callbacks := make([]func(protocol.Update), 0, len(w.callbacks))
	for _, callback := range w.callbacks {
		callbacks = append(callbacks, callback)
	}
	c.workspaceMu.Unlock()
	for _, callback := range callbacks {
		if callback != nil {
			callback(copyValue(update))
		}
	}
	c.changed()
}
func (c *Client) observeWorkspace(w *workspaceWatch) {
	defer c.workspaceOwners.Done()
	defer func() {
		c.workspaceMu.Lock()
		w.state.Connected = false
		c.workspaceMu.Unlock()
		c.workspaceChanged(w, protocol.Update{Kind: "reset_required"})
	}()
	topic := "files"
	if w.key.repository {
		topic = "repository"
	}
	delay := 250 * time.Millisecond
	for w.ctx.Err() == nil {
		subscription, err := c.transport.Subscribe(w.ctx, protocol.Topic{Kind: topic, Data: w.key.worktree})
		var previous *uint64
		if err == nil {
			for w.ctx.Err() == nil {
				var update protocol.Update
				update, err = subscription.Next(w.ctx)
				if err != nil {
					break
				}
				value, e := payload[protocol.UpdateFilesChangedPayload](update.Data)
				if update.Kind != "files_changed" || e != nil || value.Node != c.Target() || value.Worktree != w.key.worktree {
					err = fault(protocol.ErrorCodeInvalidRequest, "unexpected file subscription update")
					break
				}
				c.workspaceMu.Lock()
				if w.state.Revision == ^uint64(0) || w.state.References == ^uint64(0) {
					c.workspaceMu.Unlock()
					err = fault(protocol.ErrorCodeInternal, "workspace revision exhausted")
					break
				}
				w.state.Revision++
				if previous == nil || *previous != value.References {
					w.state.References++
					reference := value.References
					previous = &reference
				}
				w.state.Connected, w.state.Err = true, nil
				c.workspaceMu.Unlock()
				delay = 250 * time.Millisecond
				c.workspaceChanged(w, update)
			}
			_ = subscription.Close()
		}
		if w.ctx.Err() != nil {
			return
		}
		c.workspaceMu.Lock()
		w.state.Connected, w.state.Err = false, err
		c.workspaceMu.Unlock()
		c.workspaceChanged(w, protocol.Update{Kind: "reset_required"})
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
func (c *Client) closeWorkspaces() {
	c.workspaceMu.Lock()
	for _, w := range c.workspaces {
		w.cancel()
	}
	c.workspaceMu.Unlock()
	c.workspaceOwners.Wait()
}
