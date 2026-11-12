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
	return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "remote session creation could not be confirmed; send the same request again to resolve it"}
}

// dispatchSessionForward admits a session for another Node and sends this
// Node's provider, secret and roles to it over the paired Link. The transfer
// runs outside the command lanes, so a slow peer delays no local command.
//
// A replay whose recorded outcome is unknown sends the import again: the target
// admits it under the same request identifier, so it answers an import it
// already holds from its own ledger and creates the session only if the
// earlier one never arrived. Its answer then replaces the unknown outcome in
// this Node's ledger.
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
		if err != nil {
			return err
		}
		unresolved := previous != nil && previous.Fault != nil && previous.Fault.Code == protocol.ErrorCodeOutcomeUnknown
		if previous != nil && !unresolved {
			return nil
		}
		forwarded, rejected = store.PrepareSessionForward(ctx, db, node.ID(), caller, request)
		if rejected == nil {
			if reserved = send != nil && node.reserveForward(request.ID); reserved {
				return nil
			}
			rejected = &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "Node operation queue is unavailable"}
		}
		if unresolved {
			return nil
		}
		return admission.Complete(ctx, db, caller, request.ID, response(protocol.Output{}, rejected))
	})
	if err != nil {
		if reserved {
			node.releaseForward(request.ID)
		}
		node.work.Done()
		return link.Admission{}, nodeFault(err)
	}
	if !reserved {
		// An unknown outcome that cannot be sent again now stays unknown.
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
		defer node.releaseForward(request.ID)
		result, answered := node.runSessionForward(send, forwarded)
		if previous == nil {
			completion <- node.settle(caller, request.ID, result)
			return
		}
		// Only the target's own answer says what became of the earlier
		// import. A send that failed here says nothing about it, and a
		// conflict means the target holds an earlier import with other
		// content: the outcome is still not known.
		if !answered || result.Fault != nil && (result.Fault.Code == protocol.ErrorCodeOutcomeUnknown || result.Fault.Code == protocol.ErrorCodeConflict) {
			completion <- *previous
			return
		}
		// The answer replaces the unknown outcome, so a later replay and an
		// inspection read it here. When it cannot be stored the row stays
		// unknown and the next replay asks the target again.
		_ = admission.Resolve(node.ctx, node.profile.Store.Node.Write, caller, request.ID, result)
		completion <- result
	}()
	return accepted, nil
}

// reserveForward claims one of the forward slots for a request that has no
// transfer in flight, so a replay never sends beside its own first attempt.
func (node *Node) reserveForward(id protocol.RequestID) bool {
	if _, running := node.forwarding.LoadOrStore(id, struct{}{}); running {
		return false
	}
	if node.forwards.Add(1) > forwardCapacity {
		node.releaseForward(id)
		return false
	}
	return true
}

func (node *Node) releaseForward(id protocol.RequestID) {
	node.forwards.Add(-1)
	node.forwarding.Delete(id)
}

// runSessionForward waits for the target's outcome and reports whether the
// target answered. Once dispatch begins, a lost response cannot establish that
// the target did not admit the import.
func (node *Node) runSessionForward(send func(context.Context, protocol.Request) (link.Admission, error), forwarded protocol.Request) (link.Response, bool) {
	ctx, cancel := context.WithTimeout(node.ctx, forwardLimit)
	defer cancel()
	accepted, err := send(ctx, forwarded)
	if err != nil {
		if ctx.Err() != nil {
			return link.Response{Fault: forwardUnknown()}, false
		}
		return response(protocol.Output{}, err), false
	}
	select {
	case result, delivered := <-accepted.Completion:
		if !delivered {
			return link.Response{Fault: forwardUnknown()}, false
		}
		return result, true
	case <-ctx.Done():
		return link.Response{Fault: forwardUnknown()}, false
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
