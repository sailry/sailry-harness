package client

import (
	"context"
	"errors"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type observation struct {
	subscription link.Subscription
	context      context.Context
	cancel       context.CancelFunc
	done         chan struct{}
	generation   uint64
}

// Start establishes the initial snapshot before returning to composition.
func (client *Client) Start(ctx context.Context) error {
	client.lifecycle.Lock()
	if client.closed {
		client.lifecycle.Unlock()
		return fault(protocol.ErrorCodeUnavailable, "Client is closed")
	}
	if client.context != nil {
		client.lifecycle.Unlock()
		return nil
	}
	client.context, client.stop = context.WithCancel(ctx)
	watch, err := client.openObservation(ctx)
	if err != nil {
		client.stop()
		client.context, client.stop = nil, nil
	} else {
		client.watch = watch
		go client.observe(watch)
		client.connectionDone = make(chan struct{})
		go client.observeConnection(client.context, client.connectionDone)
	}
	client.lifecycle.Unlock()
	client.changed()
	return err
}

// Refresh replaces the Node subscription; it never starts a second Node owner.
func (client *Client) Refresh(ctx context.Context) error {
	client.lifecycle.Lock()
	if client.closed || client.context == nil {
		client.lifecycle.Unlock()
		return fault(protocol.ErrorCodeUnavailable, "Client observation is unavailable")
	}
	// A failed attempt to establish the fresh route does not discard an existing
	// valid observation. Its initial read is consumed only after the old owner joins.
	subscription, err := client.transport.Subscribe(ctx, protocol.Topic{Kind: "node"})
	if err != nil {
		client.lifecycle.Unlock()
		return err
	}
	client.stopObservation()
	watch, err := client.initializeObservation(ctx, subscription)
	if err == nil {
		client.watch = watch
		go client.observe(watch)
	} else {
		client.mu.Lock()
		client.connected, client.err = false, err
		client.mu.Unlock()
	}
	client.lifecycle.Unlock()
	client.changed()
	return err
}

func (client *Client) Close() error {
	client.closePluginStatusObservers()
	client.lifecycle.Lock()
	if client.closed {
		client.lifecycle.Unlock()
		return nil
	}
	client.closed = true
	if client.stop != nil {
		client.stop()
	}
	err := client.stopObservation()
	client.closeTopics()
	client.closeForwards()
	client.closeConversations()
	client.closeWorkspaces()
	client.closeCommands()
	if client.connectionDone != nil {
		<-client.connectionDone
	}
	client.mu.Lock()
	client.connected = false
	client.mu.Unlock()
	client.lifecycle.Unlock()
	client.changed()
	return err
}

func (client *Client) observeConnection(ctx context.Context, done chan struct{}) {
	defer close(done)
	_ = client.transport.WatchConnection(ctx, func(info link.ConnectionInfo) {
		client.mu.Lock()
		client.connection = copyValue(info)
		client.mu.Unlock()
		client.changed()
	})
}

// lifecycle is held while replacing or joining the sole observation owner.
func (client *Client) stopObservation() error {
	watch := client.watch
	client.watch = nil
	if watch == nil {
		return nil
	}
	watch.cancel()
	err := watch.subscription.Close()
	<-watch.done
	return err
}

func (client *Client) openObservation(ctx context.Context) (*observation, error) {
	subscription, err := client.transport.Subscribe(ctx, protocol.Topic{Kind: "node"})
	if err != nil {
		return nil, err
	}
	return client.initializeObservation(ctx, subscription)
}

func (client *Client) initializeObservation(ctx context.Context, subscription link.Subscription) (*observation, error) {
	client.mu.Lock()
	if client.project.generation == ^uint64(0) {
		client.mu.Unlock()
		subscription.Close()
		return nil, fault(protocol.ErrorCodeInternal, "subscription generation exhausted")
	}
	generation := client.project.generation + 1
	err := client.project.Reconnect(generation)
	client.mu.Unlock()
	if err != nil {
		subscription.Close()
		return nil, err
	}
	update, err := subscription.Next(ctx)
	if err == nil {
		_, err = client.apply(ctx, generation, update)
	}
	if err != nil {
		subscription.Close()
		return nil, err
	}
	watchContext, cancel := context.WithCancel(client.context)
	client.resumeTopics()
	return &observation{subscription: subscription, context: watchContext, cancel: cancel, done: make(chan struct{}), generation: generation}, nil
}

func (client *Client) apply(ctx context.Context, generation uint64, update protocol.Update) (Apply, error) {
	client.mu.Lock()
	applied, err := client.project.Apply(generation, update)
	client.mu.Unlock()
	if err != nil {
		return applied, err
	}
	if applied == Recover {
		output, err := client.Do(ctx, protocol.Command{Kind: "snapshot"})
		if err != nil {
			return applied, err
		}
		if output.Kind != "snapshot" {
			return applied, fault(protocol.ErrorCodeInternal, "snapshot response expected")
		}
		client.mu.Lock()
		applied, err = client.project.Apply(generation, protocol.Update{Kind: "snapshot", Data: output.Data})
		client.mu.Unlock()
		if err != nil {
			return applied, err
		}
	}
	if applied != Ignored {
		client.mu.Lock()
		if client.project.snapshot != nil {
			client.feed.observe(*client.project.snapshot)
		}
		client.connected, client.err = true, nil
		client.mu.Unlock()
		client.reconcileConversations()
	}
	return applied, nil
}

func (client *Client) observe(watch *observation) {
	subscription, generation := watch.subscription, watch.generation
	defer close(watch.done)
	defer func() {
		subscription.Close()
		client.mu.Lock()
		client.connected = false
		client.mu.Unlock()
	}()
	delay := 250 * time.Millisecond
	for {
		update, err := subscription.Next(watch.context)
		if err == nil {
			var applied Apply
			applied, err = client.apply(watch.context, generation, update)
			if err == nil {
				if applied != Ignored {
					delay = 250 * time.Millisecond
					client.changed()
				}
				continue
			}
		}
		if watch.context.Err() != nil {
			return
		}
		client.mu.Lock()
		client.connected = faultCode(err) == protocol.ErrorCodeBusy && client.connected
		client.err = err
		client.mu.Unlock()
		client.changed()
		if !transient(err) {
			return
		}
		subscription.Close()
		for {
			timer := time.NewTimer(delay)
			select {
			case <-watch.context.Done():
				timer.Stop()
				return
			case <-timer.C:
			}
			reconnected, err := client.openObservation(watch.context)
			if err == nil {
				// Retain this observation's cancellation and completion ownership.
				reconnected.cancel()
				subscription, generation = reconnected.subscription, reconnected.generation
				client.changed()
				delay = 250 * time.Millisecond
				break
			}
			if watch.context.Err() != nil || errors.Is(err, context.Canceled) {
				return
			}
			client.mu.Lock()
			client.err = err
			client.mu.Unlock()
			client.changed()
			if !transient(err) {
				return
			}
			delay = min(delay*2, 5*time.Second)
		}
	}
}

func transient(err error) bool {
	code := faultCode(err)
	return code == protocol.ErrorCodeUnavailable || code == protocol.ErrorCodeBusy
}
