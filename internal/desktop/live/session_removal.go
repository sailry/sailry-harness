package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type removalKey struct {
	host protocol.NodeID
	// A zero session identifies the host's bulk removal.
	session protocol.SessionID
}

type sessionRemoval struct {
	request protocol.Request
	running bool
}

// The window owns an admitted deletion, not the transient host binding. Retiring
// a binding on the UI thread must not join the Node's deletion or its recovery.
func (r *Runtime) submitRemoval(command protocol.Command, finished func(*ui.Context, error)) bool {
	root := r.localRuntime()
	key := removalKey{host: r.controller.Target()}
	if single, ok := command.Data.(protocol.CommandRemoveSessionPayload); ok {
		key.session = single.Session
	}
	for other, pending := range root.sessionRemovals {
		if other.host != key.host || other == key {
			continue
		}
		if other.session != (protocol.SessionID{}) && key.session != (protocol.SessionID{}) {
			continue
		}
		code := protocol.ErrorCodeBusy
		if !pending.running {
			code = protocol.ErrorCodeOutcomeUnknown
		}
		root.enqueue(func(c *ui.Context) {
			finished(c, &protocol.Fault{Code: code, Message: "another session removal is unresolved"})
		})
		return false
	}
	operation := root.sessionRemovals[key]
	if operation != nil && operation.running {
		return false
	}
	if operation == nil {
		if root.sessionRemovals == nil {
			root.sessionRemovals = make(map[removalKey]*sessionRemoval)
		}
		operation = &sessionRemoval{}
		root.sessionRemovals[key] = operation
	}
	operation.running = true
	request := operation.request
	controller := r.controller
	return root.async(func() {
		var output protocol.Output
		var err error
		recovering := request.ID != (protocol.RequestID{})
		inspectionFailed := false
		if !recovering {
			request = controller.Prepare(command)
		}
		switch {
		case r != root && !controller.Connected():
			err = fileActionFault(protocol.ErrorCodeUnavailable, "resource host is offline")
			inspectionFailed = recovering
		case recovering:
			var resolution client.Resolution
			resolution, err = controller.Resolve(root.ctx, request)
			inspectionFailed = err != nil
			if err == nil {
				if resolution.State == client.RequestReplayable {
					output, err = controller.Settle(root.ctx, request)
				} else {
					output, err = resolution.Result()
				}
			}
		default:
			output, err = controller.Settle(root.ctx, request)
		}
		if inspectionFailed {
			err = fileActionFault(protocol.ErrorCodeOutcomeUnknown, "could not inspect the original session removal: "+err.Error())
		}
		var removed []protocol.SessionID
		if err == nil {
			var ok bool
			removed, ok = detail[[]protocol.SessionID](output.Data)
			if output.Kind != "sessions_removed" || !ok {
				err = fileActionFault(protocol.ErrorCodeOutcomeUnknown, "unexpected session removal response")
			}
		}
		root.enqueue(func(c *ui.Context) {
			if faultCode(err) == protocol.ErrorCodeOutcomeUnknown {
				operation.request, operation.running = request, false
			} else {
				delete(root.sessionRemovals, key)
			}
			if err == nil {
				r.forgetRemovedSessions(removed)
			}
			finished(c, err)
		})
	})
}
