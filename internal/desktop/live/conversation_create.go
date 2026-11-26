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
	actions       *conversationActions
	admission     <-chan protocol.TurnSummary
	stopAdmission func()
	request       protocol.Request
	turn          protocol.QueuedTurn
	team          string
	transcript    bool
	dispatched    bool
	observeOnly   bool
	checking      bool
	detached      *detachedCreationDraft
}

func (p *conversationCreation) readAdmission() {
	if p.turn.Session != (protocol.SessionID{}) {
		return
	}
	select {
	case turn := <-p.admission:
		p.turn = protocol.QueuedTurn{ID: turn.ID, Session: turn.Session, Request: turn.Request}
	default:
	}
}

func (p *conversationCreation) releaseAdmission() {
	if p.stopAdmission != nil {
		p.stopAdmission()
		p.readAdmission()
		p.stopAdmission, p.admission = nil, nil
	}
}

type detachedCreationDraft struct {
	message     *composer.Message
	fingerprint string
	attachments map[*composer.Attachment]protocol.Attachment
}

func (r *Runtime) hasLocalProviderCredential(provider protocol.ConversationProvider) bool {
	return provider.Credential == nil || provider.Credential.Node == r.controller.Target()
}

// composerProviders are the providers owned by this execution Node, plus the
// immutable provider captured by an existing session.
func (r *Runtime) composerProviders() []protocol.ConversationProvider {
	snapshot := r.snapshot()
	return slices.Concat(snapshot.Providers, r.privateComposerProviders(snapshot))
}

func (r *Runtime) privateComposerProviders(snapshot protocol.Snapshot) []protocol.ConversationProvider {
	var providers []protocol.ConversationProvider
	private := func(session protocol.Session) {
		if session.Profile == nil {
			return
		}
		provider := session.Profile.Provider
		same := func(p protocol.ConversationProvider) bool { return p.ID == provider.ID }
		if !slices.ContainsFunc(snapshot.Providers, same) && !slices.ContainsFunc(providers, same) {
			providers = append(providers, provider)
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

// Find the selection without expanding the complete picker catalog on every frame.
func (r *Runtime) composerModel(key string) (protocol.ConversationProvider, protocol.ConversationModel, bool) {
	providerKey, modelID, ok := strings.Cut(key, "/")
	if ok {
		p, found := r.composerProvider(providerKey)
		if found && p.Enabled && r.hasLocalProviderCredential(p) {
			for _, model := range p.Models {
				if model.ID == modelID {
					return p, model, true
				}
			}
		}
	}
	return protocol.ConversationProvider{}, protocol.ConversationModel{}, false
}

func (r *Runtime) composerProvider(key string) (protocol.ConversationProvider, bool) {
	snapshot := r.snapshot()
	for i := range snapshot.Providers {
		if snapshot.Providers[i].ID.String() == key {
			return snapshot.Providers[i], true
		}
	}
	for i := range snapshot.Sessions {
		profile := snapshot.Sessions[i].Profile
		if profile != nil && profile.Provider.ID.String() == key {
			return profile.Provider, true
		}
	}
	for _, session := range r.admittedSessions {
		if session.Profile != nil && session.Profile.Provider.ID.String() == key {
			return session.Profile.Provider, true
		}
	}
	return protocol.ConversationProvider{}, false
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
func (x *conversationActions) start(m composer.Message, config protocol.SessionConfig, transcript bool) {
	r := x.r
	if pending := r.guardedCreation(x.cv.ID); pending != nil {
		r.discardUnsubmitted(x.cv.ID)
		x.cv.Node = pending.request.Target.String()
		x.comp.Host = x.cv.Node
		pending.resume()
		return
	}
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
	pending := &conversationCreation{actions: x, team: composerTeamKey(x.comp), transcript: transcript}
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

func (r *Runtime) guardedCreation(draft string) *conversationCreation {
	root := r.localRuntime()
	if pending := root.conversationCreations[draft]; pending != nil && pending.observeOnly {
		return pending
	}
	for _, owner := range root.bindings {
		if pending := owner.conversationCreations[draft]; pending != nil && pending.observeOnly {
			return pending
		}
	}
	return nil
}

// The window retains the original request without retaining a closed binding or its callbacks.
func (r *Runtime) detachCreation(draft string, pending *conversationCreation) {
	pending.releaseAdmission()
	delete(r.conversationCreations, draft)
	delete(r.conversationCreating, draft)
	r.discardOutgoing(draft)
	root := r.localRuntime()
	if root.App.Data.Find(draft) == nil {
		return
	}
	x := pending.actions
	detached := &detachedCreationDraft{message: x.restore, fingerprint: x.restoreDraft}
	if r.attachments != nil {
		for card, record := range r.attachments.items {
			if record.composer != x.comp || record.conversation != draft || record.ready == nil || record.pending != nil ||
				record.running || record.removing || record.ready.Spec.Worktree != (protocol.WorktreeID{}) {
				continue
			}
			if detached.attachments == nil {
				detached.attachments = map[*composer.Attachment]protocol.Attachment{}
			}
			detached.attachments[card] = *record.ready
		}
	}
	x.comp.SetAttachmentCallbacks(nil)
	root.conversationCreations[draft] = &conversationCreation{
		request: pending.request, turn: pending.turn, team: pending.team, dispatched: true, observeOnly: true,
		detached: detached,
	}
}

func (r *Runtime) restoreDetachedCreations() {
	root := r.localRuntime()
	for draft, pending := range root.conversationCreations {
		if pending.detached == nil || pending.request.Target != r.controller.Target() {
			continue
		}
		delete(root.conversationCreations, draft)
		cv := r.App.Data.Find(draft)
		if cv == nil {
			continue
		}
		comp := r.App.Composer(draft)
		for card, ready := range pending.detached.attachments {
			r.attachments.items[card] = &attachmentRecord{composer: comp, conversation: draft, ready: &ready}
			r.registerConversationAttachment(draft, ready)
		}
		pending.actions = &conversationActions{r: r, cv: cv, comp: comp, restore: pending.detached.message, restoreDraft: pending.detached.fingerprint}
		pending.detached = nil
		r.conversationCreations[draft] = pending
		r.readsConnected = false
	}
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
		x.restoreFailedDraft(c, err)
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
	if x == nil {
		return
	}
	r, before := x.r, x.cv.ID
	if p.checking || r.conversationCreating[before] {
		return
	}
	if p.dispatched && p.observe(r.snapshot()) {
		return
	}
	p.checking = true
	if !p.observeOnly {
		if p.transcript {
			x.stageOutgoing(*x.restore)
		}
		r.conversationCreating[before] = true
	}
	if p.turn.Session != (protocol.SessionID{}) {
		p.readSession()
		return
	}
	if p.admission == nil {
		p.admission, p.stopAdmission = r.controller.WatchTaskAdmission(p.request.ID)
	}
	first := !p.dispatched
	p.dispatched = true
	request := p.request
	r.async(func() {
		var output protocol.Output
		var err error
		var rejected bool
		if first {
			output, err = r.controller.Settle(r.ctx, request)
		} else {
			resolution, readErr := r.controller.Resolve(r.ctx, request)
			if readErr != nil {
				err = fileActionFault(protocol.ErrorCodeOutcomeUnknown, "first message outcome is unavailable")
			} else {
				rejected = resolution.State == client.RequestAbsent || resolution.State == client.RequestFailed
				output, err = resolution.Result()
			}
		}
		r.enqueue(func(c *ui.Context) {
			if r.conversationCreations[before] != p {
				return
			}
			turn, ok := detail[protocol.QueuedTurn](output.Data)
			if err == nil && output.Kind == "queued_turn" && ok && turn.Request == request.ID && turn.ID != (protocol.TurnID{}) && turn.Session != (protocol.SessionID{}) {
				p.turn = turn
				p.readSession()
				return
			}
			unknown := faultCode(err) == protocol.ErrorCodeOutcomeUnknown
			if !p.observeOnly && unknown && client.Released(err) {
				p.observeOnly = true
				delete(r.conversationCreating, before)
				r.discardOutgoing(before)
			}
			if p.observeOnly || err == nil || unknown {
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
			if (!p.observeOnly || rejected) && !client.Retained(err) {
				p.releaseAdmission()
				delete(r.conversationCreations, before)
			}
			p.checking = false
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
	if p.actions == nil || snapshot.Node != p.request.Target {
		return false
	}
	p.readAdmission()
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
		if r.conversationCreations[before] != p {
			return
		}
		session, ok := detail[protocol.Session](out.Data)
		if err != nil || out.Kind != "session" || !ok || session.ID != id || session.Revision < p.turn.Revision {
			if p.observe(r.snapshot()) {
				return
			}
			p.checking = false
			delete(r.conversationCreating, before)
			switch faultCode(err) {
			case protocol.ErrorCodeNotFound, protocol.ErrorCodePermissionDenied:
				if !p.observeOnly {
					p.releaseAdmission()
					// A known admission releases its draft even if the session cannot be opened.
					delete(r.conversationCreations, before)
					r.discardOutgoing(before)
					x.finishSubmission(protocol.Output{}, true, true)
				}
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
	p.releaseAdmission()
	x := p.actions
	r, before, after := x.r, x.cv.ID, session.ID.String()
	if p.observeOnly {
		x.cv.Node = p.request.Target.String()
		x.comp.Host = x.cv.Node
	} else if p.transcript {
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
	r.setComposerBase(x.comp, session.Config)
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
	x.finishSubmission(protocol.Output{Kind: "queued_turn", Data: p.turn}, true, true)
}
