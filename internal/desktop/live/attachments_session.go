package live

import (
	"encoding/json"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type attachmentSessionWaiter func(*ui.Context, protocol.Session, error)

// ensureAttachmentSession coalesces the first draft upload without sending input.
func (x *conversationActions) ensureAttachmentSession(done func(*ui.Context, protocol.Session, error)) {
	if session, ok := x.r.session(x.cv.ID); ok {
		done(x.c, session, nil)
		return
	}
	r := x.r
	before := x.cv.ID
	if r.attachmentSessionWaiters == nil {
		r.attachmentSessionWaiters = map[string][]attachmentSessionWaiter{}
	}
	if waiters, ok := r.attachmentSessionWaiters[before]; ok {
		r.attachmentSessionWaiters[before] = append(waiters, done)
		return
	}
	if start := r.pendingSessionBase(x.comp); start != nil {
		x.startSessionBase(start, func(c *ui.Context, err error) {
			if err != nil {
				done(c, protocol.Session{}, err)
				return
			}
			next := *x
			next.c = c
			next.ensureAttachmentSession(done)
		})
		return
	}
	message := composer.Message{Model: x.comp.Model, Mode: x.comp.Mode, Permission: x.comp.Permission, Features: x.comp.Features}
	if chosen := x.comp.Chosen(r.models()); chosen.Reasoning {
		message.Effort = x.comp.EffortName(x.c, chosen)
	}
	config, revision, err := r.composerConfig(x.comp, message)
	if err != nil {
		done(x.c, protocol.Session{}, err)
		return
	}
	input := protocol.CommandCreateSessionAtPayload{Target: r.controller.Target(), Config: &config, ProviderRevision: revision}
	if x.comp.Project != "" {
		id, err := protocol.ParseProjectID(x.comp.Project)
		if err != nil {
			done(x.c, protocol.Session{}, err)
			return
		}
		input.Project = &id
	}
	input.Worktree = r.draftWorktree(x.comp, input.Project)
	r.attachmentSessionWaiters[before] = []attachmentSessionWaiter{done}
	r.conversationCreating[before] = true
	r.executeScoped("create/"+before, protocol.Command{Kind: "create_session_at", Data: input}, func(c *ui.Context, out protocol.Output, err error) {
		delete(r.conversationCreating, before)
		waiters := r.attachmentSessionWaiters[before]
		delete(r.attachmentSessionWaiters, before)
		var session protocol.Session
		if err == nil {
			var ok bool
			session, ok = detail[protocol.Session](out.Data)
			if !ok {
				err = fileActionFault(protocol.ErrorCodeOutcomeUnknown, "session creation result is unavailable")
			}
		}
		if err == nil {
			r.admittedSessions[session.ID.String()] = session
			r.App.ReplaceConversationID(before, session.ID.String())
			r.browserAdopt(before, session.ID.String())
			x.cv.ID = session.ID.String()
			r.composerInitialized[x.cv.ID] = true
			r.composerRevisions[x.cv.ID] = session.Revision
			r.composerBases[x.comp] = session.Config
			key, _ := json.Marshal(session.Config)
			r.composerObserved[x.cv.ID] = string(key)
			r.project()
			if x.created != nil {
				x.created(session)
			}
		}
		for _, waiter := range waiters {
			waiter(c, session, err)
		}
	})
}
