package node

import (
	"context"
	"database/sql"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	forwardCapacity = 128
	forwardLimit    = 30 * time.Second
)

func forwardUnknown() *protocol.Fault {
	return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "remote session creation could not be confirmed; automatic replay is disabled"}
}

// dispatchSessionForward admits a session for another Node and sends this
// Node's provider, secret and roles to it over the paired Link. The transfer
// runs outside the command lanes, so a slow peer delays no local command.
func (node *Node) dispatchSessionForward(ctx context.Context, caller protocol.NodeID, request protocol.Request, body []byte) (link.Admission, error) {
	if err := node.begin(); err != nil {
		return link.Admission{}, err
	}
	completion := make(chan link.Response, 1)
	accepted := link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: true}, Completion: completion}
	send := node.forward
	if send == nil && node.network != nil {
		send = node.sendSessionForward
	}
	var previous *link.Response
	var forwarded protocol.Request
	var rejected error
	reserved := false
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		previous, err = admission.Admit(ctx, db, caller, request, body)
		if err != nil || previous != nil {
			return err
		}
		forwarded, rejected = store.PrepareSessionForward(ctx, db, node.ID(), caller, request)
		if rejected == nil {
			if reserved = send != nil && node.forwards.Add(1) <= forwardCapacity; reserved {
				return nil
			}
			if send != nil {
				node.forwards.Add(-1)
			}
			rejected = &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "Node operation queue is unavailable"}
		}
		return admission.Complete(ctx, db, caller, request.ID, response(protocol.Output{}, rejected))
	})
	if err != nil {
		if reserved {
			node.forwards.Add(-1)
		}
		node.work.Done()
		return link.Admission{}, nodeFault(err)
	}
	if previous != nil || rejected != nil {
		if previous == nil {
			result := response(protocol.Output{}, rejected)
			previous = &result
		}
		completion <- *previous
		close(completion)
		node.work.Done()
		return accepted, nil
	}
	// Admission is durable. Cancellation of its observer cannot cancel the transfer.
	go func() {
		defer node.work.Done()
		defer close(completion)
		defer node.forwards.Add(-1)
		result := node.runSessionForward(send, forwarded)
		err := node.profile.Store.Node.Write(node.ctx, func(db *sql.Conn) error {
			return admission.Complete(node.ctx, db, caller, request.ID, result)
		})
		if err != nil && (result.Fault == nil || result.Fault.Code != protocol.ErrorCodeOutcomeUnknown) {
			result = response(protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "operation result could not be recorded; inspect the target before retrying"})
		}
		completion <- result
	}()
	return accepted, nil
}

// runSessionForward waits for the target's outcome. Once dispatch begins, a
// lost response cannot establish that the target did not admit the import.
func (node *Node) runSessionForward(send func(context.Context, protocol.Request) (link.Admission, error), forwarded protocol.Request) link.Response {
	ctx, cancel := context.WithTimeout(node.ctx, forwardLimit)
	defer cancel()
	accepted, err := send(ctx, forwarded)
	if err != nil {
		if ctx.Err() != nil {
			return link.Response{Fault: forwardUnknown()}
		}
		return response(protocol.Output{}, err)
	}
	select {
	case result, delivered := <-accepted.Completion:
		if !delivered {
			return link.Response{Fault: forwardUnknown()}
		}
		return result
	case <-ctx.Done():
		return link.Response{Fault: forwardUnknown()}
	}
}

func (node *Node) sendSessionForward(ctx context.Context, forwarded protocol.Request) (link.Admission, error) {
	peers, err := node.network.Peers(ctx)
	if err != nil {
		return link.Admission{}, err
	}
	for _, address := range peers {
		if protocol.NodeID(address.ID) == forwarded.Target {
			accepted, err := node.network.Remote(address).Dispatch(ctx, forwarded)
			if err != nil {
				return link.Admission{}, forwardUnknown()
			}
			return accepted, nil
		}
	}
	return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "target Node is not paired"}
}
