package live

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type removalKey struct {
	host    protocol.NodeID
	session protocol.SessionID
	batch   string
}

type sessionRemoval struct {
	request protocol.Request
	running bool
}

func (r *Runtime) removeSession(cv *viewmodel.Conversation) {
	r.removeSessions([]*viewmodel.Conversation{cv})
}

func (r *Runtime) removeSessions(targets []*viewmodel.Conversation) {
	root := r.localRuntime()
	type removalGroup struct {
		expected []protocol.SessionRevision
		err      error
	}
	groups := make(map[*Runtime]*removalGroup)
	for _, cv := range targets {
		if cv == nil {
			continue
		}
		if unsentDraft(cv) {
			root.removeDraft(cv.ID)
			continue
		}
		owner := root.runtimeFor(cv.Node)
		group := groups[owner]
		if group == nil {
			group = &removalGroup{}
			groups[owner] = group
		}
		if owner == nil {
			group.err = fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable")
			continue
		}
		id, err := protocol.ParseSessionID(cv.ID)
		if err != nil {
			group.err = err
			continue
		}
		group.expected = append(group.expected, protocol.SessionRevision{Session: id, Revision: cv.Revision})
	}
	for owner, group := range groups {
		if group.err != nil {
			root.Report(group.err)
			continue
		}
		expected := group.expected
		command := protocol.Command{Kind: "remove_sessions", Data: protocol.CommandRemoveSessionsPayload{Expected: expected}}
		if len(expected) == 1 {
			command = protocol.Command{Kind: "remove_session", Data: protocol.CommandRemoveSessionPayload{
				Session: expected[0].Session, ExpectedRevision: expected[0].Revision,
			}}
		}
		owner.submitRemoval(command, func(c *ui.Context, err error) { root.report(c, err, "session_changed") })
	}
}

// The original request retains its revisions even if a retry comes from a newer snapshot.
func sessionRemovalKey(host protocol.NodeID, command protocol.Command) (removalKey, bool) {
	key := removalKey{host: host}
	switch input := command.Data.(type) {
	case protocol.CommandRemoveSessionPayload:
		if command.Kind != "remove_session" {
			return removalKey{}, false
		}
		key.session = input.Session
	case protocol.CommandRemoveSessionsPayload:
		if command.Kind != "remove_sessions" || len(input.Expected) == 0 {
			return removalKey{}, false
		}
		if len(input.Expected) == 1 {
			key.session = input.Expected[0].Session
		} else {
			ids := make([]string, len(input.Expected))
			for i, entry := range input.Expected {
				ids[i] = entry.Session.String()
			}
			slices.Sort(ids)
			key.batch = strings.Join(ids, ",")
		}
	default:
		return removalKey{}, false
	}
	return key, key.session != (protocol.SessionID{}) || key.batch != ""
}

// The window owns an admitted deletion, not the transient host binding. Retiring
// a binding on the UI thread must not join the Node's deletion or its recovery.
func (r *Runtime) submitRemoval(command protocol.Command, finished func(*ui.Context, error)) bool {
	root := r.localRuntime()
	key, valid := sessionRemovalKey(r.controller.Target(), command)
	if !valid {
		root.enqueue(func(c *ui.Context) {
			finished(c, fileActionFault(protocol.ErrorCodeInvalidRequest, "invalid session removal targets"))
		})
		return false
	}
	for other, pending := range root.sessionRemovals {
		if other.host != key.host || other == key {
			continue
		}
		if other.batch == "" && key.batch == "" {
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
