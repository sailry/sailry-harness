package client

import (
	"context"
	"encoding/json"
	"io"
	"net"
	"strconv"
	"sync"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type ForwardState struct {
	Kind  string          `json:"kind"`
	Error *protocol.Fault `json:"error,omitempty"`
}

type ServiceSource struct {
	Session protocol.SessionID      `json:"session"`
	Command protocol.RequestID      `json:"command"`
	Service protocol.ProcessService `json:"service"`
}

func (source ServiceSource) present(items []protocol.ProcessInfo) bool {
	for _, item := range items {
		if item.ID != source.Command || item.Session != source.Session || item.Status.Kind != "running" && item.Status.Kind != "stopping" {
			continue
		}
		for _, service := range item.Services {
			if service == source.Service {
				return true
			}
		}
	}
	return false
}

type Forwarder struct {
	node      protocol.NodeID
	remote    uint16
	local     uint16
	ctx       context.Context
	cancel    context.CancelFunc
	listener  net.Listener
	done      chan struct{}
	release   func()
	mu        sync.Mutex
	next      chan struct{}
	state     ForwardState
	version   uint64
	observed  uint64
	wake      chan struct{}
	callbacks map[uint64]func()
	callback  uint64
	close     sync.Once
}

func (forwarder *Forwarder) Node() protocol.NodeID { return forwarder.node }
func (forwarder *Forwarder) RemotePort() uint16    { return forwarder.remote }
func (forwarder *Forwarder) LocalPort() uint16     { return forwarder.local }
func (forwarder *Forwarder) State() ForwardState {
	forwarder.mu.Lock()
	defer forwarder.mu.Unlock()
	return copyValue(forwarder.state)
}
func (forwarder *Forwarder) OnChange(callback func()) func() {
	forwarder.mu.Lock()
	forwarder.callback++
	token := forwarder.callback
	forwarder.callbacks[token] = callback
	forwarder.mu.Unlock()
	return func() {
		forwarder.mu.Lock()
		delete(forwarder.callbacks, token)
		forwarder.mu.Unlock()
	}
}

func (forwarder *Forwarder) Next(ctx context.Context) (json.RawMessage, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	select {
	case forwarder.next <- struct{}{}:
	default:
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-forwarder.ctx.Done():
			return nil, forwarder.ctx.Err()
		case forwarder.next <- struct{}{}:
		}
	}
	defer func() { <-forwarder.next }()
	for {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		forwarder.mu.Lock()
		if forwarder.observed < forwarder.version {
			forwarder.observed = forwarder.version
			state := forwarder.state
			forwarder.mu.Unlock()
			return json.Marshal(state)
		}
		closed := forwarder.state.Kind != "listening"
		forwarder.mu.Unlock()
		if closed {
			return nil, fault(protocol.ErrorCodeUnavailable, "port forwarding is closed")
		}
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-forwarder.wake:
		}
	}
}

func (forwarder *Forwarder) setState(state ForwardState) {
	forwarder.mu.Lock()
	forwarder.state = state
	forwarder.version++
	callbacks := make([]func(), 0, len(forwarder.callbacks))
	for _, callback := range forwarder.callbacks {
		callbacks = append(callbacks, callback)
	}
	forwarder.mu.Unlock()
	select {
	case forwarder.wake <- struct{}{}:
	default:
	}
	for _, callback := range callbacks {
		if callback != nil {
			callback()
		}
	}
}

func (forwarder *Forwarder) Close() error {
	forwarder.close.Do(func() {
		forwarder.cancel()
		_ = forwarder.listener.Close()
	})
	<-forwarder.done
	return nil
}

func (client *Client) portStream(ctx context.Context, port uint16) (link.Stream, error) {
	output, err := client.Do(ctx, protocol.Command{Kind: "open_port", Data: protocol.CommandOpenPortPayload{Port: port}})
	if err != nil {
		return nil, err
	}
	opened, err := payload[protocol.OutputPortStreamPayload](output.Data)
	if output.Kind != "port_stream" || err != nil {
		return nil, fault(protocol.ErrorCodeInternal, "port stream expected")
	}
	stream, err := client.transport.Open(ctx, opened.Stream)
	if err != nil {
		_, _ = client.Do(ctx, protocol.Command{Kind: "cancel_port", Data: protocol.CommandCancelPortPayload{Stream: opened.Stream}})
	}
	return stream, err
}

func (client *Client) ForwardPort(ctx context.Context, remotePort, localPort uint16) (*Forwarder, error) {
	subscription, err := client.transport.Subscribe(ctx, protocol.Topic{Kind: "node"})
	if err != nil {
		return nil, err
	}
	projection := NewProjection(client.Target(), 1)
	update, err := subscription.Next(ctx)
	if err == nil {
		_, err = projection.Apply(1, update)
	}
	if err != nil {
		_ = subscription.Close()
		return nil, err
	}
	stream, err := client.portStream(ctx, remotePort)
	if err != nil {
		_ = subscription.Close()
		return nil, err
	}
	_ = stream.Close()
	listener, err := (&net.ListenConfig{}).Listen(ctx, "tcp4", net.JoinHostPort("127.0.0.1", strconv.Itoa(int(localPort))))
	if err != nil {
		_ = subscription.Close()
		return nil, fault(protocol.ErrorCodeUnavailable, "local port could not be bound")
	}
	client.lifecycle.Lock()
	parent := client.context
	if parent == nil {
		parent = context.WithoutCancel(ctx)
	}
	if client.closed {
		client.lifecycle.Unlock()
		_ = listener.Close()
		_ = subscription.Close()
		return nil, fault(protocol.ErrorCodeUnavailable, "Client is closed")
	}
	owner, cancel := context.WithCancel(parent)
	forwarder := &Forwarder{
		node: client.Target(), remote: remotePort, local: uint16(listener.Addr().(*net.TCPAddr).Port),
		ctx: owner, cancel: cancel, listener: listener, done: make(chan struct{}),
		next: make(chan struct{}, 1), state: ForwardState{Kind: "listening"}, version: 1,
		wake: make(chan struct{}, 1), callbacks: map[uint64]func(){},
	}
	client.forwardMu.Lock()
	if client.forwarders == nil {
		client.forwarders = map[*Forwarder]struct{}{}
	}
	client.forwarders[forwarder] = struct{}{}
	client.forwardMu.Unlock()
	client.lifecycle.Unlock()
	forwarder.release = func() {
		client.forwardMu.Lock()
		delete(client.forwarders, forwarder)
		client.forwardMu.Unlock()
	}
	go forwarder.run(client, subscription, projection)
	return forwarder, nil
}

func (forwarder *Forwarder) run(client *Client, subscription link.Subscription, projection *Projection) {
	defer close(forwarder.done)
	defer forwarder.release()
	stopListener := context.AfterFunc(forwarder.ctx, func() { _ = forwarder.listener.Close() })
	defer stopListener()
	failures := make(chan error, 1)
	observed := make(chan struct{})
	go func() {
		defer close(observed)
		for {
			update, err := subscription.Next(forwarder.ctx)
			if err == nil {
				var applied Apply
				applied, err = projection.Apply(1, update)
				if err == nil && applied == Recover {
					var output protocol.Output
					output, err = client.Do(forwarder.ctx, protocol.Command{Kind: "snapshot"})
					if err == nil {
						_, err = projection.Apply(1, protocol.Update{Kind: "snapshot", Data: output.Data})
					}
				}
			}
			if err != nil {
				if forwarder.ctx.Err() == nil {
					failures <- err
				}
				forwarder.cancel()
				return
			}
		}
	}()
	connections := make(chan struct{}, 16)
	var pending sync.WaitGroup
	var runError error
	for forwarder.ctx.Err() == nil {
		select {
		case <-forwarder.ctx.Done():
			continue
		case connections <- struct{}{}:
		}
		connection, err := forwarder.listener.Accept()
		if err != nil {
			<-connections
			if forwarder.ctx.Err() == nil {
				runError = err
			}
			break
		}
		pending.Add(1)
		go func() {
			defer pending.Done()
			defer func() { <-connections }()
			defer connection.Close()
			stream, err := client.portStream(forwarder.ctx, forwarder.remote)
			if err != nil {
				return
			}
			defer stream.Close()
			stop := context.AfterFunc(forwarder.ctx, func() {
				_ = connection.Close()
				_ = stream.Close()
			})
			defer stop()
			copyPort(connection, stream)
		}()
	}
	forwarder.cancel()
	_ = forwarder.listener.Close()
	_ = subscription.Close()
	<-observed
	pending.Wait()
	select {
	case runError = <-failures:
	default:
	}
	state := ForwardState{Kind: "closed"}
	if runError != nil {
		state.Kind = "failed"
		state.Error = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "port forwarding stopped"}
	}
	forwarder.setState(state)
}

func copyPort(connection net.Conn, stream link.Stream) {
	results := make(chan error, 2)
	go func() {
		_, err := io.CopyBuffer(stream, connection, make([]byte, 8192))
		if err == nil {
			err = stream.CloseWrite()
		}
		results <- err
	}()
	go func() {
		_, err := io.CopyBuffer(connection, stream, make([]byte, 8192))
		if err == nil {
			err = connection.(interface{ CloseWrite() error }).CloseWrite()
		}
		results <- err
	}()
	if err := <-results; err != nil {
		_ = stream.Close()
		_ = connection.Close()
	}
	<-results
}

func (client *Client) ForwardService(ctx context.Context, encoded json.RawMessage, localPort uint16) (*Forwarder, error) {
	var source ServiceSource
	if err := json.Unmarshal(encoded, &source); err != nil {
		return nil, fault(protocol.ErrorCodeInvalidRequest, "invalid service source")
	}
	output, err := client.Do(ctx, protocol.Command{Kind: "list_commands", Data: protocol.CommandListCommandsPayload{Session: source.Session}})
	if err != nil {
		return nil, err
	}
	items, err := payload[[]protocol.ProcessInfo](output.Data)
	if output.Kind != "commands" || err != nil {
		return nil, fault(protocol.ErrorCodeInternal, "command list expected")
	}
	if !source.present(items) {
		return nil, fault(protocol.ErrorCodeNotFound, "service is no longer available")
	}
	forwarder, err := client.ForwardPort(ctx, source.Service.Port, localPort)
	if err != nil {
		return nil, err
	}
	release, err := client.ObserveCommands(forwarder.ctx, source.Session, func() {
		state, ready := client.Commands(source.Session)
		if ready && state.Connected && !source.present(state.Items) {
			forwarder.cancel()
		}
	})
	if err != nil {
		_ = forwarder.Close()
		return nil, err
	}
	context.AfterFunc(forwarder.ctx, release)
	return forwarder, nil
}

func (client *Client) closeForwards() {
	client.forwardMu.Lock()
	forwarders := make([]*Forwarder, 0, len(client.forwarders))
	for forwarder := range client.forwarders {
		forwarders = append(forwarders, forwarder)
	}
	client.forwardMu.Unlock()
	for _, forwarder := range forwarders {
		_ = forwarder.Close()
	}
}
