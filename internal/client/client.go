// Package client owns ordered Node projections and controller request recovery.
package client

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"sync"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type Client struct {
	detailSlots             chan struct{}
	commandOutputs          map[commandOutputKey]*commandOutputWatch
	commandsMu              sync.Mutex
	commands                map[protocol.SessionID]*commandsWatch
	commandOwners           sync.WaitGroup
	workspaceMu             sync.Mutex
	workspaces              map[workspaceKey]*workspaceWatch
	workspaceOwners         sync.WaitGroup
	transferClock           transferClock
	conversationDetails     map[string]conversationDetail
	conversationDetailReads map[string]*conversationDetailRead
	conversationDetailOrder []string
	conversationDetailBytes uint64
	transport               link.Transport
	mu                      sync.RWMutex
	project                 *Projection
	feed                    activityFeed
	connected               bool
	err                     error
	callbacks               map[uint64]func()
	callback                uint64

	lifecycle          sync.Mutex
	context            context.Context
	stop               context.CancelFunc
	watch              *observation
	closed             bool
	conversationMu     sync.Mutex
	conversations      map[protocol.SessionID]*conversationWatch
	conversationOwners sync.WaitGroup
}

func New(transport link.Transport) *Client {
	return &Client{detailSlots: make(chan struct{}, 4), transferClock: systemTransferClock{}, transport: transport, project: NewProjection(transport.Target(), 0), callbacks: make(map[uint64]func())}
}

func (client *Client) Target() protocol.NodeID { return client.transport.Target() }

// Prepare captures the identity that must be retained after an uncertain result.
func (client *Client) Prepare(command protocol.Command) protocol.Request {
	return protocol.NewRequest(client.Target(), copyValue(command))
}

func (client *Client) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	if request.Target != client.Target() {
		return link.Admission{}, fault(protocol.ErrorCodeWrongTarget, "request belongs to another Node")
	}
	return client.transport.Dispatch(ctx, request)
}

// Execute never resubmits a command when observation of its result is lost.
func (client *Client) Execute(ctx context.Context, request protocol.Request) (protocol.Output, error) {
	admission, err := client.Dispatch(ctx, request)
	if err != nil {
		return protocol.Output{}, err
	}
	select {
	case response, ok := <-admission.Completion:
		if !ok {
			return protocol.Output{}, unknownCompletion()
		}
		if response.Fault != nil {
			return protocol.Output{}, &link.FaultError{Fault: *response.Fault}
		}
		return response.Output, nil
	case <-ctx.Done():
		return protocol.Output{}, unknownCompletion()
	}
}

func (client *Client) Do(ctx context.Context, command protocol.Command) (protocol.Output, error) {
	return client.Execute(ctx, client.Prepare(command))
}

// Outcome inspects a captured durable request without replaying its command.
func (client *Client) Outcome(ctx context.Context, request protocol.Request) (protocol.RequestOutcome, error) {
	if request.Target != client.Target() || request.Version != protocol.VERSION {
		return protocol.RequestOutcome{}, fault(protocol.ErrorCodeWrongTarget, "Node or protocol version mismatch")
	}
	if !request.Command.Durable() {
		return protocol.RequestOutcome{}, fault(protocol.ErrorCodeInvalidRequest, "only durable commands have a stored outcome")
	}
	body, err := json.Marshal(request)
	if err != nil {
		return protocol.RequestOutcome{}, fault(protocol.ErrorCodeInvalidRequest, "request could not be encoded")
	}
	digest := blake3.Sum256(body)
	output, err := client.Do(ctx, protocol.Command{Kind: "inspect_request", Data: protocol.CommandInspectRequestPayload{ID: request.ID, Digest: hex.EncodeToString(digest[:])}})
	if err != nil {
		return protocol.RequestOutcome{}, err
	}
	result, err := payload[protocol.OutputRequestOutcomePayload](output.Data)
	if output.Kind != "request_outcome" || err != nil || result.ID != request.ID {
		return protocol.RequestOutcome{}, fault(protocol.ErrorCodeInternal, "request outcome response does not match the query")
	}
	return result.Outcome, nil
}

// Snapshot returns an independent read copy, including nested pointers and maps.
func (client *Client) Snapshot() protocol.Snapshot {
	client.mu.RLock()
	defer client.mu.RUnlock()
	if client.project.snapshot == nil {
		state := protocol.DefaultSnapshot()
		state.Node = client.Target()
		return state
	}
	return copyValue(*client.project.snapshot)
}

func (client *Client) Connected() bool {
	client.mu.RLock()
	defer client.mu.RUnlock()
	return client.connected
}

func (client *Client) Err() error {
	client.mu.RLock()
	defer client.mu.RUnlock()
	return client.err
}

// OnChange callbacks schedule presentation updates; they run outside Client locks.
func (client *Client) OnChange(callback func()) func() {
	client.mu.Lock()
	client.callback++
	id := client.callback
	client.callbacks[id] = callback
	client.mu.Unlock()
	return func() {
		client.mu.Lock()
		delete(client.callbacks, id)
		client.mu.Unlock()
	}
}

func (client *Client) changed() {
	client.mu.RLock()
	callbacks := make([]func(), 0, len(client.callbacks))
	for _, callback := range client.callbacks {
		callbacks = append(callbacks, callback)
	}
	client.mu.RUnlock()
	for _, callback := range callbacks {
		callback()
	}
}

func fault(code protocol.ErrorCode, message string) error {
	return &link.FaultError{Fault: protocol.Fault{Code: code, Message: message}}
}

func unknownCompletion() error {
	return fault(protocol.ErrorCodeOutcomeUnknown, "completion unavailable; query using the same request identifier")
}

func faultCode(err error) protocol.ErrorCode {
	var failed *link.FaultError
	if errors.As(err, &failed) {
		return failed.Fault.Code
	}
	var direct protocol.Fault
	if errors.As(err, &direct) {
		return direct.Code
	}
	var pointer *protocol.Fault
	if errors.As(err, &pointer) {
		return pointer.Code
	}
	return protocol.ErrorCodeInternal
}

func payload[T any](data any) (T, error) {
	switch data := data.(type) {
	case T:
		return data, nil
	case *T:
		if data != nil {
			return *data, nil
		}
	}
	var zero T
	return zero, fault(protocol.ErrorCodeInvalidRequest, "update payload does not match its kind")
}
