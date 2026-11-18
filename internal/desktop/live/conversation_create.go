package live

import (
	"encoding/json"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type conversationCreation struct {
	actions    *conversationActions
	request    protocol.Request
	turn       protocol.QueuedTurn
	team       string
	transcript bool
	dispatched bool
	finish     func(protocol.Output, bool)
}

func (r *Runtime) hasLocalProviderCredential(provider protocol.ConversationProvider) bool {
	return provider.Credential == nil || provider.Credential.Node == r.controller.Target()
}

// composerProviders are the providers owned by this execution Node, plus the
// immutable provider captured by an existing session.
func (r *Runtime) composerProviders() []protocol.ConversationProvider {
	snapshot := r.snapshot()
	providers := slices.Clone(snapshot.Providers)
	private := func(session protocol.Session) {
		if session.Profile != nil && !slices.ContainsFunc(providers, func(p protocol.ConversationProvider) bool { return p.ID == session.Profile.Provider.ID }) {
			providers = append(providers, session.Profile.Provider)
		}
	}
	for _, session := range snapshot.Sessions {
		private(session)
	}
	for _, session := range r.admittedSessions {
		private(session)
	}
	return providers
}

// offeredModels are this Node's provider models for a draft, or the captured
// provider model for an existing session.
func (r *Runtime) offeredModels(id string, models []composer.Model) []composer.Model {
	allowed := map[string]bool{}
	for _, provider := range r.snapshot().Providers {
		if r.hasLocalProviderCredential(provider) {
			allowed[provider.ID.String()] = true
		}
	}
	if session, exists := r.session(id); exists && session.Profile != nil && session.Profile.Source != r.controller.Target() {
		return nil
	} else if exists && session.Profile != nil && r.hasLocalProviderCredential(session.Profile.Provider) {
		allowed[session.Profile.Provider.ID.String()] = true
	}
	var offered []composer.Model
	for _, model := range models {
		if provider, _, _ := strings.Cut(model.ID, "/"); allowed[provider] {
			offered = append(offered, model)
		}
	}
	return offered
}

// memberModels restricts a remote session's role choices to the providers
// listed on its execution Node.
func (r *Runtime) memberModels(offered []composer.Model) []composer.Model {
	if r.parent == nil {
		return nil
	}
	own := []composer.Model{}
	for _, model := range offered {
		provider, _, _ := strings.Cut(model.ID, "/")
		if slices.ContainsFunc(r.snapshot().Providers, func(p protocol.ConversationProvider) bool { return p.ID.String() == provider }) {
			own = append(own, model)
		}
	}
	return own
}

// reportCreation toasts why a session was not created. A model that is not
// configured, as a provider the local Node no longer has or an account
// sign-in that stays with the Node it was made on, is its own failure.
func (r *Runtime) reportCreation(c *ui.Context, err error) {
	if faultCode(err) == protocol.ErrorCodeNotConfigured {
		kit.Toast(c, kit.Failed, i18n.T(c, "chat_config_unavailable"))
		return
	}
	r.report(c, err, "session_changed")
}

// start captures the first message and its session settings before an optional
// managed worktree is created. The Node admits both in one transaction.
func (x *conversationActions) start(m composer.Message, config protocol.SessionConfig, transcript bool, finish func(protocol.Output, bool)) {
	r := x.r
	if x.comp.Host != "" && x.comp.Host != r.controller.Target().String() {
		r.discardUnsubmitted(x.cv.ID)
		x.unavailable()
		return
	}
	var project *protocol.ProjectID
	if x.comp.Project != "" {
		id, err := protocol.ParseProjectID(x.comp.Project)
		if err != nil {
			r.discardUnsubmitted(x.cv.ID)
			x.unavailable()
			return
		}
		project = &id
	}
	for _, a := range m.Attachments {
		var record *attachmentRecord
		if r.attachments != nil {
			record = r.attachments.items[a]
		}
		if record == nil || record.conversation != x.cv.ID || record.ready == nil || record.ready.Spec.Worktree != (protocol.WorktreeID{}) || a.ID != record.ready.ID.String() {
			r.discardUnsubmitted(x.cv.ID)
			x.unavailable()
			return
		}
	}
	execution, err := r.composerExecution(x.comp)
	if err != nil {
		r.failCreation(x.c, x, err)
		return
	}
	input := protocol.ConversationStart{
		Project: project, Config: &config, Message: conversationInput(m), UseExplicitConfig: true,
		Execution: execution.Execution, Team: execution.Team, Models: execution.Models,
	}
	pending := &conversationCreation{actions: x, team: composerTeamKey(x.comp), transcript: transcript, finish: finish}
	begin := func() {
		input.Worktree = r.draftWorktree(x.comp, project)
		pending.request = r.controller.Prepare(protocol.Command{Kind: "start_session", Data: &input})
		r.conversationCreations[x.cv.ID] = pending
		pending.resume()
	}
	if base := r.pendingSessionBase(x.comp); base != nil {
		x.startSessionBase(base, func(c *ui.Context, err error) {
			if err != nil {
				if !r.failUnsubmitted(c, x.cv.ID, err) {
					r.reportKeys(c, err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
				}
				return
			}
			begin()
		})
		return
	}
	begin()
}

func (r *Runtime) failCreation(c *ui.Context, x *conversationActions, err error) {
	r.discardOutgoing(x.cv.ID)
	if client.Released(err) && faultCode(err) == protocol.ErrorCodeOutcomeUnknown {
		kit.Toast(c, kit.Uncertain, i18n.T(c, "chat_first_send_unconfirmed"))
		return
	}
	missingAttachment := false
	if x.restore != nil && r.attachments != nil && faultCode(err) == protocol.ErrorCodeNotFound {
		message := faultMessage(err)
		for _, a := range x.restore.Attachments {
			record := r.attachments.items[a]
			if record == nil || record.conversation != x.cv.ID || record.ready == nil ||
				record.worktree != (protocol.WorktreeID{}) || message != protocol.NewAttachmentNotFound(record.ready.ID).Message {
				continue
			}
			delete(r.attachments.files, attachmentKey(x.cv.ID, a.ID))
			record.ready = nil
			a.ID, a.Status, a.Error, a.Progress = "", composer.Failed, "files_upload_failed", 0
			missingAttachment = true
			break
		}
	}
	if x.restore != nil && x.restoreDraft != composerFingerprint(x.comp) && faultCode(err) != protocol.ErrorCodeOutcomeUnknown {
		x.offerRestore(c, err)
		return
	}
	if missingAttachment {
		r.attachmentOutcome(c, err)
		return
	}
	r.reportCreation(c, err)
}

// resume asks about an uncertain first message using its original request ID.
// Once the turn is admitted, only the session read may need to be retried.
func (p *conversationCreation) resume() {
	x := p.actions
	r, before := x.r, x.cv.ID
	if r.conversationCreating[before] {
		return
	}
	if p.dispatched && p.observe(r.snapshot()) {
		return
	}
	if p.transcript {
		x.stageOutgoing(*x.restore)
	}
	r.conversationCreating[before] = true
	if p.turn.Session != (protocol.SessionID{}) {
		p.readSession()
		return
	}
	first := !p.dispatched
	p.dispatched = true
	request := p.request
	r.async(func() {
		var output protocol.Output
		var err error
		if first {
			output, err = r.controller.Settle(r.ctx, request)
		} else {
			resolution, readErr := r.controller.Resolve(r.ctx, request)
			if readErr != nil {
				err = fileActionFault(protocol.ErrorCodeOutcomeUnknown, "first message outcome is unavailable")
			} else {
				output, err = resolution.Result()
			}
		}
		r.enqueue(func(c *ui.Context) {
			turn, ok := detail[protocol.QueuedTurn](output.Data)
			if err == nil && output.Kind == "queued_turn" && ok && turn.Request == request.ID && turn.ID != (protocol.TurnID{}) && turn.Session != (protocol.SessionID{}) {
				p.turn = turn
				p.readSession()
				return
			}
			if err == nil || faultCode(err) == protocol.ErrorCodeOutcomeUnknown {
				if p.observe(r.snapshot()) {
					return
				}
				if p.turn.Session != (protocol.SessionID{}) {
					p.readSession()
					return
				}
				if err == nil {
					err = fileActionFault(protocol.ErrorCodeOutcomeUnknown, "first message outcome is unavailable")
				}
			}
			if !client.Retained(err) {
				delete(r.conversationCreations, before)
			}
			delete(r.conversationCreating, before)
			r.failCreation(c, x, err)
		})
	})
}

func (r *Runtime) observeConversationCreations() {
	if len(r.conversationCreations) == 0 {
		return
	}
	snapshot := r.snapshot()
	for draft, pending := range r.conversationCreations {
		if pending.dispatched && !r.conversationCreating[draft] {
			pending.observe(snapshot)
		}
	}
}

// The task event or replacement snapshot proves admission by immutable request ID.
func (p *conversationCreation) observe(snapshot protocol.Snapshot) bool {
	if p.turn.Session == (protocol.SessionID{}) {
		for _, turn := range snapshot.Turns {
			if turn.Request == p.request.ID && turn.ID != (protocol.TurnID{}) && turn.Session != (protocol.SessionID{}) {
				p.turn = protocol.QueuedTurn{ID: turn.ID, Session: turn.Session, Request: turn.Request}
				break
			}
		}
	}
	if p.turn.Session != (protocol.SessionID{}) {
		for _, session := range snapshot.Sessions {
			if session.ID == p.turn.Session && session.Revision >= p.turn.Revision {
				p.adopt(session)
				return true
			}
		}
	}
	return false
}

func (p *conversationCreation) readSession() {
	x := p.actions
	r, before := x.r, x.cv.ID
	id := p.turn.Session
	if session, ok := r.session(id.String()); ok && session.Revision >= p.turn.Revision {
		p.adopt(session)
		return
	}
	r.executeScoped("open-created/"+before, protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: id}}, func(c *ui.Context, out protocol.Output, err error) {
		session, ok := detail[protocol.Session](out.Data)
		if err != nil || out.Kind != "session" || !ok || session.ID != id || session.Revision < p.turn.Revision {
			if p.observe(r.snapshot()) {
				return
			}
			delete(r.conversationCreating, before)
			switch faultCode(err) {
			case protocol.ErrorCodeNotFound, protocol.ErrorCodePermissionDenied:
				// The send was admitted. Release only its draft state, even
				// if this controller can no longer open the new conversation.
				delete(r.conversationCreations, before)
				r.discardOutgoing(before)
				p.finish(protocol.Output{}, true)
				kit.Notify(c, "", i18n.T(c, "chat_first_send_unavailable"))
				return
			}
			r.failCreation(c, x, fileActionFault(protocol.ErrorCodeOutcomeUnknown, "admitted conversation is unavailable"))
			return
		}
		p.adopt(session)
	})
}

func (p *conversationCreation) adopt(session protocol.Session) {
	x := p.actions
	r, before, after := x.r, x.cv.ID, session.ID.String()
	if p.transcript {
		x.stageOutgoing(*x.restore)
	}
	delete(r.conversationCreating, before)
	delete(r.conversationCreations, before)
	r.admittedSessions[after] = session
	r.App.ReplaceConversationID(before, after)
	if pending := r.composerOutgoing[before]; pending != nil {
		delete(r.composerOutgoing, before)
		pending.request, pending.turn = p.request.ID, p.turn.ID
		r.composerOutgoing[after] = pending
	}
	r.browserAdopt(before, after)
	x.cv.ID = after
	r.composerInitialized[after] = true
	r.composerRevisions[after] = session.Revision
	r.composerBases[x.comp] = session.Config
	key, _ := json.Marshal(session.Config)
	r.composerObserved[after] = string(key)
	if r.teams != nil {
		state := r.teams.composers[before]
		delete(r.teams.composers, before)
		if state == nil {
			state = &teamComposer{composer: x.comp}
		}
		state.revision, state.observed = session.Revision, p.team
		r.teams.composers[after] = state
	}
	r.adoptDraftAttachments(before, session, x.restore.Attachments)
	r.project()
	if x.created != nil {
		x.created(session)
	}
	p.finish(protocol.Output{Kind: "queued_turn", Data: p.turn}, true)
}
