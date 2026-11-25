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
	hidden  map[protocol.SessionID]bool
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
func sessionRemovalTargets(host protocol.NodeID, command protocol.Command) (removalKey, []protocol.SessionID) {
	key := removalKey{host: host}
	var targets []protocol.SessionID
	switch input := command.Data.(type) {
	case protocol.CommandRemoveSessionPayload:
		if command.Kind != "remove_session" {
			return removalKey{}, nil
		}
		targets = []protocol.SessionID{input.Session}
	case protocol.CommandRemoveSessionsPayload:
		if command.Kind != "remove_sessions" || len(input.Expected) == 0 {
			return removalKey{}, nil
		}
		targets = make([]protocol.SessionID, len(input.Expected))
		for i, entry := range input.Expected {
			targets[i] = entry.Session
		}
	default:
		return removalKey{}, nil
	}
	if len(targets) == 1 {
		key.session = targets[0]
		if key.session == (protocol.SessionID{}) {
			return removalKey{}, nil
		}
	} else {
		ids := make([]string, len(targets))
		for i, id := range targets {
			ids[i] = id.String()
		}
		slices.Sort(ids)
		key.batch = strings.Join(ids, ",")
	}
	return key, targets
}

func (r *Runtime) removalTree(targets []protocol.SessionID) map[protocol.SessionID]bool {
	children := map[protocol.SessionID][]protocol.SessionID{}
	add := func(session protocol.Session) {
		if session.Delegation != nil && *session.Delegation != nil {
			parent := (*session.Delegation).Session
			children[parent] = append(children[parent], session.ID)
		}
	}
	for _, session := range r.snapshot().Sessions {
		add(session)
	}
	for _, session := range r.admittedSessions {
		add(session)
	}
	hidden := make(map[protocol.SessionID]bool, len(targets))
	queue := slices.Clone(targets)
	for i := 0; i < len(queue); i++ {
		id := queue[i]
		if hidden[id] {
			continue
		}
		hidden[id] = true
		queue = append(queue, children[id]...)
	}
	return hidden
}

// visibleSessions is the host's read-only session list for presentation.
func (r *Runtime) visibleSessions() []protocol.Session {
	return r.filterRemovedSessions(r.snapshot().Sessions)
}

func (r *Runtime) filterRemovedSessions(sessions []protocol.Session) []protocol.Session {
	root := r.localRuntime()
	if len(root.sessionRemovals) == 0 && len(root.confirmedRemovals[r.controller.Target()]) == 0 {
		return sessions
	}
	hidden := func(session protocol.Session) bool { return r.sessionHidden(session.ID) }
	if !slices.ContainsFunc(sessions, hidden) {
		return sessions
	}
	return slices.DeleteFunc(slices.Clone(sessions), hidden)
}

func (r *Runtime) sessionHidden(id protocol.SessionID) bool {
	root, host := r.localRuntime(), r.controller.Target()
	if root.confirmedRemovals[host][id] {
		return true
	}
	for key, pending := range root.sessionRemovals {
		if key.host == host && pending.hidden[id] {
			return true
		}
	}
	return false
}

// A receipt can get here before the subscription, even across a host binding
// change. Hold the confirmed IDs until the active Client no longer lists them.
func (r *Runtime) recordSessionRemovals(ids []protocol.SessionID) {
	if len(ids) == 0 {
		return
	}
	root, host := r.localRuntime(), r.controller.Target()
	if root.confirmedRemovals == nil {
		root.confirmedRemovals = make(map[protocol.NodeID]map[protocol.SessionID]bool)
	}
	confirmed := root.confirmedRemovals[host]
	if confirmed == nil {
		confirmed = make(map[protocol.SessionID]bool, len(ids))
		root.confirmedRemovals[host] = confirmed
	}
	active := root.runtimeFor(host.String())
	for _, id := range ids {
		confirmed[id] = true
		delete(r.admittedSessions, id.String())
		if active != nil && active != r {
			delete(active.admittedSessions, id.String())
		}
	}
}

func (r *Runtime) reconcileSessionRemovals(sessions []protocol.Session) []protocol.SessionID {
	removed, next := r.controller.RemovedSessions(r.removedRead)
	r.removedRead = next
	r.recordSessionRemovals(removed)
	confirmed := r.localRuntime().confirmedRemovals
	host := r.controller.Target()
	if hidden := confirmed[host]; len(hidden) != 0 && r.controller.Connected() {
		listed := make(map[protocol.SessionID]bool, len(hidden))
		for _, session := range sessions {
			if hidden[session.ID] {
				listed[session.ID] = true
			}
		}
		if len(listed) == 0 {
			delete(confirmed, host)
		} else {
			confirmed[host] = listed
		}
	}
	return removed
}

// The window owns an admitted deletion, not the transient host binding. Retiring
// a binding on the UI thread must not join the Node's deletion or its recovery.
func (r *Runtime) submitRemoval(command protocol.Command, finished func(*ui.Context, error)) bool {
	root := r.localRuntime()
	key, targets := sessionRemovalTargets(r.controller.Target(), command)
	if len(targets) == 0 {
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
		if pending.hidden == nil {
			code = protocol.ErrorCodeOutcomeUnknown
		}
		root.enqueue(func(c *ui.Context) {
			finished(c, &protocol.Fault{Code: code, Message: "another session removal is unresolved"})
		})
		return false
	}
	operation := root.sessionRemovals[key]
	if operation != nil && operation.hidden != nil {
		return false
	}
	controller := r.controller
	if r != root && !controller.Connected() {
		var err error = resourceOfflineFault()
		if operation != nil {
			err = fileActionFault(protocol.ErrorCodeOutcomeUnknown, "could not inspect the original session removal: resource host is offline")
		}
		root.enqueue(func(c *ui.Context) { finished(c, err) })
		return false
	}
	if operation == nil {
		operation = &sessionRemoval{}
	}
	request := operation.request
	recovering := request.ID != (protocol.RequestID{})
	if !recovering {
		request = controller.Prepare(command)
	}
	if !root.async(func() {
		var output protocol.Output
		var err error
		inspectionFailed := false
		switch {
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
			if output.Kind != "sessions_removed" || !ok || len(removed) == 0 {
				err = fileActionFault(protocol.ErrorCodeOutcomeUnknown, "unexpected session removal response")
			}
		}
		root.enqueue(func(c *ui.Context) {
			if err == nil {
				r.recordSessionRemovals(removed)
			}
			if faultCode(err) == protocol.ErrorCodeOutcomeUnknown {
				operation.hidden = nil
			} else {
				delete(root.sessionRemovals, key)
			}
			root.project()
			if err == nil {
				r.forgetRemovedSessions(removed)
			}
			finished(c, err)
		})
	}) {
		return false
	}
	if root.sessionRemovals == nil {
		root.sessionRemovals = make(map[removalKey]*sessionRemoval)
	}
	operation.request, operation.hidden = request, r.removalTree(targets)
	root.sessionRemovals[key] = operation
	root.changed()
	return true
}
